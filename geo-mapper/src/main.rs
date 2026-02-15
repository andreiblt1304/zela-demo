use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::{self, ErrorKind},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

use maxminddb::{MaxMindDbError, Reader, geoip2};
use serde_json::{Value, json};

const DEFAULT_DB_PATH: &str = "../GeoLite2-City_20260210/GeoLite2-City.mmdb";
const RECORD_SIZE: usize = 33;

#[derive(Debug, Clone)]
struct Cli {
    input: Option<PathBuf>,
    rpc_url: Option<String>,
    output: PathBuf,
    db_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum GeoBucket {
    Unknown = 0,
    Eu = 1,
    Na = 2,
    Apac = 3,
    Me = 4,
}

impl GeoBucket {
    fn from_label(value: &str) -> Option<Self> {
        let normalized = value.trim().trim_start_matches('@').to_ascii_uppercase();
        match normalized.as_str() {
            "UNKNOWN" => Some(Self::Unknown),
            "EU" => Some(Self::Eu),
            "NA" => Some(Self::Na),
            "APAC" => Some(Self::Apac),
            "ME" => Some(Self::Me),
            _ => None,
        }
    }

    fn as_u8(self) -> u8 {
        self as u8
    }
}

enum GeoSource {
    Ip(IpAddr),
    Bucket(GeoBucket),
}

struct InputRow {
    pubkey: [u8; 32],
    geo_source: GeoSource,
}

type DbReader = Reader<Vec<u8>>;

fn main() {
    if let Err(err) = run() {
        eprintln!("geo-mapper failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = parse_cli()?;

    let rows = if let Some(input_path) = &cli.input {
        let input = fs::read_to_string(input_path)?;
        parse_rows(&input)?
    } else {
        let rpc_url = cli
            .rpc_url
            .as_deref()
            .expect("parse_cli validates rpc_url is set");
        let rows = fetch_rows_from_rpc(rpc_url)?;
        println!("fetched {} rows from {}", rows.len(), rpc_url);
        rows
    };

    if rows.is_empty() {
        println!("warning: no rows found; output map will be empty");
    }

    let requires_db = rows
        .iter()
        .any(|row| matches!(row.geo_source, GeoSource::Ip(_)));

    let reader = if requires_db {
        Some(get_db_reader(&cli.db_path)?)
    } else {
        None
    };

    let mut map: BTreeMap<[u8; 32], GeoBucket> = BTreeMap::new();

    for row in rows {
        let bucket = match row.geo_source {
            GeoSource::Bucket(bucket) => bucket,
            GeoSource::Ip(ip) => {
                let Some(reader) = reader.as_ref() else {
                    return Err("database reader not initialized".into());
                };
                compute_geolocation(reader, ip)?
            }
        };

        map.entry(row.pubkey)
            .and_modify(|existing| {
                if *existing == GeoBucket::Unknown && bucket != GeoBucket::Unknown {
                    *existing = bucket;
                }
            })
            .or_insert(bucket);
    }

    write_binary_map(&cli.output, &map)?;

    println!(
        "wrote {} records ({} bytes) to {}",
        map.len(),
        map.len() * RECORD_SIZE,
        cli.output.display()
    );

    Ok(())
}

fn parse_cli() -> Result<Cli, Box<dyn Error>> {
    let mut args = std::env::args().skip(1);

    let mut input: Option<PathBuf> = None;
    let mut rpc_url: Option<String> = None;
    let mut output: Option<PathBuf> = None;
    let mut db_path = PathBuf::from(DEFAULT_DB_PATH);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                let Some(value) = args.next() else {
                    return Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        "missing value for --input",
                    )
                    .into());
                };
                input = Some(PathBuf::from(value));
            }
            "--rpc-url" => {
                let Some(value) = args.next() else {
                    return Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        "missing value for --rpc-url",
                    )
                    .into());
                };
                rpc_url = Some(value);
            }
            "--output" => {
                let Some(value) = args.next() else {
                    return Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        "missing value for --output",
                    )
                    .into());
                };
                output = Some(PathBuf::from(value));
            }
            "--db" => {
                let Some(value) = args.next() else {
                    return Err(
                        io::Error::new(ErrorKind::InvalidInput, "missing value for --db").into(),
                    );
                };
                db_path = PathBuf::from(value);
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown argument: {arg}"),
                )
                .into());
            }
        }
    }

    let Some(output) = output else {
        return Err(io::Error::new(ErrorKind::InvalidInput, "--output is required").into());
    };

    if input.is_some() == rpc_url.is_some() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "exactly one of --input or --rpc-url is required",
        )
        .into());
    }

    Ok(Cli {
        input,
        rpc_url,
        output,
        db_path,
    })
}

fn print_usage() {
    println!(
        "Usage:\n  geo-mapper --input <leaders.csv> --output <leader_geo_map.bin> [--db <GeoLite2-City.mmdb>]\n  geo-mapper --rpc-url <solana_rpc_url> --output <leader_geo_map.bin> [--db <GeoLite2-City.mmdb>]"
    );
    println!("CSV format: <leader_pubkey>,<ip_or_bucket>");
    println!("Examples of second column: 95.217.151.43 | EU | @NA | APAC");
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

        rows.push(InputRow {
            pubkey,
            geo_source: GeoSource::Ip(ip),
        });
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

fn parse_rows(input: &str) -> Result<Vec<InputRow>, Box<dyn Error>> {
    let mut rows = Vec::new();

    for (idx, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split(',');
        let pubkey_str = parts
            .next()
            .map(str::trim)
            .ok_or_else(|| format!("line {}: missing pubkey", idx + 1))?;
        let source_str = parts
            .next()
            .map(str::trim)
            .ok_or_else(|| format!("line {}: missing geo source", idx + 1))?;

        if parts.next().is_some() {
            return Err(format!(
                "line {}: expected exactly two comma-separated columns",
                idx + 1
            )
            .into());
        }

        let pubkey = decode_pubkey(pubkey_str)
            .map_err(|err| format!("line {}: invalid pubkey: {err}", idx + 1))?;

        let geo_source = if let Ok(ip) = source_str.parse::<IpAddr>() {
            GeoSource::Ip(ip)
        } else if let Some(bucket) = GeoBucket::from_label(source_str) {
            GeoSource::Bucket(bucket)
        } else {
            return Err(format!(
                "line {}: geo source must be an IP or one of UNKNOWN|EU|NA|APAC|ME",
                idx + 1
            )
            .into());
        };

        rows.push(InputRow { pubkey, geo_source });
    }

    Ok(rows)
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

fn compute_geolocation(reader: &DbReader, ip: IpAddr) -> Result<GeoBucket, Box<dyn Error>> {
    let result = reader.lookup(ip)?;

    if let Some(city) = result.decode::<geoip2::City>()?
        && let Some(iso_code) = city.country.iso_code
    {
        return Ok(country_to_bucket(iso_code));
    }

    Ok(GeoBucket::Unknown)
}

fn get_db_reader(path: &Path) -> Result<DbReader, MaxMindDbError> {
    Reader::open_readfile(path)
}

fn country_to_bucket(iso_code: &str) -> GeoBucket {
    match iso_code.to_ascii_uppercase().as_str() {
        "DE" | "FR" | "NL" | "GB" | "CH" | "SE" | "NO" | "PL" | "ES" | "IT" => GeoBucket::Eu,
        "AE" | "SA" | "IL" | "TR" | "QA" | "BH" | "OM" | "KW" => GeoBucket::Me,
        "US" | "CA" | "MX" => GeoBucket::Na,
        "JP" | "KR" | "SG" | "HK" | "TW" | "IN" | "AU" | "NZ" => GeoBucket::Apac,
        _ => GeoBucket::Unknown,
    }
}

fn write_binary_map(
    path: &Path,
    map: &BTreeMap<[u8; 32], GeoBucket>,
) -> Result<(), Box<dyn Error>> {
    let mut output = Vec::with_capacity(map.len() * RECORD_SIZE);
    for (pubkey, bucket) in map {
        output.extend_from_slice(pubkey);
        output.push(bucket.as_u8());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bucket_labels_case_insensitively() {
        assert_eq!(GeoBucket::from_label("eu"), Some(GeoBucket::Eu));
        assert_eq!(GeoBucket::from_label("@na"), Some(GeoBucket::Na));
        assert_eq!(GeoBucket::from_label("APAC"), Some(GeoBucket::Apac));
        assert_eq!(GeoBucket::from_label("unknown"), Some(GeoBucket::Unknown));
        assert_eq!(GeoBucket::from_label("ZZ"), None);
    }

    #[test]
    fn country_to_bucket_maps_known_codes() {
        assert_eq!(country_to_bucket("DE"), GeoBucket::Eu);
        assert_eq!(country_to_bucket("US"), GeoBucket::Na);
        assert_eq!(country_to_bucket("JP"), GeoBucket::Apac);
        assert_eq!(country_to_bucket("AE"), GeoBucket::Me);
        assert_eq!(country_to_bucket("BR"), GeoBucket::Unknown);
    }

    #[test]
    fn parse_rows_accepts_ip_and_bucket_inputs() {
        let input = "\
7XSXtg2CWwjWCa7j4kXfYLMi8xawJbq6XW6xMa6Y5P9Q,EU\n\
2jXy799ynN5A6xM4mT2QPY2ATqNnSboP8Gr3HdWu3UwR,8.8.8.8\n";
        let rows = parse_rows(input).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(matches!(
            rows[0].geo_source,
            GeoSource::Bucket(GeoBucket::Eu)
        ));
        assert!(matches!(rows[1].geo_source, GeoSource::Ip(_)));
    }

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

        match rows[0].geo_source {
            GeoSource::Ip(ip) => assert_eq!(ip, "1.2.3.4".parse::<IpAddr>().unwrap()),
            GeoSource::Bucket(_) => panic!("expected ip row"),
        }

        match rows[1].geo_source {
            GeoSource::Ip(ip) => assert_eq!(ip, "2001:db8::1".parse::<IpAddr>().unwrap()),
            GeoSource::Bucket(_) => panic!("expected ip row"),
        }
    }
}
