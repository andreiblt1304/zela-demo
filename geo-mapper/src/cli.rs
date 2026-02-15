use std::{
    error::Error,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

const DEFAULT_DB_REL_PATH: &str = "GeoLite2-City_20260210/GeoLite2-City.mmdb";
const DEFAULT_RPC_URL: &str = "https://api.mainnet-beta.solana.com";

#[derive(Debug, Clone)]
pub struct Cli {
    pub rpc_url: String,
    pub output: PathBuf,
    pub db_path: PathBuf,
}

impl Cli {
    pub fn parse() -> Result<Self, Box<dyn Error>> {
        let mut args = std::env::args().skip(1);

        let mut rpc_url: Option<String> = None;
        let mut output: Option<PathBuf> = None;
        let mut db_path = detect_default_db_path();

        while let Some(arg) = args.next() {
            match arg.as_str() {
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
                        return Err(io::Error::new(
                            ErrorKind::InvalidInput,
                            "missing value for --db",
                        )
                        .into());
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

        Ok(Self {
            rpc_url: rpc_url.unwrap_or_else(|| DEFAULT_RPC_URL.to_string()),
            output,
            db_path,
        })
    }
}

fn print_usage() {
    println!(
        "Usage: geo-mapper --output <leader_geo_map.bin> [--rpc-url <solana_rpc_url>] [--db <GeoLite2-City.mmdb>]"
    );
}

fn detect_default_db_path() -> PathBuf {
    let candidates = [
        PathBuf::from(DEFAULT_DB_REL_PATH),
        PathBuf::from("..").join(DEFAULT_DB_REL_PATH),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(DEFAULT_DB_REL_PATH),
    ];

    candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone())
}
