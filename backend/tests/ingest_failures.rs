//! Spec §15.3 ingestion failure tests against a local mock server.
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use wda::ingest::http_client::{build_client, fetch_once, ip_prohibited, FetchError, FetchOutcome};
use wda::ingest::source_registry::{SourceConfig, SourceFormat};

async fn serve() -> SocketAddr {
    let app = Router::new()
        .route(
            "/ok",
            get(|h: HeaderMap| async move {
                if h.get("if-none-match").and_then(|v| v.to_str().ok()) == Some("\"v1\"") {
                    return StatusCode::NOT_MODIFIED.into_response();
                }
                ([("etag", "\"v1\"")], "{\"type\":\"FeatureCollection\",\"features\":[]}").into_response()
            }),
        )
        .route("/limited", get(|| async { (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "120")], "slow down") }))
        .route("/broken", get(|| async { StatusCode::SERVICE_UNAVAILABLE }))
        .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
        .route("/big", get(|| async { "x".repeat(10_000) }))
        .route("/redirect-out", get(|| async { (StatusCode::FOUND, [("location", "http://evil.invalid/x")]) }))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                "late"
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

fn src(addr: SocketAddr, path: &str, max: usize, timeout: u64) -> SourceConfig {
    let toml = format!(
        r#"
source_id = "mock"
agency_name = "Mock"
country_or_region = "Test"
official_homepage = "http://127.0.0.1/"
feed_url = "http://{addr}{path}"
format = "geojson_usgs"
hazard_types = ["earthquake"]
geographic_coverage = "test"
poll_interval_seconds = 60
request_timeout_seconds = {timeout}
max_response_bytes = {max}
expected_freshness_seconds = 60
terms_or_redistribution_notes = "test"
adapter_name = "usgs_geojson"
enabled = true
publisher_class = "scientific"
authority_role = "reference_only"
redistribution_status = "review_pending"
coverage_state = "integrated"
onboarding_state = "discovered"
"#
    );
    let s: SourceConfig = toml::from_str(&toml).unwrap();
    assert_eq!(s.format, SourceFormat::GeojsonUsgs);
    s
}

#[tokio::test]
async fn http_failure_matrix() {
    let addr = serve().await;
    let client = build_client("wda-test", &[], true).unwrap();
    // 200 then 304 with ETag
    let s = src(addr, "/ok", 1 << 20, 5);
    match fetch_once(&client, &s, None, None, true).await.unwrap() {
        FetchOutcome::Body { etag, .. } => assert_eq!(etag.as_deref(), Some("\"v1\"")),
        o => panic!("{o:?}"),
    }
    assert!(matches!(fetch_once(&client, &s, Some("\"v1\""), None, true).await.unwrap(), FetchOutcome::NotModified));
    // 429 + Retry-After
    let e = fetch_once(&client, &src(addr, "/limited", 1 << 20, 5), None, None, true).await.unwrap_err();
    assert_eq!(e, FetchError::Status { code: 429, retry_after_s: Some(120) });
    assert!(!e.is_permanent());
    // 5xx is transient, 404 is permanent
    let e = fetch_once(&client, &src(addr, "/broken", 1 << 20, 5), None, None, true).await.unwrap_err();
    assert!(matches!(e, FetchError::Status { code: 503, .. }) && !e.is_permanent());
    let e = fetch_once(&client, &src(addr, "/missing", 1 << 20, 5), None, None, true).await.unwrap_err();
    assert!(e.is_permanent());
    // oversized body
    let e = fetch_once(&client, &src(addr, "/big", 1000, 5), None, None, true).await.unwrap_err();
    assert_eq!(e, FetchError::TooLarge(1000));
    // redirect outside allowlist
    let e = fetch_once(&client, &src(addr, "/redirect-out", 1 << 20, 5), None, None, true).await.unwrap_err();
    assert!(matches!(e, FetchError::Redirect(_)), "{e:?}");
    // timeout
    let e = fetch_once(&client, &src(addr, "/slow", 1 << 20, 1), None, None, true).await.unwrap_err();
    assert_eq!(e, FetchError::Timeout);
}

#[tokio::test]
async fn dns_failure_is_classified() {
    let client = build_client("wda-test", &[], true).unwrap();
    let mut s = src("127.0.0.1:1".parse().unwrap(), "/", 1000, 5);
    s.feed_url = "http://does-not-exist.invalid/feed".into();
    let e = fetch_once(&client, &s, None, None, true).await.unwrap_err();
    assert!(matches!(e, FetchError::Connect(_)), "{e:?}");
}

#[tokio::test]
async fn private_destinations_are_blocked_by_default() {
    let addr = serve().await;
    let client = build_client("wda-test", &[], false).unwrap();
    let e = fetch_once(&client, &src(addr, "/ok", 1000, 5), None, None, false).await.unwrap_err();
    assert!(matches!(e, FetchError::Prohibited(_) | FetchError::Other(_)), "{e:?}");
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "192.168.1.1",
        "100.64.0.1",
        "::1",
        "fe80::1",
        "fc00::1",
        "::ffff:127.0.0.1",
        "0.0.0.0",
    ] {
        assert!(ip_prohibited(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["8.8.8.8", "2001:4860:4860::8888"] {
        assert!(!ip_prohibited(ip.parse().unwrap()), "{ip}");
    }
}

#[test]
fn malformed_json_and_non_geojson_fail_cleanly() {
    assert!(wda::ingest::usgs::geojson::parse_usgs(b"{not json").is_err());
    assert!(wda::ingest::usgs::geojson::parse_usgs(br#"{"type":"Point"}"#).is_err());
}
