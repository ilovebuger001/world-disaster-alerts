//! Antimeridian-crossing polygons and viewports (spec §6.4, §15.2, §23.3).
mod common;
use common::*;
use wda::domain::alert::AlertRecord;
use wda::domain::geometry::*;
use wda::domain::lifecycle::{ApplyCtx, MemStore};
use wda::domain::message::MessageBody;
use wda::geo::antimeridian::query_boxes;
use wda::geo::spatial_index::AlertIndex;
use wda::geo::validate::Viewport;
use wda::geo::viewport::{query, Filters};

fn feed_records() -> Vec<AlertRecord> {
    let m = mapping();
    // Use a "now" after the feed's sent time so the record is active.
    let ctx = ApplyCtx { source_id: "feed", source_agency: "Feed", mapping: &m, now_ms: 1_794_500_000_000 };
    let batch = wda::ingest::cap::parse("feed", "https://example.invalid", &cap("feed_multi.xml"), ctx.now_ms).unwrap();
    let mut s = MemStore::default();
    for msg in &batch.messages {
        let MessageBody::Cap(c) = &msg.body else { unreachable!() };
        wda::ingest::cap::lifecycle::apply_cap(&mut s, msg, c, &ctx).unwrap();
    }
    s.alerts.into_values().collect()
}

#[test]
fn wrapped_viewport_splits_into_two_boxes() {
    let v = Viewport { west: 170.0, south: -30.0, east: -170.0, north: 0.0, zoom: 4.0 }.validate().unwrap();
    let b = query_boxes(&v);
    assert_eq!(b.len(), 2);
    assert_eq!((b[0].min_lon, b[0].max_lon), (170.0, 180.0));
    assert_eq!((b[1].min_lon, b[1].max_lon), (-180.0, -170.0));
}

#[test]
fn crossing_polygon_is_indexed_on_both_sides() {
    let recs = feed_records();
    let poly = recs[0].geometry_parts.iter().find(|g| matches!(g, GeometryPart::Polygon { .. })).unwrap();
    let boxes = poly.index_boxes();
    assert_eq!(boxes.len(), 2, "split at the antimeridian, not a 358° wide box");
    assert!(boxes.iter().any(|b| b.min_lon >= 179.0 && b.max_lon == 180.0));
    assert!(boxes.iter().any(|b| b.min_lon == -180.0 && b.max_lon <= -179.0));
}

#[test]
fn viewport_queries_find_crossing_alert_from_either_side_and_wrapped() {
    let recs = feed_records();
    let idx = AlertIndex::rebuild(recs, 1, Default::default());
    let f = Filters::default();
    for v in [
        Viewport { west: 175.0, south: -20.0, east: 180.0, north: -10.0, zoom: 5.0 },
        Viewport { west: -180.0, south: -20.0, east: -175.0, north: -10.0, zoom: 5.0 },
        Viewport { west: 178.0, south: -20.0, east: -178.0, north: -10.0, zoom: 5.0 },
    ] {
        let r = query(&idx, &v, &f, 20, None);
        assert_eq!(r.total_matching, 1, "{v:?}");
    }
    // A plain (non-wrapped) viewport elsewhere must not match.
    let r = query(&idx, &Viewport { west: 0.0, south: -20.0, east: 10.0, north: -10.0, zoom: 5.0 }, &f, 20, None);
    assert_eq!(r.total_matching, 0);
}

#[test]
fn invalid_viewports_are_rejected() {
    assert!(Viewport { west: 0.0, south: 10.0, east: 1.0, north: 5.0, zoom: 3.0 }.validate().is_err());
    assert!(Viewport { west: 0.0, south: 0.0, east: 181.0, north: 5.0, zoom: 3.0 }.validate().is_err());
    assert!(Viewport { west: 0.0, south: 0.0, east: 1.0, north: 5.0, zoom: 30.0 }.validate().is_err());
    assert!(Viewport { west: f64::NAN, south: 0.0, east: 1.0, north: 5.0, zoom: 3.0 }.validate().is_err());
}

#[test]
fn clustering_caps_visible_features_and_keeps_list_complete() {
    use wda::domain::alert::{AlertStatus, DisplayPriority};
    let base = feed_records().remove(0);
    let mut recs = Vec::new();
    for i in 0..300 {
        let mut a = base.clone();
        a.alert_id = format!("{:024x}", i + 1);
        a.status = AlertStatus::Active;
        a.display_priority = if i % 2 == 0 { DisplayPriority::Warning } else { DisplayPriority::Watch };
        a.geometry_parts =
            vec![GeometryPart::Point { point: GeoPoint { lat_deg: (i % 30) as f64 - 15.0, lon_deg: (i / 30) as f64 * 3.0 } }];
        recs.push(a);
    }
    let idx = AlertIndex::rebuild(recs, 7, Default::default());
    let v = Viewport { west: -180.0, south: -85.0, east: 180.0, north: 85.0, zoom: 2.0 };
    let r = query(&idx, &v, &Filters::default(), 20, None);
    assert_eq!(r.total_matching, 300);
    assert!(r.features.len() <= 50, "visible map features capped at 50");
    assert!(r.clustered);
    assert_eq!(r.event_sequence, 7);
    // walk the paginated list: every record is reachable
    let mut seen = 0;
    let mut cursor = None;
    loop {
        let page = query(&idx, &v, &Filters::default(), 100, cursor.clone());
        seen += page.items.len();
        match page.next_cursor {
            Some(c) => cursor = wda::geo::viewport::decode_cursor(&c),
            None => break,
        }
    }
    assert_eq!(seen, 300);
}
