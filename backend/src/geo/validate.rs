//! Viewport validation (spec §9.2): named coordinates, clamp/reject invalid values.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct Viewport {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
    pub zoom: f64,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ViewportError {
    #[error("viewport values must be finite numbers")]
    NotFinite,
    #[error("latitude must be within [-90, 90] and south <= north")]
    Latitude,
    #[error("longitude must be within [-180, 180]")]
    Longitude,
    #[error("zoom must be within [0, 22]")]
    Zoom,
}

pub const MAX_ZOOM: f64 = 22.0;

impl Viewport {
    /// Rejects invalid values (no silent clamping of latitude/longitude).
    /// `west > east` is valid and means the viewport crosses the antimeridian.
    pub fn validate(self) -> Result<Self, ViewportError> {
        let v = [self.west, self.south, self.east, self.north, self.zoom];
        if v.iter().any(|x| !x.is_finite()) {
            return Err(ViewportError::NotFinite);
        }
        if !(-90.0..=90.0).contains(&self.south) || !(-90.0..=90.0).contains(&self.north) || self.south > self.north {
            return Err(ViewportError::Latitude);
        }
        if !(-180.0..=180.0).contains(&self.west) || !(-180.0..=180.0).contains(&self.east) {
            return Err(ViewportError::Longitude);
        }
        if !(0.0..=MAX_ZOOM).contains(&self.zoom) {
            return Err(ViewportError::Zoom);
        }
        Ok(self)
    }
}
