//! Deterministic zoom-dependent grid clustering in Web Mercator world pixels
//! (spec §6.6, §26.4). Clusters expose id, count, bbox/centre, highest
//! display priority and per-priority counts so mixed severities stay visible.

use crate::domain::alert::DisplayPriority;
use crate::domain::geometry::GeoPoint;
use serde::Serialize;
use std::collections::BTreeMap;

pub const TILE_SIZE: f64 = 256.0;
pub const MAX_MERCATOR_LAT: f64 = 85.051_128_78;

/// World pixel coordinates at `zoom` (x grows east, y grows south).
pub fn mercator_px(p: &GeoPoint, zoom: f64) -> (f64, f64) {
    let world = TILE_SIZE * 2f64.powf(zoom);
    let lat = p.lat_deg.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT).to_radians();
    let x = (p.lon_deg + 180.0) / 360.0 * world;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * world;
    (x.clamp(0.0, world - 1e-9), y.clamp(0.0, world - 1e-9))
}

pub struct ClusterInput {
    pub alert_id: String,
    pub point: GeoPoint,
    pub priority: DisplayPriority,
    pub hazard_type: String,
    pub revision: u64,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MapFeature {
    Alert {
        alert_id: String,
        revision: u64,
        lat: f64,
        lon: f64,
        hazard_type: String,
        display_priority: DisplayPriority,
    },
    Cluster {
        cluster_id: String,
        count: usize,
        lat: f64,
        lon: f64,
        bbox: [f64; 4],
        top_priority: DisplayPriority,
        priority_counts: BTreeMap<String, usize>,
        /// Bounded member list for expansion (spec §6.6); `count` is authoritative.
        member_ids: Vec<String>,
    },
}

pub const MAX_CLUSTER_MEMBERS: usize = 100;

/// Groups points into grid cells of `cell_px`; doubles the cell size until the
/// feature count is <= `max_features` (spec §9.2: 50 visible symbols).
pub fn cluster(items: &[ClusterInput], zoom: f64, mut cell_px: f64, max_features: usize) -> (Vec<MapFeature>, bool) {
    loop {
        let mut cells: BTreeMap<(i64, i64), Vec<&ClusterInput>> = BTreeMap::new();
        for it in items {
            let (x, y) = mercator_px(&it.point, zoom);
            cells.entry(((x / cell_px).floor() as i64, (y / cell_px).floor() as i64)).or_default().push(it);
        }
        if cells.len() <= max_features || cell_px > TILE_SIZE * 2f64.powf(zoom) {
            let clustered = cells.values().any(|v| v.len() > 1);
            let feats = cells
                .into_iter()
                .map(|((cx, cy), mut members)| {
                    if members.len() == 1 {
                        let m = members[0];
                        return MapFeature::Alert {
                            alert_id: m.alert_id.clone(),
                            revision: m.revision,
                            lat: m.point.lat_deg,
                            lon: m.point.lon_deg,
                            hazard_type: m.hazard_type.clone(),
                            display_priority: m.priority,
                        };
                    }
                    members.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.alert_id.cmp(&b.alert_id)));
                    let n = members.len() as f64;
                    let lat = members.iter().map(|m| m.point.lat_deg).sum::<f64>() / n;
                    let lon = members.iter().map(|m| m.point.lon_deg).sum::<f64>() / n;
                    let bbox = [
                        members.iter().map(|m| m.point.lon_deg).fold(f64::INFINITY, f64::min),
                        members.iter().map(|m| m.point.lat_deg).fold(f64::INFINITY, f64::min),
                        members.iter().map(|m| m.point.lon_deg).fold(f64::NEG_INFINITY, f64::max),
                        members.iter().map(|m| m.point.lat_deg).fold(f64::NEG_INFINITY, f64::max),
                    ];
                    let mut counts = BTreeMap::new();
                    for m in &members {
                        let k = serde_json::to_value(m.priority).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                        *counts.entry(k).or_insert(0) += 1;
                    }
                    MapFeature::Cluster {
                        cluster_id: format!("z{}:c{}:{}:{}", zoom.floor() as i64, cell_px as i64, cx, cy),
                        count: members.len(),
                        lat,
                        lon,
                        bbox,
                        top_priority: members[0].priority,
                        priority_counts: counts,
                        member_ids: members.iter().take(MAX_CLUSTER_MEMBERS).map(|m| m.alert_id.clone()).collect(),
                    }
                })
                .collect();
            return (feats, clustered);
        }
        cell_px *= 2.0;
    }
}
