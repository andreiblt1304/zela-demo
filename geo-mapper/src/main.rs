use std::{net::IpAddr, sync::Arc};

type DbReader = Reader<Vec<u8>>;

use maxminddb::{MaxMindDbError, Reader, geoip2};

const DB_PATH: &str = "../GeoLite2-City_20260210/GeoLite2-City.mmdb";

fn compute_geolocation(ip: IpAddr) -> Result<(), Box<dyn std::error::Error>> {
    let reader = get_db_reader()?;

    let result = reader.lookup(ip)?;

    if let Some(city) = result.decode::<geoip2::City>()? {
        // Access nested structs directly - no Option unwrapping needed
        println!("Country: {}", city.country.iso_code.unwrap_or("Unknown"));
    }

    Ok(())
}

fn get_db_reader() -> Result<DbReader, MaxMindDbError> {
    let reader = Reader::open_readfile(DB_PATH)?;

    Ok(reader)
}

fn main() {}
