//! HTTP API contract tests (spec §9).
mod common;
use common::*;
use http_body_util::BodyExt;
use tower::ServiceExt;
use wda::config::{validate, AppConfig, Loaded};
use wda::ingest::source_registry::{SourceHealth, SourcesFile};

fn loaded(dir: &std::path::Path) -> Loaded {
    let mut app: AppConfig = toml::from_str(include_str!("../config/app.toml")).unwrap();
    app.data_dir = dir.to_path_buf();
    let sources: SourcesFile = toml::from_str(include_str!("../config/sources.example.toml")).unwrap();
    let m = mapping();
    validate(&app, &sources.sources, &m).unwrap();
    let gazetteer = toml::from_str(include_str!("../config/gazetteer.toml")).unwrap();
    Loaded { app, sources: sources.sources, mapping: m, gazetteer }
}

async fn get(app: &axum::Router, uri: &str) -> (u16, serde_json::Value) {
    let res = app.clone().oneshot(axum::http::Request::builder().uri(uri).body(axum::body::Body::empty()).unwrap()).await.unwrap();
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

#[tokio::test(flavor = "multi_thread")]
async fn endpoints_and_error_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let l = loaded(dir.path());
    let running = wda::app::start(&l, false).unwrap();
    let app = wda::app::router(running.state.clone(), None);

    let (s, b) = get(&app, "/health/live").await;
    assert_eq!((s, b["status"].as_str()), (200, Some("live")));
    let (s, _) = get(&app, "/health/ready").await;
    assert_eq!(s, 200);

    let msgs = wda::ingest::cap::parse("sample-cap", "https://example.invalid", &cap("alert.xml"), NOW_MS).unwrap().messages;
    let c = running.writer.ingest("sample-cap", "Sample", msgs, SourceHealth::default()).await.unwrap();
    let id = c.changed[0].alert_id.clone();

    let (s, b) = get(&app, "/api/v1/alerts?west=98&south=18&east=100&north=20&zoom=8").await;
    assert_eq!(s, 200);
    assert_eq!(b["event_sequence"], 1);
    assert_eq!(b["total_matching"], 1);
    assert_eq!(b["items"][0]["alert_id"], id.as_str());
    assert!(b["items"][0]["point"]["lat"].is_number());
    assert_eq!(b["features"].as_array().unwrap().len(), 1);
    assert!(b["next_cursor"].is_null());

    // antimeridian-wrapped viewport is valid
    let (s, _) = get(&app, "/api/v1/alerts?west=170&south=-30&east=-170&north=0&zoom=3").await;
    assert_eq!(s, 200);

    let (s, b) = get(&app, &format!("/api/v1/alerts/{id}")).await;
    assert_eq!(s, 200);
    assert_eq!(b["alert"]["revision"], 1);
    assert_eq!(b["alert"]["language_variants"][0]["language"], "th-TH");
    assert_eq!(b["signature_verified"], false);
    assert_eq!(b["translations"]["status"], "unavailable");

    let (s, b) = get(&app, "/api/v1/alerts?west=0&south=10&east=1&north=5&zoom=3").await;
    assert_eq!(s, 400);
    assert_eq!(b["error"]["code"], "invalid_viewport");
    assert!(b["error"]["request_id"].is_string());

    let (s, b) = get(&app, "/api/v1/alerts/abcdef0123456789abcdef01").await;
    assert_eq!((s, b["error"]["code"].as_str()), (404, Some("not_found")));
    let (s, _) = get(&app, "/api/v1/alerts/../../etc").await;
    assert!(s == 400 || s == 404);

    let (s, b) = get(&app, "/api/v1/meta").await;
    assert_eq!(s, 200);
    assert_eq!(b["api_version"], "v1");
    assert_eq!(b["event_sequence"], 1);
    assert_ne!(b["freshness"]["overall"], "live", "never live without fresh sources");

    let (s, b) = get(&app, "/api/v1/sources").await;
    assert_eq!(s, 200);
    let usgs = b["sources"].as_array().unwrap().iter().find(|x| x["source_id"] == "usgs-earthquakes-all-day").unwrap();
    assert_eq!(usgs["public_redistribution"], false, "pending legal review is not public redistribution");

    let (s, b) = get(&app, "/api/v1/search?q=chiang").await;
    assert_eq!(s, 200);
    assert_eq!(b["areas"][0]["name_en"], "Chiang Mai");
    assert_eq!(b["disasters"][0]["alert_id"], id.as_str());
    let (s, _) = get(&app, "/api/v1/search?q=").await;
    assert_eq!(s, 400);

    let (s, b) = get(&app, "/api/v1/stream?after=abc").await;
    assert_eq!((s, b["error"]["code"].as_str()), (400, Some("invalid_cursor")));
}

#[test]
fn production_refuses_unreviewed_enabled_sources() {
    let mut app: AppConfig = toml::from_str(include_str!("../config/app.toml")).unwrap();
    app.environment = wda::config::Environment::Production;
    let sources: SourcesFile = toml::from_str(include_str!("../config/sources.example.toml")).unwrap();
    let err = validate(&app, &sources.sources, &mapping()).unwrap_err();
    assert!(err.to_string().contains("requires approved review"), "{err}");
}
