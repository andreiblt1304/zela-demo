use log::info;
use serde::Serialize;
use zela_std::rpc_client::{RpcClient, response::RpcLeaderSchedule};
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
    type Params = Option<JsonValue>;
    type SuccessData = LeaderRoutingOutput;
    type ErrorData = ProcedureErrorData;

    async fn run(_params: Self::Params) -> Result<Self::SuccessData, RpcError<Self::ErrorData>> {
        let rpc = RpcClient::new();

        let slot = rpc.get_slot().await.map_err(|err| {
            internal_error("get_slot", format!("failed to fetch current slot: {err}"))
        })?;

        let epoch_schedule = rpc.get_epoch_schedule().await.map_err(|err| {
            internal_error(
                "get_epoch_schedule",
                format!("failed to fetch epoch schedule: {err}"),
            )
        })?;

        let leader_schedule = rpc.get_leader_schedule(Some(slot)).await.map_err(|err| {
            internal_error(
                "get_leader_schedule",
                format!("failed to fetch leader schedule for slot {slot}: {err}"),
            )
        })?;

        let leader_schedule = leader_schedule.ok_or_else(|| {
            internal_error(
                "get_leader_schedule",
                format!("leader schedule was missing for slot {slot}"),
            )
        })?;

        let epoch = epoch_schedule.get_epoch(slot);
        let first_slot_in_epoch = epoch_schedule.get_first_slot_in_epoch(epoch);
        let slot_index =
            usize::try_from(slot.saturating_sub(first_slot_in_epoch)).map_err(|err| {
                internal_error(
                    "slot_index",
                    format!(
                        "failed to compute slot index in epoch for slot {slot} and first slot {first_slot_in_epoch}: {err}"
                    ),
                )
            })?;

        let leader = find_leader_for_slot_index(&leader_schedule, slot_index).ok_or_else(|| {
            internal_error(
                "resolve_leader",
                format!(
                    "no leader found in leader schedule for slot {slot} (slot_index={slot_index})"
                ),
            )
        })?;

        let (leader_geo, closest_region) =
            derive_leader_geo_and_region(&leader, LEADER_GEO_MAP_BIN);

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

fn find_leader_for_slot_index(
    leader_schedule: &RpcLeaderSchedule,
    slot_index: usize,
) -> Option<String> {
    // HashMap iteration order is non-deterministic; pick a stable lexicographic winner.
    leader_schedule
        .iter()
        .filter_map(|(leader, slots)| slots.contains(&slot_index).then_some(leader.as_str()))
        .min()
        .map(ToString::to_string)
}

fn derive_leader_geo_and_region(leader_pubkey: &str, geo_map: &[u8]) -> (String, ServerRegion) {
    let leader_geo = lookup_leader_geo_in_map(geo_map, leader_pubkey)
        .unwrap_or(UNKNOWN_GEO)
        .to_string();
    let closest_region = choose_region(&leader_geo, leader_pubkey);
    (leader_geo, closest_region)
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
    use serde::Deserialize;
    use std::collections::HashMap;

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
    fn find_leader_for_slot_is_stable() {
        let mut schedule: HashMap<String, Vec<usize>> = HashMap::new();
        schedule.insert("validator-z".to_string(), vec![8]);
        schedule.insert("validator-a".to_string(), vec![8]);

        let leader = find_leader_for_slot_index(&schedule, 8).unwrap();
        assert_eq!(leader, "validator-a");
    }

    #[test]
    fn choose_region_uses_geo_then_fallback() {
        assert_eq!(choose_region("EU", "x"), ServerRegion::Frankfurt);

        let from_unknown_geo = choose_region(UNKNOWN_GEO, "validator-x");
        let deterministic_again = choose_region(UNKNOWN_GEO, "validator-x");
        assert_eq!(from_unknown_geo, deterministic_again);
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

    #[derive(Debug, Deserialize)]
    struct RpcEnvelope {
        #[serde(default)]
        params: Option<JsonValue>,
    }

    #[test]
    fn no_input_params_shape_omitted_is_supported() {
        let request = r#"{
            "jsonrpc":"2.0",
            "id":1,
            "method":"zela.geolocation#hash"
        }"#;
        let envelope: RpcEnvelope = serde_json::from_str(request).unwrap();
        assert_eq!(envelope.params, None);
    }

    #[test]
    fn no_input_params_shape_null_is_supported() {
        let request = r#"{
            "jsonrpc":"2.0",
            "id":1,
            "method":"zela.geolocation#hash",
            "params":null
        }"#;
        let envelope: RpcEnvelope = serde_json::from_str(request).unwrap();
        assert_eq!(envelope.params, None);
    }

    #[test]
    fn no_input_params_shape_empty_object_is_supported() {
        let request = r#"{
            "jsonrpc":"2.0",
            "id":1,
            "method":"zela.geolocation#hash",
            "params":{}
        }"#;
        let envelope: RpcEnvelope = serde_json::from_str(request).unwrap();
        assert_eq!(envelope.params, Some(serde_json::json!({})));
    }

    #[test]
    fn malformed_geo_map_degrades_to_unknown_geo_without_panicking() {
        let leader = "9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv";
        let malformed_geo_map = [1u8, 2, 3];

        let (leader_geo, closest_region) = derive_leader_geo_and_region(leader, &malformed_geo_map);

        assert_eq!(leader_geo, UNKNOWN_GEO);
        assert_eq!(closest_region, fallback_region(leader));
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
