use log::info;
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use zela_std::rpc_client::{RpcClient, response::RpcContactInfo};
use zela_std::{CustomProcedure, JsonValue, RpcError};

const ERROR_CODE_INTERNAL: i32 = 500;
const UNKNOWN_GEO: &str = "UNKNOWN";
const LEADER_GEO_MAP_BIN: &[u8] = include_bytes!("../data/leader_geo_map.bin");
const LEADER_GEO_RECORD_SIZE: usize = 33;

pub struct LeaderRoutingProcedure;

#[derive(Debug, Clone, Serialize)]
pub struct LeaderRoutingOutput {
    pub slot: u64,
    pub leader: String,
    pub leader_geo: String,
    pub closest_region: ServerRegion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ServerRegion {
    #[serde(rename = "Dubai")]
    Dubai,
    #[serde(rename = "Frankfurt")]
    Frankfurt,
    #[serde(rename = "NewYork")]
    NewYork,
    #[serde(rename = "Tokyo")]
    Tokyo,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcedureErrorData {
    pub stage: &'static str,
    pub details: String,
}

impl CustomProcedure for LeaderRoutingProcedure {
    type Params = JsonValue;
    type SuccessData = LeaderRoutingOutput;
    type ErrorData = ProcedureErrorData;

    async fn run(_params: Self::Params) -> Result<Self::SuccessData, RpcError<Self::ErrorData>> {
        let rpc = RpcClient::new();

        let slot = rpc.get_slot().await.map_err(|err| {
            internal_error("get_slot", format!("failed to fetch current slot: {err}"))
        })?;

        let leader = rpc
            .get_slot_leaders(slot, 1)
            .await
            .map_err(|err| {
                internal_error(
                    "get_slot_leaders",
                    format!("failed to fetch current slot leader for slot {slot}: {err}"),
                )
            })?
            .into_iter()
            .next()
            .map(|pubkey| pubkey.to_string())
            .ok_or_else(|| {
                internal_error(
                    "resolve_leader",
                    format!("no leader returned for slot {slot}"),
                )
            })?;

        let leader_geo = lookup_leader_geo(&leader)
            .unwrap_or(UNKNOWN_GEO)
            .to_string();
        let closest_region = choose_region(&leader_geo, &leader);

        info!(
            "slot={slot} leader={leader} leader_geo={} closest_region={closest_region:?}",
            leader_geo
        );

        Ok(LeaderRoutingOutput {
            slot,
            leader,
            leader_geo,
            closest_region,
        })
    }
}

fn internal_error(stage: &'static str, details: String) -> RpcError<ProcedureErrorData> {
    RpcError {
        code: ERROR_CODE_INTERNAL,
        message: "leader routing procedure failed".to_string(),
        data: Some(ProcedureErrorData { stage, details }),
    }
}

pub async fn current_leader_ip() -> Result<Option<IpAddr>, RpcError<ProcedureErrorData>> {
    let rpc = RpcClient::new();
    current_leader_ip_with_client(&rpc).await
}

async fn current_leader_ip_with_client(
    rpc: &RpcClient,
) -> Result<Option<IpAddr>, RpcError<ProcedureErrorData>> {
    let slot = rpc.get_slot().await.map_err(|err| {
        internal_error(
            "get_slot_for_leader_ip",
            format!("failed to fetch current slot for leader ip lookup: {err}"),
        )
    })?;

    let leader_pubkey = rpc
        .get_slot_leaders(slot, 1)
        .await
        .map_err(|err| {
            internal_error(
                "get_slot_leaders",
                format!("failed to fetch slot leader for slot {slot}: {err}"),
            )
        })?
        .into_iter()
        .next()
        .map(|pubkey| pubkey.to_string());

    let Some(leader_pubkey) = leader_pubkey else {
        return Ok(None);
    };

    let cluster_nodes = rpc.get_cluster_nodes().await.map_err(|err| {
        internal_error(
            "get_cluster_nodes",
            format!("failed to fetch cluster nodes for leader ip lookup: {err}"),
        )
    })?;

    Ok(cluster_nodes
        .iter()
        .find(|node| node.pubkey == leader_pubkey)
        .and_then(preferred_contact_addr)
        .map(|addr| addr.ip()))
}

fn preferred_contact_addr(contact: &RpcContactInfo) -> Option<SocketAddr> {
    contact
        .tpu_quic
        .or(contact.tpu)
        .or(contact.gossip)
        .or(contact.rpc)
}

fn lookup_leader_geo(leader_pubkey: &str) -> Option<&'static str> {
    lookup_leader_geo_in_map(LEADER_GEO_MAP_BIN, leader_pubkey)
}

fn lookup_leader_geo_in_map(geo_map: &[u8], leader_pubkey: &str) -> Option<&'static str> {
    let leader_pubkey = decode_leader_pubkey(leader_pubkey)?;
    let bucket = lookup_geo_bucket(geo_map, &leader_pubkey)?;
    geo_bucket_to_label(bucket)
}

fn decode_leader_pubkey(leader_pubkey: &str) -> Option<[u8; 32]> {
    let decoded = bs58::decode(leader_pubkey).into_vec().ok()?;
    if decoded.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&decoded);
    Some(bytes)
}

fn lookup_geo_bucket(geo_map: &[u8], leader_pubkey: &[u8; 32]) -> Option<u8> {
    if !geo_map.len().is_multiple_of(LEADER_GEO_RECORD_SIZE) {
        return None;
    }

    let mut left = 0usize;
    let mut right = geo_map.len() / LEADER_GEO_RECORD_SIZE;

    while left < right {
        let mid = left + (right - left) / 2;
        let offset = mid * LEADER_GEO_RECORD_SIZE;
        let key = &geo_map[offset..offset + 32];

        match key.cmp(leader_pubkey) {
            std::cmp::Ordering::Less => left = mid + 1,
            std::cmp::Ordering::Greater => right = mid,
            std::cmp::Ordering::Equal => return geo_map.get(offset + 32).copied(),
        }
    }

    None
}

fn geo_bucket_to_label(bucket: u8) -> Option<&'static str> {
    match bucket {
        1 => Some("EU"),
        2 => Some("NA"),
        3 => Some("APAC"),
        4 => Some("ME"),
        _ => None,
    }
}

fn choose_region(leader_geo: &str, leader_pubkey: &str) -> ServerRegion {
    region_from_geo(leader_geo).unwrap_or_else(|| fallback_region(leader_pubkey))
}

fn region_from_geo(leader_geo: &str) -> Option<ServerRegion> {
    match leader_geo.trim().to_ascii_uppercase().as_str() {
        "EU" | "DE" | "FR" | "NL" | "GB" | "CH" | "SE" | "NO" | "PL" | "ES" | "IT" => {
            Some(ServerRegion::Frankfurt)
        }
        "ME" | "AE" | "SA" | "IL" | "TR" | "QA" | "BH" | "OM" | "KW" => Some(ServerRegion::Dubai),
        "NA" | "US" | "CA" | "MX" => Some(ServerRegion::NewYork),
        "APAC" | "JP" | "KR" | "SG" | "HK" | "TW" | "IN" | "AU" | "NZ" => Some(ServerRegion::Tokyo),
        _ => None,
    }
}

fn fallback_region(leader_pubkey: &str) -> ServerRegion {
    match fnv1a64(leader_pubkey.as_bytes()) % 4 {
        0 => ServerRegion::Dubai,
        1 => ServerRegion::Frankfurt,
        2 => ServerRegion::NewYork,
        _ => ServerRegion::Tokyo,
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

zela_std::zela_custom_procedure!(LeaderRoutingProcedure);

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn region_mapping_works() {
        assert_eq!(region_from_geo("EU"), Some(ServerRegion::Frankfurt));
        assert_eq!(region_from_geo("ae"), Some(ServerRegion::Dubai));
        assert_eq!(region_from_geo("us"), Some(ServerRegion::NewYork));
        assert_eq!(region_from_geo("JP"), Some(ServerRegion::Tokyo));
        assert_eq!(region_from_geo("unknown"), None);
    }

    #[test]
    fn fallback_region_is_deterministic() {
        let leader = "SomeLeaderPubkey111111111111111111111111111";
        let first = fallback_region(leader);
        let second = fallback_region(leader);
        assert_eq!(first, second);
    }

    #[test]
    fn choose_region_uses_geo_then_fallback() {
        assert_eq!(choose_region("EU", "x"), ServerRegion::Frankfurt);

        let from_unknown_geo = choose_region(UNKNOWN_GEO, "validator-x");
        let deterministic_again = choose_region(UNKNOWN_GEO, "validator-x");
        assert_eq!(from_unknown_geo, deterministic_again);
    }

    #[test]
    fn preferred_contact_addr_prioritizes_transport_addresses() {
        let tpu_quic = socket(1000);
        let tpu = socket(2000);
        let gossip = socket(3000);
        let rpc = socket(4000);

        let contact = contact_info(Some(tpu_quic), Some(tpu), Some(gossip), Some(rpc));
        assert_eq!(preferred_contact_addr(&contact), Some(tpu_quic));

        let contact = contact_info(None, Some(tpu), Some(gossip), Some(rpc));
        assert_eq!(preferred_contact_addr(&contact), Some(tpu));

        let contact = contact_info(None, None, Some(gossip), Some(rpc));
        assert_eq!(preferred_contact_addr(&contact), Some(gossip));

        let contact = contact_info(None, None, None, Some(rpc));
        assert_eq!(preferred_contact_addr(&contact), Some(rpc));

        let contact = contact_info(None, None, None, None);
        assert_eq!(preferred_contact_addr(&contact), None);
    }

    #[test]
    fn lookup_geo_bucket_uses_binary_search() {
        let geo_map = build_geo_map(&[
            ("7XSXtg2CWwjWCa7j4kXfYLMi8xawJbq6XW6xMa6Y5P9Q", 1),
            ("2jXy799ynN5A6xM4mT2QPY2ATqNnSboP8Gr3HdWu3UwR", 2),
            ("9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv", 3),
        ]);

        let key = decode_leader_pubkey("2jXy799ynN5A6xM4mT2QPY2ATqNnSboP8Gr3HdWu3UwR").unwrap();
        assert_eq!(lookup_geo_bucket(&geo_map, &key), Some(2));

        let missing_key = decode_leader_pubkey("11111111111111111111111111111111").unwrap();
        assert_eq!(lookup_geo_bucket(&geo_map, &missing_key), None);
    }

    #[test]
    fn lookup_leader_geo_in_map_decodes_pubkey() {
        let geo_map = build_geo_map(&[
            ("7XSXtg2CWwjWCa7j4kXfYLMi8xawJbq6XW6xMa6Y5P9Q", 1),
            ("2jXy799ynN5A6xM4mT2QPY2ATqNnSboP8Gr3HdWu3UwR", 2),
            ("9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv", 3),
            ("9YvS2fH5A2m2W6B8hWcP8d9Yhrb2nJbLg2xwqQ8CbW2s", 4),
        ]);

        assert_eq!(
            lookup_leader_geo_in_map(&geo_map, "9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv"),
            Some("APAC")
        );
        assert_eq!(
            lookup_leader_geo_in_map(&geo_map, "11111111111111111111111111111111"),
            None
        );
    }

    #[test]
    fn lookup_geo_bucket_rejects_misaligned_data() {
        assert_eq!(lookup_geo_bucket(&[1, 2, 3], &[0u8; 32]), None);
    }

    fn socket(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 10), port))
    }

    fn contact_info(
        tpu_quic: Option<SocketAddr>,
        tpu: Option<SocketAddr>,
        gossip: Option<SocketAddr>,
        rpc: Option<SocketAddr>,
    ) -> RpcContactInfo {
        RpcContactInfo {
            pubkey: "leader".to_string(),
            gossip,
            tvu: None,
            tpu,
            tpu_quic,
            tpu_forwards: None,
            tpu_forwards_quic: None,
            tpu_vote: None,
            serve_repair: None,
            rpc,
            pubsub: None,
            version: None,
            feature_set: None,
            shred_version: None,
        }
    }

    fn build_geo_map(entries: &[(&str, u8)]) -> Vec<u8> {
        let mut decoded: Vec<([u8; 32], u8)> = entries
            .iter()
            .map(|(pubkey, bucket)| (decode_leader_pubkey(pubkey).unwrap(), *bucket))
            .collect();
        decoded.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

        let mut bytes = Vec::with_capacity(decoded.len() * LEADER_GEO_RECORD_SIZE);
        for (pubkey, bucket) in decoded {
            bytes.extend_from_slice(&pubkey);
            bytes.push(bucket);
        }
        bytes
    }
}
