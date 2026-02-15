mod cli;
mod db;

use std::{
    collections::BTreeMap,
    error::Error,
    io::{self, ErrorKind},
    net::{IpAddr, SocketAddr},
};

use crate::cli::Cli;
use crate::db::{GeoBucket, RECORD_SIZE, compute_geolocation, get_db_reader, write_binary_map};
use serde_json::{Value, json};

#[derive(Debug)]
struct InputRow {
    pubkey: [u8; 32],
    ip: IpAddr,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("geo-mapper failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse()?;

    let rows = fetch_rows_from_rpc(&cli.rpc_url)?;
    println!(
        "fetched {} candidate leader rows from {}",
        rows.len(),
        cli.rpc_url
    );

    if rows.is_empty() {
        println!("warning: no rows found; output map will be empty");
    }

    let reader = get_db_reader(&cli.db_path)?;

    let mut map: BTreeMap<[u8; 32], GeoBucket> = BTreeMap::new();

    for row in rows {
        let bucket = compute_geolocation(&reader, row.ip)?;

        map.entry(row.pubkey)
            .and_modify(|existing| {
                if *existing == GeoBucket::Unknown && bucket != GeoBucket::Unknown {
                    *existing = bucket;
                }
            })
            .or_insert(bucket);
    }

    write_binary_map(&cli.output, &map)?;

    let total_leaders = map.len();
    let unknown_leaders = map
        .values()
        .filter(|bucket| **bucket == GeoBucket::Unknown)
        .count();
    let mapped_leaders = total_leaders.saturating_sub(unknown_leaders);
    let unknown_rate = if total_leaders == 0 {
        0.0
    } else {
        (unknown_leaders as f64 / total_leaders as f64) * 100.0
    };
    let output_bytes = total_leaders * RECORD_SIZE;

    println!(
        "wrote {} records ({} bytes) to {}",
        total_leaders,
        output_bytes,
        cli.output.display()
    );
    println!(
        "stats: total_leaders={} mapped_leaders={} unknown_leaders={} unknown_rate={:.2}% output_bytes={}",
        total_leaders, mapped_leaders, unknown_leaders, unknown_rate, output_bytes
    );

    Ok(())
}

fn fetch_rows_from_rpc(rpc_url: &str) -> Result<Vec<InputRow>, Box<dyn Error>> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getClusterNodes",
        "params": [],
    })
    .to_string();

    let response = ureq::post(rpc_url)
        .set("Content-Type", "application/json")
        .send_string(&request)?;
    let body = response.into_string()?;

    parse_cluster_nodes_response(&body)
}

fn parse_cluster_nodes_response(body: &str) -> Result<Vec<InputRow>, Box<dyn Error>> {
    let payload: Value = serde_json::from_str(body)?;

    if let Some(err) = payload.get("error") {
        return Err(io::Error::other(format!("RPC getClusterNodes error: {err}")).into());
    }

    let result = payload
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidData,
                "RPC getClusterNodes response missing result array",
            )
        })?;

    let mut rows = Vec::new();

    for node in result {
        let Some(pubkey) = node.get("pubkey").and_then(Value::as_str) else {
            continue;
        };

        let Ok(pubkey) = decode_pubkey(pubkey) else {
            continue;
        };

        let Some(ip) = preferred_ip_from_node(node) else {
            continue;
        };

        rows.push(InputRow { pubkey, ip });
    }

    Ok(rows)
}

fn preferred_ip_from_node(node: &Value) -> Option<IpAddr> {
    for key in ["tpu_quic", "tpu", "gossip", "rpc"] {
        if let Some(socket) = node.get(key).and_then(Value::as_str)
            && let Some(ip) = extract_ip_from_socket(socket)
        {
            return Some(ip);
        }
    }

    None
}

fn extract_ip_from_socket(socket: &str) -> Option<IpAddr> {
    if let Ok(addr) = socket.parse::<SocketAddr>() {
        return Some(addr.ip());
    }

    if let Ok(ip) = socket.parse::<IpAddr>() {
        return Some(ip);
    }

    if socket.starts_with('[')
        && let Some(end) = socket.find(']')
    {
        return socket[1..end].parse::<IpAddr>().ok();
    }

    if let Some((host, _port)) = socket.rsplit_once(':') {
        return host.parse::<IpAddr>().ok();
    }

    None
}

fn decode_pubkey(pubkey: &str) -> Result<[u8; 32], Box<dyn Error>> {
    let decoded = bs58::decode(pubkey).into_vec()?;
    if decoded.len() != 32 {
        return Err(format!(
            "expected 32 bytes after base58 decode, got {}",
            decoded.len()
        )
        .into());
    }
    let mut pubkey_bytes = [0u8; 32];
    pubkey_bytes.copy_from_slice(&decoded);
    Ok(pubkey_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_ip_from_socket_supports_ipv4_and_ipv6() {
        assert_eq!(
            extract_ip_from_socket("95.217.151.43:8001"),
            Some("95.217.151.43".parse().unwrap())
        );
        assert_eq!(
            extract_ip_from_socket("[2001:db8::1]:8001"),
            Some("2001:db8::1".parse().unwrap())
        );
        assert_eq!(extract_ip_from_socket("not-an-ip"), None);
    }

    #[test]
    fn parse_cluster_nodes_response_prefers_transport_order() {
        let body = r#"{
            "jsonrpc":"2.0",
            "id":1,
            "result":[
                {
                    "pubkey":"7XSXtg2CWwjWCa7j4kXfYLMi8xawJbq6XW6xMa6Y5P9Q",
                    "tpu_quic":"1.2.3.4:8001",
                    "tpu":"5.6.7.8:8001",
                    "gossip":"9.9.9.9:8001",
                    "rpc":"10.0.0.1:8899"
                },
                {
                    "pubkey":"2jXy799ynN5A6xM4mT2QPY2ATqNnSboP8Gr3HdWu3UwR",
                    "tpu_quic":null,
                    "tpu":"[2001:db8::1]:8001",
                    "gossip":null,
                    "rpc":null
                },
                {
                    "pubkey":"invalid",
                    "tpu_quic":"2.2.2.2:8001"
                }
            ]
        }"#;

        let rows = parse_cluster_nodes_response(body).unwrap();
        assert_eq!(rows.len(), 2);

        assert_eq!(rows[0].ip, "1.2.3.4".parse::<IpAddr>().unwrap());
        assert_eq!(rows[1].ip, "2001:db8::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_cluster_nodes_response_returns_error_on_rpc_error() {
        let body = r#"{
            "jsonrpc":"2.0",
            "id":1,
            "error":{"code":-32000,"message":"boom"}
        }"#;

        let err = parse_cluster_nodes_response(body).unwrap_err();
        assert!(err.to_string().contains("getClusterNodes"));
    }
}
