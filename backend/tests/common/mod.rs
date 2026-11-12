#![allow(dead_code)]
use wda::domain::severity::SeverityMapping;

pub fn mapping() -> SeverityMapping {
    let s = include_str!("../../config/severity-mapping.toml");
    let m: SeverityMapping = toml::from_str(s).expect("mapping parses");
    m.validate().expect("mapping valid");
    m
}

pub fn cap(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/cap/{name}", env!("CARGO_MANIFEST_DIR"))).expect("fixture")
}

pub fn geojson(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/geojson/{name}", env!("CARGO_MANIFEST_DIR"))).expect("fixture")
}

/// Fixed "now" after all CAP fixture times (2026-11-12T13:00:00+07:00) and before expiry.
pub const NOW_MS: i64 = 1_794_463_200_000;
