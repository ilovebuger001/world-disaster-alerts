//! Spec §15.1 CAP tests and Phase 1 lifecycle exit conditions.
mod common;
use common::*;
use wda::domain::alert::{AlertStatus, DisplayPriority};
use wda::domain::geometry::GeometryPart;
use wda::domain::lifecycle::{ApplyCtx, ApplyOutcome, MemStore, SkipReason};
use wda::domain::message::MessageBody;
use wda::ingest::cap;
use wda::ingest::cap::parser::{parse_cap_document, CapError, CapLimits};

fn apply(store: &mut MemStore, name: &str) -> ApplyOutcome {
    let m = mapping();
    let ctx = ApplyCtx { source_id: "sample-cap", source_agency: "Sample Agency", mapping: &m, now_ms: NOW_MS };
    let batch = cap::parse("sample-cap", "https://example.invalid/feed.xml", &cap(name), NOW_MS).expect("parse");
    let mut total = ApplyOutcome::default();
    for msg in &batch.messages {
        let MessageBody::Cap(c) = &msg.body else { panic!("cap") };
        let o = cap::lifecycle::apply_cap(store, msg, c, &ctx).expect("apply");
        total.changed.extend(o.changed);
        total.skipped.extend(o.skipped);
    }
    total
}

#[test]
fn parses_multiple_infos_areas_and_geometry() {
    let docs = parse_cap_document(&cap("alert.xml"), &CapLimits::default()).unwrap();
    let c = docs[0].as_ref().unwrap();
    assert_eq!(c.infos.len(), 2);
    assert_eq!(c.infos[0].language, "th-TH");
    assert_eq!(c.infos[0].areas.len(), 2);
    let a = &c.infos[0].areas[0];
    assert_eq!(a.polygons[0].len(), 5);
    assert_eq!(a.circles.len(), 1);
    assert_eq!(a.geocodes, vec![("SAMPLE-CODE".to_string(), "50".to_string())]);
    assert!(!c.has_signature);
}

#[test]
fn alert_then_update_then_cancel_keeps_history() {
    let mut s = MemStore::default();
    let o1 = apply(&mut s, "alert.xml");
    assert_eq!(o1.changed.len(), 1);
    let a1 = &o1.changed[0];
    assert_eq!(a1.status, AlertStatus::Active);
    assert_eq!(a1.revision, 1);
    assert_eq!(a1.display_priority, DisplayPriority::Warning);
    assert_eq!(a1.hazard_type, "flood");
    assert_eq!(a1.language_variants.len(), 2);
    // duplicated polygon across languages is stored once; text-only area kept
    let polys = a1.geometry_parts.iter().filter(|g| matches!(g, GeometryPart::Polygon { .. })).count();
    assert_eq!(polys, 1);
    assert!(a1.geometry_parts.iter().any(|g| matches!(g, GeometryPart::TextOnly { .. })));
    assert!(a1.geometry_parts.iter().any(|g| matches!(g, GeometryPart::Geocode { .. })));

    let o2 = apply(&mut s, "update.xml");
    assert_eq!(o2.changed.len(), 1);
    let a2 = &o2.changed[0];
    assert_eq!(a2.alert_id, a1.alert_id, "update follows exact reference triple");
    assert_eq!(a2.revision, 2);
    assert_eq!(a2.display_priority, DisplayPriority::Emergency);
    assert_eq!(a2.message_type, "Update");

    let o3 = apply(&mut s, "cancel.xml");
    let a3 = &o3.changed[0];
    assert_eq!(a3.alert_id, a1.alert_id);
    assert_eq!(a3.status, AlertStatus::Cancelled);
    assert_eq!(a3.revision, 3);
    assert_eq!(a3.message_history.len(), 3, "cancelled message history preserved");
    assert!(!a3.language_variants.is_empty(), "cancel without info keeps original wording");
    assert_eq!(s.messages.len(), 3);
}

#[test]
fn duplicate_fetch_is_idempotent() {
    let mut s = MemStore::default();
    apply(&mut s, "alert.xml");
    let again = apply(&mut s, "alert.xml");
    assert!(again.changed.is_empty());
    assert_eq!(again.skipped[0].1, SkipReason::DuplicateMessage);
    assert_eq!(s.alerts.len(), 1);
}

#[test]
fn out_of_order_update_before_alert_does_not_roll_back() {
    let mut s = MemStore::default();
    let o1 = apply(&mut s, "update.xml");
    let id = o1.changed[0].alert_id.clone();
    let o2 = apply(&mut s, "alert.xml");
    assert!(o2.changed.is_empty(), "older original must not overwrite newer update");
    assert_eq!(o2.skipped[0].1, SkipReason::OlderThanApplied);
    let a = &s.alerts[&id];
    assert_eq!(a.revision, 1);
    assert_eq!(a.display_priority, DisplayPriority::Emergency);
    assert_eq!(a.message_history.len(), 2, "older message still recorded in history");
    assert_eq!(s.alerts.len(), 1);
}

#[test]
fn cancel_arriving_first_wins() {
    let mut s = MemStore::default();
    apply(&mut s, "cancel.xml");
    apply(&mut s, "update.xml");
    apply(&mut s, "alert.xml");
    assert_eq!(s.alerts.len(), 1);
    let a = s.alerts.values().next().unwrap();
    assert_eq!(a.status, AlertStatus::Cancelled);
}

#[test]
fn test_exercise_restricted_and_ack_never_become_public() {
    let mut s = MemStore::default();
    for (f, reason) in [
        ("test_status.xml", SkipReason::NotActual("Test".into())),
        ("exercise.xml", SkipReason::NotActual("Exercise".into())),
        ("restricted.xml", SkipReason::NotPublic("Restricted".into())),
        ("ack.xml", SkipReason::NonAlertMessageType("Ack".into())),
    ] {
        let o = apply(&mut s, f);
        assert!(o.changed.is_empty(), "{f}");
        assert_eq!(o.skipped[0].1, reason, "{f}");
    }
    assert!(s.alerts.is_empty());
    assert_eq!(s.messages.len(), 4, "messages kept for audit/test tooling");
}

#[test]
fn malformed_polygon_is_rejected_not_coerced() {
    let docs = parse_cap_document(&cap("malformed_polygon.xml"), &CapLimits::default()).unwrap();
    assert!(matches!(docs[0], Err(CapError::Invalid { field: "polygon", .. })));
}

#[test]
fn dtd_and_external_entities_are_rejected() {
    assert_eq!(parse_cap_document(&cap("dtd.xml"), &CapLimits::default()).unwrap_err(), CapError::DtdRejected);
}

#[test]
fn wrong_namespace_is_not_cap12() {
    assert_eq!(parse_cap_document(&cap("wrong_namespace.xml"), &CapLimits::default()).unwrap_err(), CapError::NoAlert);
}

#[test]
fn feed_with_one_bad_alert_keeps_the_good_one() {
    let docs = parse_cap_document(&cap("feed_multi.xml"), &CapLimits::default()).unwrap();
    assert_eq!(docs.len(), 2);
    assert!(docs[0].is_ok());
    assert_eq!(docs[1], Err(CapError::Missing("sender")));
}

#[test]
fn timezone_offsets_normalize_to_utc_and_missing_expiry_stays_unknown() {
    let docs = parse_cap_document(&cap("feed_multi.xml"), &CapLimits::default()).unwrap();
    let c = docs[0].as_ref().unwrap();
    assert_eq!(c.sent_ms, 1_794_488_400_000); // 2026-11-12T13:00:00Z
    let mut s = MemStore::default();
    let o = apply(&mut s, "feed_multi.xml");
    assert_eq!(o.changed[0].expires_at_ms, None);
    assert_eq!(o.changed[0].hazard_type, "tsunami");
}

#[test]
fn limits_are_enforced() {
    let limits = CapLimits { max_depth: 2, ..CapLimits::default() };
    assert_eq!(parse_cap_document(&cap("alert.xml"), &limits).unwrap_err(), CapError::Limit("xml depth"));
}

#[test]
fn expiry_is_source_stated_and_upcoming_before_onset() {
    use wda::domain::lifecycle::derive_status;
    assert_eq!(derive_status(100, Some(0), None, Some(50), false, false), AlertStatus::Expired);
    assert_eq!(derive_status(100, Some(0), Some(200), None, false, false), AlertStatus::Upcoming);
    assert_eq!(derive_status(100, Some(0), None, None, false, false), AlertStatus::Active);
    assert_eq!(derive_status(100, Some(0), None, None, true, false), AlertStatus::Cancelled);
}
