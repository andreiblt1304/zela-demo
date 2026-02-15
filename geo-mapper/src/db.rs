use std::{collections::BTreeMap, error::Error, fs, net::IpAddr, path::Path};

use maxminddb::{MaxMindDbError, Reader, geoip2};

pub const RECORD_SIZE: usize = 33;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GeoBucket {
    Unknown = 0,
    Eu = 1,
    Na = 2,
    Apac = 3,
    Me = 4,
}

impl GeoBucket {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

pub type DbReader = Reader<Vec<u8>>;

pub fn compute_geolocation(reader: &DbReader, ip: IpAddr) -> Result<GeoBucket, Box<dyn Error>> {
    let result = reader.lookup(ip)?;

    if let Some(city) = result.decode::<geoip2::City>()?
        && let Some(iso_code) = city.country.iso_code
    {
        return Ok(country_to_bucket(iso_code));
    }

    Ok(GeoBucket::Unknown)
}

pub fn get_db_reader(path: &Path) -> Result<DbReader, MaxMindDbError> {
    Reader::open_readfile(path)
}

pub fn write_binary_map(
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

fn country_to_bucket(iso_code: &str) -> GeoBucket {
    match iso_code.to_ascii_uppercase().as_str() {
        "DE" | "FR" | "NL" | "GB" | "CH" | "SE" | "NO" | "PL" | "ES" | "IT" => GeoBucket::Eu,
        "AE" | "SA" | "IL" | "TR" | "QA" | "BH" | "OM" | "KW" => GeoBucket::Me,
        "US" | "CA" | "MX" => GeoBucket::Na,
        "JP" | "KR" | "SG" | "HK" | "TW" | "IN" | "AU" | "NZ" => GeoBucket::Apac,
        _ => GeoBucket::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_to_bucket_maps_known_codes() {
        assert_eq!(country_to_bucket("DE"), GeoBucket::Eu);
        assert_eq!(country_to_bucket("US"), GeoBucket::Na);
        assert_eq!(country_to_bucket("JP"), GeoBucket::Apac);
        assert_eq!(country_to_bucket("AE"), GeoBucket::Me);
        assert_eq!(country_to_bucket("BR"), GeoBucket::Unknown);
    }
}
