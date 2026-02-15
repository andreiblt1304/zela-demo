use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::{self, ErrorKind, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::db::{GeoBucket, RECORD_SIZE};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub struct GenerationStats {
    pub total_leaders: usize,
    pub mapped_leaders: usize,
    pub unknown_leaders: usize,
    pub unknown_rate_pct: f64,
    pub output_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct MetadataOutput {
    pub stats: GenerationStats,
    pub path: PathBuf,
}

pub fn write_map_metadata(
    rpc_url: &str,
    db_path: &Path,
    map_path: &Path,
    map: &BTreeMap<[u8; 32], GeoBucket>,
) -> Result<MetadataOutput, Box<dyn Error>> {
    let generated_at_unix_secs = current_unix_secs()?;
    let rpc_slot = fetch_current_slot_from_rpc(rpc_url)?;
    let stats = compute_generation_stats(map);
    let metadata_path = metadata_path_for_map(map_path);
    let map_sha256 = sha256_file_hex(map_path)?;
    let mmdb_sha256 = sha256_file_hex(db_path)?;

    write_metadata_file(
        &metadata_path,
        &json!({
            "schema_version": 1,
            "generated_at_unix_secs": generated_at_unix_secs,
            "rpc_url": rpc_url,
            "rpc_slot": rpc_slot,
            "db_path": db_path.display().to_string(),
            "mmdb_sha256": mmdb_sha256,
            "record_size_bytes": RECORD_SIZE,
            "total_leaders": stats.total_leaders,
            "mapped_leaders": stats.mapped_leaders,
            "unknown_leaders": stats.unknown_leaders,
            "unknown_rate_pct": stats.unknown_rate_pct,
            "map_size_bytes": stats.output_bytes,
            "map_sha256": map_sha256
        }),
    )?;

    Ok(MetadataOutput {
        stats,
        path: metadata_path,
    })
}

fn current_unix_secs() -> Result<u64, Box<dyn Error>> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn compute_generation_stats(map: &BTreeMap<[u8; 32], GeoBucket>) -> GenerationStats {
    let total_leaders = map.len();
    let unknown_leaders = map
        .values()
        .filter(|bucket| **bucket == GeoBucket::Unknown)
        .count();
    let mapped_leaders = total_leaders.saturating_sub(unknown_leaders);
    let unknown_rate_pct = if total_leaders == 0 {
        0.0
    } else {
        (unknown_leaders as f64 / total_leaders as f64) * 100.0
    };
    let output_bytes = total_leaders * RECORD_SIZE;

    GenerationStats {
        total_leaders,
        mapped_leaders,
        unknown_leaders,
        unknown_rate_pct,
        output_bytes,
    }
}

fn metadata_path_for_map(map_path: &Path) -> PathBuf {
    map_path.with_extension("meta.json")
}

fn write_metadata_file(path: &Path, metadata: &Value) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(metadata)?;
    fs::write(path, bytes)?;
    Ok(())
}

fn sha256_file_hex(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let digest = hasher.finalize();
    Ok(format!("{digest:x}"))
}

fn fetch_current_slot_from_rpc(rpc_url: &str) -> Result<u64, Box<dyn Error>> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSlot",
        "params": [],
    })
    .to_string();

    let response = ureq::post(rpc_url)
        .set("Content-Type", "application/json")
        .send_string(&request)?;
    let body = response.into_string()?;

    parse_get_slot_response(&body)
}

fn parse_get_slot_response(body: &str) -> Result<u64, Box<dyn Error>> {
    let payload: Value = serde_json::from_str(body)?;

    if let Some(err) = payload.get("error") {
        return Err(io::Error::other(format!("RPC getSlot error: {err}")).into());
    }

    payload
        .get("result")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidData,
                "RPC getSlot response missing integer result",
            )
            .into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_path_uses_meta_json_sidecar() {
        let map_path = Path::new("procedure/data/leader_geo_map.bin");
        assert_eq!(
            metadata_path_for_map(map_path),
            PathBuf::from("procedure/data/leader_geo_map.meta.json")
        );
    }

    #[test]
    fn parse_get_slot_response_parses_result() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":400403440}"#;
        assert_eq!(parse_get_slot_response(body).unwrap(), 400403440);
    }

    #[test]
    fn parse_get_slot_response_returns_error_on_rpc_error() {
        let body = r#"{
            "jsonrpc":"2.0",
            "id":1,
            "error":{"code":-32000,"message":"boom"}
        }"#;
        let err = parse_get_slot_response(body).unwrap_err();
        assert!(err.to_string().contains("getSlot"));
    }
}
