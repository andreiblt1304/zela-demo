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

    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Eu),
            2 => Some(Self::Na),
            3 => Some(Self::Apac),
            4 => Some(Self::Me),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Eu => "EU",
            Self::Na => "NA",
            Self::Apac => "APAC",
            Self::Me => "ME",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Dubai,
    Frankfurt,
    NewYork,
    Tokyo,
}

pub fn bucket_from_country_iso(iso_code: &str) -> GeoBucket {
    match iso_code.trim().to_ascii_uppercase().as_str() {
        "DE" | "FR" | "NL" | "GB" | "CH" | "SE" | "NO" | "PL" | "ES" | "IT" => GeoBucket::Eu,
        "AE" | "SA" | "IL" | "TR" | "QA" | "BH" | "OM" | "KW" => GeoBucket::Me,
        "US" | "CA" | "MX" => GeoBucket::Na,
        "JP" | "KR" | "SG" | "HK" | "TW" | "IN" | "AU" | "NZ" => GeoBucket::Apac,
        _ => GeoBucket::Unknown,
    }
}

pub fn bucket_from_geo_input(input: &str) -> GeoBucket {
    let normalized = input.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "EU" => GeoBucket::Eu,
        "NA" => GeoBucket::Na,
        "APAC" => GeoBucket::Apac,
        "ME" => GeoBucket::Me,
        _ => bucket_from_country_iso(&normalized),
    }
}

pub fn region_from_bucket(bucket: GeoBucket) -> Option<Region> {
    match bucket {
        GeoBucket::Eu => Some(Region::Frankfurt),
        GeoBucket::Me => Some(Region::Dubai),
        GeoBucket::Na => Some(Region::NewYork),
        GeoBucket::Apac => Some(Region::Tokyo),
        GeoBucket::Unknown => None,
    }
}

pub fn region_from_geo_input(input: &str) -> Option<Region> {
    region_from_bucket(bucket_from_geo_input(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_from_country_iso_maps_known_codes() {
        assert_eq!(bucket_from_country_iso("DE"), GeoBucket::Eu);
        assert_eq!(bucket_from_country_iso("US"), GeoBucket::Na);
        assert_eq!(bucket_from_country_iso("JP"), GeoBucket::Apac);
        assert_eq!(bucket_from_country_iso("AE"), GeoBucket::Me);
        assert_eq!(bucket_from_country_iso("BR"), GeoBucket::Unknown);
    }

    #[test]
    fn bucket_from_geo_input_accepts_bucket_labels() {
        assert_eq!(bucket_from_geo_input("eu"), GeoBucket::Eu);
        assert_eq!(bucket_from_geo_input("NA"), GeoBucket::Na);
        assert_eq!(bucket_from_geo_input("apac"), GeoBucket::Apac);
        assert_eq!(bucket_from_geo_input("ME"), GeoBucket::Me);
        assert_eq!(bucket_from_geo_input("UNKNOWN"), GeoBucket::Unknown);
    }

    #[test]
    fn region_from_geo_input_maps_country_codes_and_labels() {
        assert_eq!(region_from_geo_input("EU"), Some(Region::Frankfurt));
        assert_eq!(region_from_geo_input("AE"), Some(Region::Dubai));
        assert_eq!(region_from_geo_input("US"), Some(Region::NewYork));
        assert_eq!(region_from_geo_input("JP"), Some(Region::Tokyo));
        assert_eq!(region_from_geo_input("unknown"), None);
    }
}
