use serde::{Deserialize, Serialize};

use solana_transaction_status_client_types::{EncodedTransaction, TransactionDetails, UiMessage, UiTransactionEncoding};
#[cfg(target_arch = "wasm32")]
use zela_std::rpc_client::{RpcClient, RpcBlockConfig};
#[cfg(not(target_arch = "wasm32"))]
use solana_client::{
	rpc_config::RpcBlockConfig,
	nonblocking::rpc_client::RpcClient
};

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Input {
	Latest {
		block_count: usize
	},
	Specific {
		blocks: Vec<u64>
	}
}

#[derive(Serialize, Debug)]
pub struct Output {
	/// Total number of transactions scanned.
	total_transactions: usize,
	/// Number of transaction skipped because they are voting.
	vote_transactions: usize,
	/// Latest processed block.
	latest_block: u64,
	// Average priority fees paid per non-voting transactions
	average_priority_fee_lamports: u64
}

pub struct PriorityFees;
impl PriorityFees {
	/// Divisor for computing a margin when fetching blocks
	const BLOCK_COUNT_SLOT_MARGIN_DIV: usize = 10;
	/// Base fee every transactions pays.
	const BASE_FEE: u64 = 5000;
	const VOTE_ACCOUNT: &'static str = "Vote111111111111111111111111111111111111111";

	/// Selects blocks according to input and returns their slot numbers.
	async fn select_blocks(p: Input, rpc: &RpcClient) -> Result<impl Iterator<Item = u64>, String> {
		let block_count = match p {
			// we apply skip here to match the types
			Input::Specific { blocks } => return Ok(blocks.into_iter().skip(0)),
			Input::Latest { block_count } => block_count,
		};

		// start off with some latest slot number - it doesn't need to be the absolute latest,
		// just needs to be close (at our commitment level)
		let latest_slot = rpc.get_slot().await.map_err(|e| e.to_string())?;

		// find slot numbers of lastest block_count blocks
		let mut block_slots = Vec::<u64>::new();
		while block_slots.len() < block_count {
			// we compute an approximate start slot to start the listing at
			// we don't know how many slots were skipped, so we estimate with respect to block_count
			let start_slot = latest_slot.saturating_sub(
				(block_count + block_count / Self::BLOCK_COUNT_SLOT_MARGIN_DIV + 1) as u64
			);
			block_slots = rpc.get_blocks_with_commitment(
				start_slot,
				None,
				rpc.commitment()
			).await.map_err(|e| e.to_string())?;
			log::trace!("get_blocks({start_slot}..) = {}", block_slots.len());
		}
		log::info!("Got {} lestest blocks: {:?}", block_slots.len(), block_slots);

		let to_skip = block_slots.len() - block_count;

		Ok(block_slots.into_iter().skip(to_skip))
	}

	pub async fn run(p: Input, rpc: &RpcClient) -> Result<Output, String> {
		log::debug!("run({p:?})");

		let mut total_fees: u64 = 0;
		let mut nonvote_count: usize = 0;
		let mut total_count: usize = 0;
		let mut latest_block: u64 = 0;

		for slot in Self::select_blocks(p, rpc).await? {
			log::debug!("Processing block {slot}");
			let block = rpc.get_block_with_config(
				slot,
				RpcBlockConfig {
					encoding: Some(UiTransactionEncoding::Json),
					transaction_details: Some(TransactionDetails::Full),
					rewards: None,
					commitment: Some(rpc.commitment()),
					max_supported_transaction_version: Some(0),
				}
			).await.map_err(|e| e.to_string())?;
			let transactions = match block.transactions {
				Some(t) => t,
				None => {
					log::error!("Transactions not found (block={})", slot);
					continue;
				}
			};
			total_count += transactions.len();
			latest_block = slot;

			for (i, transaction) in transactions.into_iter().enumerate() {
				log::trace!("transaction: {transaction:#?}");

				let is_voting = match transaction.transaction {
					EncodedTransaction::Json(t) => match t.message {
						UiMessage::Parsed(m) => m.account_keys.iter().any(|k| k.pubkey == Self::VOTE_ACCOUNT),
						UiMessage::Raw(m) => m.account_keys.iter().any(|k| k == Self::VOTE_ACCOUNT)
					}
					_ => {
						log::error!("Transaction account keys not found (block={}, idx={})", slot, i);
						continue;
					}
				};
				// skip voting transactions
				if is_voting {
					continue;
				}

				let priority_fee = match transaction.meta {
					Some(m) if m.fee < Self::BASE_FEE => {
						log::error!("Transaction fee less than base fee (block={}, idx={})", slot, i);
						continue;
					}
					Some(m) => m.fee - Self::BASE_FEE,
					None => {
						log::error!("Transaction fee not found (block={}, idx={})", slot, i);
						continue;
					}
				};

				total_fees += priority_fee;
				nonvote_count += 1;
			}
		}

		Ok(Output {
			total_transactions: total_count,
			vote_transactions: total_count - nonvote_count,
			latest_block,
			average_priority_fee_lamports: total_fees / (nonvote_count as u64),
		})
	}
}

#[cfg(target_arch = "wasm32")]
mod zela {
	use zela_std::{zela_custom_procedure, CustomProcedure, RpcError};

	use super::*;

	impl CustomProcedure for PriorityFees {
		type Params = Input;
		type ErrorData = ();
		type SuccessData = Output;

		// Run method is the entry point of every custom procedure
		// It will be called once for each incoming request.
		async fn run(params: Self::Params) -> Result<Self::SuccessData, RpcError<Self::ErrorData>> {
			let rpc = RpcClient::new();

			match Self::run(params, &rpc).await {
				Ok(v) => Ok(v),
				Err(err) => Err(RpcError {
					code: 1,
					message: err,
					data: None
				})
			}
		}

		const LOG_MAX_LEVEL: log::LevelFilter = log::LevelFilter::Debug;
	}
	zela_custom_procedure!(PriorityFees);
}

#[cfg(test)]
mod tests {
	use super::*;

	use solana_client::nonblocking::rpc_client::RpcClient;
	use solana_sdk::commitment_config::CommitmentConfig;

	#[tokio::test]
	async fn test_procedure_local() {
		env_logger::builder()
			.is_test(true)
			.parse_env(env_logger::Env::new().default_filter_or("info,priority_fees=debug"))
			.init();

		let rpc = RpcClient::new_with_commitment(
			"https://api.mainnet-beta.solana.com".to_string(),
			CommitmentConfig::confirmed(),
		);

		let out = PriorityFees::run(Input::Latest {
			block_count: 1
		}, &rpc).await.unwrap();
		log::warn!("Test output: {out:?}");
	}
}
