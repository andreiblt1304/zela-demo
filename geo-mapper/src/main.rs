use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    net::IpAddr,
    path::{Path, PathBuf},
};

use maxminddb::{MaxMindDbError, Reader, geoip2};

const DEFAULT_DB_PATH: &str = "../GeoLite2-City_20260210/GeoLite2-City.mmdb";
const RECORD_SIZE: usize = 33;

#[derive(Debug, Clone)]
struct Cli {
    input: PathBuf,
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
    let input = fs::read_to_string(&cli.input)?;

    let rows = parse_rows(&input)?;
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
    let mut output: Option<PathBuf> = None;
    let mut db_path = PathBuf::from(DEFAULT_DB_PATH);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                let Some(value) = args.next() else {
                    return Err("missing value for --input".into());
                };
                input = Some(PathBuf::from(value));
            }
            "--output" => {
                let Some(value) = args.next() else {
                    return Err("missing value for --output".into());
                };
                output = Some(PathBuf::from(value));
            }
            "--db" => {
                let Some(value) = args.next() else {
                    return Err("missing value for --db".into());
                };
                db_path = PathBuf::from(value);
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                return Err(format!("unknown argument: {arg}").into());
            }
        }
    }

    let Some(input) = input else {
        return Err("--input is required".into());
    };
    let Some(output) = output else {
        return Err("--output is required".into());
    };

    Ok(Cli {
        input,
        output,
        db_path,
    })
}

fn print_usage() {
    println!(
        "Usage: geo-mapper --input <leaders.csv> --output <leader_geo_map.bin> [--db <GeoLite2-City.mmdb>]"
    );
    println!("Input format per line: <leader_pubkey>,<ip_or_bucket>");
    println!("Examples of second column: 95.217.151.43 | EU | @NA | APAC");
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
}
