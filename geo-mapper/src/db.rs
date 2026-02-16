use std::{collections::BTreeMap, error::Error, fs, net::IpAddr, path::Path};

use geo_rules::bucket_from_country_iso;
use maxminddb::{MaxMindDbError, Reader, geoip2};

pub use geo_rules::GeoBucket;

pub const RECORD_SIZE: usize = 33;

pub type DbReader = Reader<Vec<u8>>;

pub fn compute_geolocation(reader: &DbReader, ip: IpAddr) -> Result<GeoBucket, Box<dyn Error>> {
    let result = reader.lookup(ip)?;

    if let Some(city) = result.decode::<geoip2::City>()?
        && let Some(iso_code) = city.country.iso_code
    {
        return Ok(bucket_from_country_iso(iso_code));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_to_bucket_maps_known_codes() {
        assert_eq!(bucket_from_country_iso("DE"), GeoBucket::Eu);
        assert_eq!(bucket_from_country_iso("US"), GeoBucket::Na);
        assert_eq!(bucket_from_country_iso("JP"), GeoBucket::Apac);
        assert_eq!(bucket_from_country_iso("AE"), GeoBucket::Me);
        assert_eq!(bucket_from_country_iso("BR"), GeoBucket::Unknown);
    }
}
