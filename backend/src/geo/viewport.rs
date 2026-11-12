//! Viewport query (spec §6.4 steps 1–8, §9.2, §56 items 1–2).
//! Candidate cap (pre-clustering) and visible feature cap are separate; the
//! paginated list keeps every qualifying record even when the map is capped.

use super::antimeridian::query_boxes;
use super::clustering::{cluster, ClusterInput, MapFeature};
use super::spatial_index::AlertIndex;
use super::validate::Viewport;
use crate::domain::alert::{AlertRecord, AlertStatus, DisplayPriority};
use crate::domain::geometry::GeoPoint;
use std::collections::HashSet;
use std::sync::Arc;

pub const CANDIDATE_CAP: usize = 500;
pub const MAX_MAP_FEATURES: usize = 50;
pub const DEFAULT_PAGE: usize = 20;
pub const MAX_PAGE: usize = 100;
pub const CELL_PX: f64 = 64.0;

#[derive(Clone, Debug, Default)]
pub struct Filters {
    pub hazard_types: Option<HashSet<String>>,
    pub priorities: Option<HashSet<DisplayPriority>>,
    pub statuses: Option<HashSet<AlertStatus>>,
    /// Lower bound on source publication/update time (ms).
    pub since_ms: Option<i64>,
}

impl Filters {
    pub fn matches(&self, a: &AlertRecord) -> bool {
        let live_default = self.statuses.is_none() && !a.status.is_live();
        if live_default {
            return false;
        }
        if let Some(s) = &self.statuses {
            if !s.contains(&a.status) {
                return false;
            }
        }
        if let Some(h) = &self.hazard_types {
            if !h.contains(&a.hazard_type) {
                return false;
            }
        }
        if let Some(p) = &self.priorities {
            if !p.contains(&a.display_priority) {
                return false;
            }
        }
        if let Some(since) = self.since_ms {
            let t = a.source_updated_at_ms.unwrap_or(a.source_published_at_ms);
            if t < since {
                return false;
            }
        }
        true
    }
}

pub struct ViewportResult {
    pub event_sequence: u64,
    pub features: Vec<MapFeature>,
    pub clustered: bool,
    pub candidates_capped: bool,
    pub total_matching: usize,
    pub items: Vec<Arc<AlertRecord>>,
    pub next_cursor: Option<String>,
}

fn sort_key(a: &AlertRecord) -> (u8, i64, String) {
    (a.display_priority.rank(), -a.source_updated_at_ms.unwrap_or(a.source_published_at_ms), a.alert_id.clone())
}

pub fn encode_cursor(a: &AlertRecord) -> String {
    let (r, t, id) = sort_key(a);
    format!("{r}.{t}.{id}")
}

pub fn decode_cursor(c: &str) -> Option<(u8, i64, String)> {
    let mut it = c.splitn(3, '.');
    let r = it.next()?.parse().ok()?;
    let t = it.next()?.parse().ok()?;
    let id = it.next()?.to_string();
    if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some((r, t, id))
}

/// Runs entirely under the caller's read lock (no `.await`).
pub fn query(idx: &AlertIndex, v: &Viewport, f: &Filters, page: usize, cursor: Option<(u8, i64, String)>) -> ViewportResult {
    let boxes = query_boxes(v);
    let mut seen: HashSet<Arc<str>> = HashSet::new();
    let mut matched: Vec<(Arc<AlertRecord>, Option<GeoPoint>)> = Vec::new();
    for b in &boxes {
        for (id, part) in idx.candidates(b) {
            let Some(rec) = idx.alerts.get(id.as_ref()) else { continue };
            let Some(geom) = rec.geometry_parts.get(part as usize) else { continue };
            // exact test (step 5)
            if !geom.intersects_box(b) {
                continue;
            }
            if !seen.insert(id.clone()) {
                continue; // dedupe by alert id (step 6)
            }
            if !f.matches(rec) {
                continue; // filters (step 7)
            }
            matched.push((rec.clone(), geom.representative_point()));
        }
    }
    // deterministic ordering
    matched.sort_by(|a, b| sort_key(&a.0).cmp(&sort_key(&b.0)));
    let total = matched.len();

    // map features: bounded candidates, clustered after filtering (step 8)
    let capped = total > CANDIDATE_CAP;
    let inputs: Vec<ClusterInput> = matched
        .iter()
        .take(CANDIDATE_CAP)
        .filter_map(|(a, p)| p.map(|p| ClusterInput { alert_id: a.alert_id.clone(), point: p, priority: a.display_priority }))
        .collect();
    let (features, clustered) = cluster(&inputs, v.zoom.floor(), CELL_PX, MAX_MAP_FEATURES);

    // paginated list (independent of the map cap)
    let page = page.clamp(1, MAX_PAGE);
    let start = match &cursor {
        Some(c) => matched.iter().position(|(a, _)| sort_key(a) > *c).unwrap_or(total),
        None => 0,
    };
    let items: Vec<Arc<AlertRecord>> = matched.iter().skip(start).take(page).map(|(a, _)| a.clone()).collect();
    let next_cursor = if start + items.len() < total { items.last().map(|a| encode_cursor(a)) } else { None };

    ViewportResult { event_sequence: idx.seq, features, clustered, candidates_capped: capped, total_matching: total, items, next_cursor }
}
