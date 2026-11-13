//! NWS API adapter: CAP fields in GeoJSON → exact CAP lifecycle.
mod common;
use common::*;
use wda::domain::alert::{AlertStatus, DisplayPriority};
use wda::domain::geometry::GeometryPart;
use wda::domain::lifecycle::{ApplyCtx, ApplyOutcome, MemStore};
use wda::domain::message::MessageBody;
use wda::ingest::{cap, nws};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/nws/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn apply(store: &mut MemStore, name: &str) -> (ApplyOutcome, Vec<String>) {
    let m = mapping();
    let ctx = ApplyCtx { source_id: "nws-alerts", source_agency: "US National Weather Service", mapping: &m, now_ms: NOW_MS };
    let batch = nws::parse("nws-alerts", "https://api.weather.gov/alerts/active", &fixture(name), NOW_MS).expect("parse");
    let mut total = ApplyOutcome::default();
    for msg in &batch.messages {
        let MessageBody::Cap(c) = &msg.body else { panic!("cap") };
        let o = cap::lifecycle::apply_cap(store, msg, c, &ctx).expect("apply");
        total.changed.extend(o.changed);
        total.skipped.extend(o.skipped);
    }
    (total, batch.rejected)
}

#[test]
fn parses_polygon_and_zone_alerts_and_rejects_invalid() {
    let b = nws::parse("nws-alerts", "https://api.weather.gov/alerts/active", &fixture("active.json"), NOW_MS).unwrap();
    assert_eq!(b.messages.len(), 3, "invalid 'sent' is rejected, not coerced");
    assert_eq!(b.rejected.len(), 1);
    assert!(b.rejected[0].contains("sent"));
    let MessageBody::Cap(c) = &b.messages[0].body else { panic!() };
    assert_eq!(c.msg_type, "Alert");
    assert_eq!(c.infos[0].areas[0].polygons[0].len(), 5);
    assert!((c.infos[0].areas[0].polygons[0][0].lon_deg - -97.5).abs() < 1e-9, "GeoJSON [lon, lat] order");
    assert!(c.infos[0].parameters.iter().any(|(k, _)| k == "NWS_ends"));
    assert!(b.messages[0].source_url.starts_with("https://api.weather.gov/alerts/"));
    assert!(nws::parse("x", "u", b"{\"type\":\"Feature\"}", NOW_MS).is_err());
    assert!(nws::parse("x", "u", b"not json", NOW_MS).is_err());
}

#[test]
fn lifecycle_hazards_test_messages_and_cancel() {
    let mut s = MemStore::default();
    let (o, _) = apply(&mut s, "active.json");
    assert_eq!(o.changed.len(), 2, "Test status is never published");
    let tor = o.changed.iter().find(|a| a.hazard_raw.contains("Tornado")).unwrap();
    assert_eq!(tor.hazard_type, "tornado");
    assert_eq!(tor.display_priority, DisplayPriority::Emergency);
    assert!(tor.geometry_parts.iter().any(|g| matches!(g, GeometryPart::Polygon { .. })));
    let win = o.changed.iter().find(|a| a.hazard_raw.contains("Winter")).unwrap();
    assert_eq!(win.hazard_type, "winter");
    assert!(win.geometry_parts.iter().all(|g| !matches!(g, GeometryPart::Polygon { .. })), "no invented polygon for zone alerts");
    assert!(win.geometry_parts.iter().any(|g| matches!(g, GeometryPart::Geocode { value_name, .. } if value_name == "UGC")));

    let (o2, _) = apply(&mut s, "cancel.json");
    assert_eq!(o2.changed.len(), 1);
    assert_eq!(o2.changed[0].alert_id, tor.alert_id, "cancel follows the exact reference triple");
    assert_eq!(o2.changed[0].status, AlertStatus::Cancelled);
    assert!(o2.changed[0].cancelled);
}

#[tokio::test(flavor = "multi_thread")]
async fn zone_only_alerts_are_listed_not_hidden() {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let mut app: wda::config::AppConfig = toml::from_str(include_str!("../config/app.toml")).unwrap();
    app.data_dir = dir.path().to_path_buf();
    let sources: wda::ingest::source_registry::SourcesFile = toml::from_str(include_str!("../config/sources.example.toml")).unwrap();
    let gazetteer = toml::from_str(include_str!("../config/gazetteer.toml")).unwrap();
    let l = wda::config::Loaded { app, sources: sources.sources, mapping: mapping(), gazetteer, reference: reference() };
    let running = wda::app::start(&l, false).unwrap();
    let msgs = nws::parse("nws-alerts-active", "https://api.weather.gov/alerts/active", &fixture("active.json"), NOW_MS).unwrap().messages;
    running.writer.ingest("nws-alerts-active", "US National Weather Service", msgs, Default::default()).await.unwrap();
    let router = wda::app::router(running.state.clone(), None);
    let get = |u: &str| axum::http::Request::builder().uri(u).body(axum::body::Body::empty()).unwrap();
    let res = router.clone().oneshot(get("/api/v1/alerts-unmapped")).await.unwrap();
    let b: serde_json::Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(b["total"], 1, "the zone-only winter storm warning is listed");
    assert!(b["items"][0]["headline"].as_str().unwrap().contains("Winter Storm"));
    // Jurisdiction: NWS is a national warning issuer for the US → US contacts in details.
    let id = b["items"][0]["alert_id"].as_str().unwrap().to_string();
    let res = router.clone().oneshot(get(&format!("/api/v1/alerts/{id}"))).await.unwrap();
    let d: serde_json::Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(d["jurisdiction"]["country_code"], "US");
    assert_eq!(d["country_contacts"]["contacts"][0]["tel_uri"], "tel:911");
    let res = router.oneshot(get("/api/v1/alerts-unmapped?q=bighorn")).await.unwrap();
    let b: serde_json::Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(b["total"], 1);
}
