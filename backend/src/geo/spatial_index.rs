//! Combined in-memory alert store + R*-tree (spec §3.2, §6.4).
//! Map and index are updated together by the single writer under one short
//! synchronous write lock; no `.await` is ever held across it.

use crate::domain::alert::AlertRecord;
use crate::domain::geometry::BBox;
use crate::ingest::source_registry::SourceHealth;
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{RTree, AABB};
use std::collections::HashMap;
use std::sync::Arc;

/// R-tree entry: bounding rectangle + (alert id, geometry part id). x = lon, y = lat.
pub type Entry = GeomWithData<Rectangle<[f64; 2]>, (Arc<str>, u32)>;

#[derive(Default)]
pub struct AlertIndex {
    /// All known records (live and historical) keyed by alert id.
    pub alerts: HashMap<String, Arc<AlertRecord>>,
    tree: RTree<Entry>,
    entries: HashMap<String, Vec<Entry>>,
    /// Event sequence of the last committed change reflected here.
    pub seq: u64,
    pub sources: HashMap<String, SourceHealth>,
}

impl AlertIndex {
    /// Bulk-load from persisted records at startup (spec §24.4).
    pub fn rebuild(records: Vec<AlertRecord>, seq: u64, sources: HashMap<String, SourceHealth>) -> Self {
        let mut all = Vec::new();
        let mut entries: HashMap<String, Vec<Entry>> = HashMap::new();
        let mut alerts = HashMap::new();
        for r in records {
            let es = entries_for(&r);
            if !es.is_empty() {
                all.extend(es.iter().cloned());
                entries.insert(r.alert_id.clone(), es);
            }
            alerts.insert(r.alert_id.clone(), Arc::new(r));
        }
        Self { alerts, tree: RTree::bulk_load(all), entries, seq, sources }
    }

    /// Apply committed revisions (must only be called after a successful commit).
    pub fn apply(&mut self, records: &[AlertRecord], seq: u64) {
        for r in records {
            if let Some(old) = self.entries.remove(&r.alert_id) {
                for e in old {
                    self.tree.remove(&e);
                }
            }
            let es = entries_for(r);
            for e in &es {
                self.tree.insert(e.clone());
            }
            if !es.is_empty() {
                self.entries.insert(r.alert_id.clone(), es);
            }
            self.alerts.insert(r.alert_id.clone(), Arc::new(r.clone()));
        }
        if seq > self.seq {
            self.seq = seq;
        }
    }

    pub fn set_source(&mut self, id: &str, h: SourceHealth) {
        self.sources.insert(id.to_string(), h);
    }

    /// Candidate alert ids whose envelopes intersect `b` (non-wrapped box).
    pub fn candidates(&self, b: &BBox) -> Vec<(Arc<str>, u32)> {
        let env = AABB::from_corners([b.min_lon, b.min_lat], [b.max_lon, b.max_lat]);
        self.tree.locate_in_envelope_intersecting(&env).map(|e| e.data.clone()).collect()
    }

    pub fn live_count(&self) -> usize {
        self.entries.len()
    }
}

/// Only live records (active/upcoming/stale) are indexed for the map.
fn entries_for(r: &AlertRecord) -> Vec<Entry> {
    if !r.status.is_live() {
        return vec![];
    }
    let id: Arc<str> = Arc::from(r.alert_id.as_str());
    let mut out = Vec::new();
    for (i, part) in r.geometry_parts.iter().enumerate() {
        for b in part.index_boxes() {
            let rect = Rectangle::from_corners([b.min_lon, b.min_lat], [b.max_lon, b.max_lat]);
            out.push(GeomWithData::new(rect, (id.clone(), i as u32)));
        }
    }
    out
}
