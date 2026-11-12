//! Antimeridian-safe viewport handling (spec §6.4, §23.3).
//! A wrapped viewport (`west > east`) becomes `[west, 180]` + `[-180, east]`.

use super::validate::Viewport;
use crate::domain::geometry::BBox;

pub fn query_boxes(v: &Viewport) -> Vec<BBox> {
    if v.west > v.east {
        vec![
            BBox { min_lon: v.west, max_lon: 180.0, min_lat: v.south, max_lat: v.north },
            BBox { min_lon: -180.0, max_lon: v.east, min_lat: v.south, max_lat: v.north },
        ]
    } else {
        vec![BBox { min_lon: v.west, max_lon: v.east, min_lat: v.south, max_lat: v.north }]
    }
}
