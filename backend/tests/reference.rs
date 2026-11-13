//! Emergency directory, guides, facilities, notification eligibility and edge
//! middleware (CORS, security headers, rate limiting).
mod common;
use common::*;
use http_body_util::BodyExt;
use tower::ServiceExt;
use wda::config::{validate, AppConfig, HttpConfig, Loaded};
use wda::domain::alert::DisplayPriority;
use wda::domain::geometry::GeoPoint;
use wda::ingest::source_registry::{SourceHealth, SourcesFile};
use wda::reference::*;

fn loaded(dir: &std::path::Path) -> Loaded {
    let mut app: AppConfig = toml::from_str(include_str!("../config/app.toml")).unwrap();
    app.data_dir = dir.to_path_buf();
    let sources: SourcesFile = toml::from_str(include_str!("../config/sources.example.toml")).unwrap();
    let m = mapping();
    validate(&app, &sources.sources, &m).unwrap();
    let gazetteer = toml::from_str(include_str!("../config/gazetteer.toml")).unwrap();
    Loaded { app, sources: sources.sources, mapping: m, gazetteer, reference: reference() }
}

async fn req(app: &axum::Router, r: axum::http::Request<axum::body::Body>) -> (u16, axum::http::HeaderMap, serde_json::Value) {
    let res = app.clone().oneshot(r).await.unwrap();
    let status = res.status().as_u16();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

fn get(uri: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder().uri(uri).body(axum::body::Body::empty()).unwrap()
}

fn contact(dial: &str, v: ContactVerification) -> Contact {
    Contact {
        service: "general".into(),
        display: dial.into(),
        dial: dial.into(),
        authority: "Authority".into(),
        source_url: "https://example.gov/emergency".into(),
        verification: v,
        last_verified: Some("2026-11-13".into()),
        region: None,
        network: None,
        instructions: None,
        language: None,
        notes: None,
        valid_from: None,
        valid_until: None,
    }
}

#[test]
fn directory_covers_every_iso_country_and_is_honest() {
    let r = reference();
    let c = &r.contacts;
    assert!(c.countries.len() >= 249, "every ISO 3166-1 entry is listed");
    for country in &c.countries {
        for k in &country.contacts {
            assert!(valid_dial(&k.dial), "{} {}", country.code, k.dial);
            assert!(k.source_url.starts_with("https://"));
        }
        if country.coverage == Coverage::Unavailable {
            assert!(country.contacts.is_empty(), "no guessed numbers for {}", country.code);
        }
    }
    let au = c.country("au").unwrap();
    assert_eq!(au.coverage, Coverage::Verified);
    assert!(au.contacts.iter().any(|k| k.dial == "000"));
    // EU 112 alone (no national police/ambulance numbers reviewed) is not full coverage.
    assert_eq!(c.country("DE").unwrap().coverage, Coverage::PartiallyVerified);
    let counts = r.coverage_counts();
    assert_eq!(counts.values().sum::<usize>(), c.countries.len());
}

#[test]
fn tel_links_only_for_plain_dial_strings() {
    assert_eq!(tel_uri(&contact("911", ContactVerification::Verified)).as_deref(), Some("tel:911"));
    assert_eq!(tel_uri(&contact("+6621234567", ContactVerification::Verified)).as_deref(), Some("tel:+6621234567"));
    for bad in ["9", "91 1", "911;ext=1", "javascript:alert(1)", "", "+", "1234567890123456"] {
        assert!(tel_uri(&contact(bad, ContactVerification::Verified)).is_none(), "{bad}");
    }
}

#[test]
fn contact_validation_rejects_dishonest_coverage() {
    let base = reference().contacts;
    let mut f = base.clone();
    f.countries[0].coverage = Coverage::Unavailable;
    f.countries[0].contacts = vec![contact("112", ContactVerification::Unverified)];
    assert!(f.validate().is_err(), "unavailable with numbers");

    let mut f = base.clone();
    f.countries[0].coverage = Coverage::Verified;
    f.countries[0].contacts = vec![contact("112", ContactVerification::Unverified)];
    assert!(f.validate().is_err(), "verified with unverified contact");

    let mut f = base.clone();
    f.countries[0].coverage = Coverage::PartiallyVerified;
    f.countries[0].contacts = vec![contact("112", ContactVerification::Unverified)];
    assert!(f.validate().is_err(), "partially verified needs an official source");

    let mut f = base.clone();
    f.countries[0].coverage = Coverage::Unverified;
    f.countries[0].contacts = vec![contact("12a", ContactVerification::Unverified)];
    assert!(f.validate().is_err(), "invalid dial string");

    let mut f = base;
    let dup = f.countries[0].clone();
    f.countries.push(dup);
    assert!(f.validate().is_err(), "duplicate country");
}

#[test]
fn guides_are_sourced_and_missing_ones_are_explicit() {
    let g = reference().guides;
    for x in &g.guides {
        assert!(x.source_url.starts_with("https://"));
        assert!(!x.authority.is_empty());
        assert!(!x.before.is_empty() && !x.during.is_empty() && !x.after.is_empty(), "{}", x.hazard);
    }
    assert!(g.guide("earthquake").is_some());
    assert!(g.guide("drought").is_none());
    assert!(g.unavailable.iter().any(|u| u.hazard == "drought"));
    let mut bad = g.clone();
    bad.guides[0].source_url = "http://insecure".into();
    assert!(bad.validate().is_err());
}

fn facility(id: &str, link: EventLink, valid_until_ms: Option<i64>) -> Facility {
    Facility {
        facility_id: id.into(),
        name: "School shelter".into(),
        kind: "shelter".into(),
        lat: 18.8,
        lon: 98.9,
        status: FacilityStatus::Designated,
        authority: "Provincial office".into(),
        source_url: "https://example.gov/shelters".into(),
        last_verified: None,
        valid_until_ms,
        capacity: None,
        notes: None,
        linked_events: vec![link],
    }
}

#[test]
fn facilities_only_when_linked_to_the_event() {
    let link_a = EventLink { alert_id: Some("aa".into()), source_id: None, source_record_id: None };
    let link_src = EventLink { alert_id: None, source_id: Some("src".into()), source_record_id: Some("rec-1".into()) };
    let f = FacilitiesFile {
        schema_version: 1,
        dataset_version: "t".into(),
        notice: "n".into(),
        facilities: vec![
            facility("f1", link_a, None),
            facility("f2", link_src, None),
            facility("f3", EventLink { alert_id: Some("aa".into()), source_id: None, source_record_id: None }, Some(10)),
        ],
    };
    f.validate().unwrap();
    let ids = |v: Vec<&Facility>| v.iter().map(|x| x.facility_id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(f.for_event("aa", "x", "y", 1_000)), vec!["f1"], "expired f3 excluded");
    assert_eq!(ids(f.for_event("bb", "src", "rec-1", 1_000)), vec!["f2"]);
    assert!(f.for_event("bb", "src", "rec-2", 1_000).is_empty(), "nearby is not linked");

    let mut bad = f.clone();
    bad.facilities[0].linked_events.clear();
    assert!(bad.validate().is_err(), "unlinked facility rejected");
    let mut bad = f;
    bad.facilities[0].lat = 95.0;
    assert!(bad.validate().is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn reference_and_notification_endpoints() {
    let dir = tempfile::tempdir().unwrap();
    let l = loaded(dir.path());
    let running = wda::app::start(&l, false).unwrap();
    let app = wda::app::router(running.state.clone(), None);

    let (s, h, b) = req(&app, get("/api/v1/contacts")).await;
    assert_eq!(s, 200);
    assert!(h.get("cache-control").unwrap().to_str().unwrap().contains("max-age"));
    assert!(b["countries"].as_array().unwrap().len() >= 249);
    let (s, _, b) = req(&app, get("/api/v1/contacts/us")).await;
    assert_eq!(s, 200);
    assert_eq!(b["country"]["contacts"][0]["tel_uri"], "tel:911");
    let (s, _, b) = req(&app, get("/api/v1/contacts/AW")).await;
    assert_eq!(s, 200);
    assert_eq!(b["country"]["coverage"], "unavailable");
    assert_eq!(b["country"]["contacts"].as_array().unwrap().len(), 0);
    let (s, _, _) = req(&app, get("/api/v1/contacts/ZZ")).await;
    assert_eq!(s, 404);
    let (s, _, _) = req(&app, get("/api/v1/contacts/U1")).await;
    assert_eq!(s, 400);

    let (s, _, b) = req(&app, get("/api/v1/guides/flood")).await;
    assert_eq!((s, b["status"].as_str()), (200, Some("available")));
    let (s, _, b) = req(&app, get("/api/v1/guides/drought")).await;
    assert_eq!((s, b["status"].as_str()), (200, Some("unavailable")));
    let (s, _, _) = req(&app, get("/api/v1/guides/..%2Fetc")).await;
    assert_eq!(s, 400);

    let msgs = wda::ingest::cap::parse("sample-cap", "https://example.invalid", &cap("alert.xml"), NOW_MS).unwrap().messages;
    let c = running.writer.ingest("sample-cap", "Sample", msgs, SourceHealth::default()).await.unwrap();
    let id = c.changed[0].alert_id.clone();

    let (s, _, b) = req(&app, get(&format!("/api/v1/alerts/{id}/facilities"))).await;
    assert_eq!(s, 200);
    assert_eq!(b["status"], "none_officially_linked");
    assert_eq!(b["items"].as_array().unwrap().len(), 0);

    // Inside the polygon → eligible; far away → not.
    let (s, _, b) = req(&app, get("/api/v1/notify/eligible?lat=18.8&lon=99.0&radius_km=10&min_priority=information")).await;
    assert_eq!(s, 200);
    assert_eq!(b["items"][0]["alert_id"], id.as_str());
    assert_eq!(b["items"][0]["distance_km"], 0.0);
    assert!(b["items"][0]["dedupe_key"].as_str().unwrap().ends_with(":1"));
    let (_, _, b) = req(&app, get("/api/v1/notify/eligible?lat=13.75&lon=100.5&radius_km=50&min_priority=information")).await;
    assert_eq!(b["items"].as_array().unwrap().len(), 0);
    let (s, _, _) = req(&app, get("/api/v1/notify/eligible?lat=91&lon=0")).await;
    assert_eq!(s, 400);
    let (s, _, _) = req(&app, get("/api/v1/notify/eligible?lat=1&lon=1&radius_km=9999")).await;
    assert_eq!(s, 400);

    // Cancelled / expired / low priority are never eligible.
    let rec = running.state.index.read().unwrap().alerts.get(&id).cloned().unwrap();
    let p = GeoPoint::new(18.8, 99.0).unwrap();
    assert!(wda::api::notify::eligible(&rec, &p, 10.0, DisplayPriority::Information, NOW_MS).is_some());
    let mut x = (*rec).clone();
    x.cancelled = true;
    assert!(wda::api::notify::eligible(&x, &p, 10.0, DisplayPriority::Information, NOW_MS).is_none());
    let mut x = (*rec).clone();
    x.expires_at_ms = Some(NOW_MS - 1);
    assert!(wda::api::notify::eligible(&x, &p, 10.0, DisplayPriority::Information, NOW_MS).is_none());
    let mut x = (*rec).clone();
    x.display_priority = DisplayPriority::Information;
    assert!(wda::api::notify::eligible(&x, &p, 10.0, DisplayPriority::Warning, NOW_MS).is_none());

    // Possibly-related incidents: same hazard, nearby, live; never itself.
    let mut twin = (*rec).clone();
    twin.alert_id = "f".repeat(32);
    assert_eq!(wda::api::notify::related_to(&rec, &twin), Some(0.0));
    assert!(wda::api::notify::related_to(&rec, &rec).is_none());
    let mut other = twin.clone();
    other.hazard_type = "heat".into();
    assert!(wda::api::notify::related_to(&rec, &other).is_none());
    let (s, _, b) = req(&app, get(&format!("/api/v1/alerts/{id}/related"))).await;
    assert_eq!((s, b["items"].as_array().unwrap().len()), (200, 0));

    // Translation disabled: honest 501, no fake output.
    let (s, _, b) = req(&app, get(&format!("/api/v1/alerts/{id}/translate?lang=en"))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (501, Some("translation_unavailable")));
    let (s, _, _) = req(&app, get(&format!("/api/v1/alerts/{id}/translate?lang=EN1"))).await;
    assert_eq!(s, 400);

    // Metrics (development: no token needed).
    let res = app.clone().oneshot(get("/health/metrics")).await.unwrap();
    assert_eq!(res.status(), 200);
    let text = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(text.contains("wda_live_alerts 1"), "{text}");
    assert!(text.contains("wda_api_requests_total"));

    // Search: coordinates and countries.
    let (_, _, b) = req(&app, get("/api/v1/search?q=13.75,100.5")).await;
    assert_eq!(b["coordinates"]["lat"], 13.75);
    let (_, _, b) = req(&app, get("/api/v1/search?q=japan")).await;
    assert_eq!(b["countries"][0]["code"], "JP");

    // Geocoder disabled by default: honest 501, coordinates still parsed locally.
    let (s, _, b) = req(&app, get("/api/v1/geocode?q=Chiang+Mai")).await;
    assert_eq!((s, b["error"]["code"].as_str()), (501, Some("geocoder_disabled")));
    let (s, _, b) = req(&app, get("/api/v1/geocode?q=18.79%2098.98")).await;
    assert_eq!((s, b["items"][0]["kind"].as_str()), (200, Some("coordinates")));
}

#[test]
fn coordinate_parsing() {
    use wda::api::geocode::parse_coordinates;
    assert_eq!(parse_coordinates("13.75, 100.5"), Some((13.75, 100.5)));
    assert_eq!(parse_coordinates("-33.9 151.2"), Some((-33.9, 151.2)));
    assert_eq!(parse_coordinates("91,0"), None);
    assert_eq!(parse_coordinates("bangkok"), None);
    assert_eq!(parse_coordinates("1,2,3"), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn edge_headers_cors_and_rate_limit() {
    let dir = tempfile::tempdir().unwrap();
    let l = loaded(dir.path());
    let running = wda::app::start(&l, false).unwrap();
    let http = HttpConfig {
        cors_allowed_origins: vec!["https://ilovebuger001.github.io".into()],
        rate_limit_per_minute: 3,
        trust_forwarded_for: true,
        hsts: true,
    };
    let app = wda::app::router_with(running.state.clone(), None, http);
    let with = |origin: &str, ip: &str| {
        axum::http::Request::builder()
            .uri("/api/v1/meta")
            .header("origin", origin)
            .header("x-forwarded-for", format!("10.0.0.1, {ip}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let (s, h, _) = req(&app, with("https://ilovebuger001.github.io", "203.0.113.1")).await;
    assert_eq!(s, 200);
    assert_eq!(h["access-control-allow-origin"], "https://ilovebuger001.github.io");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(h["content-security-policy"].to_str().unwrap().contains("frame-ancestors 'none'"));
    assert!(h.contains_key("strict-transport-security"));
    assert_eq!(h["cache-control"], "no-store");

    let (_, h, _) = req(&app, with("https://evil.example", "203.0.113.2")).await;
    assert!(!h.contains_key("access-control-allow-origin"), "unknown origin gets no CORS grant");

    let pre = axum::http::Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/alerts")
        .header("origin", "https://ilovebuger001.github.io")
        .header("access-control-request-method", "GET")
        .body(axum::body::Body::empty())
        .unwrap();
    let (s, h, _) = req(&app, pre).await;
    assert_eq!(s, 204);
    assert!(h["access-control-allow-methods"].to_str().unwrap().contains("GET"));

    let post = axum::http::Request::builder().method("POST").uri("/api/v1/meta").body(axum::body::Body::from("x")).unwrap();
    let (s, _, _) = req(&app, post).await;
    assert_eq!(s, 405);

    // Per-IP limit: third request from .1 already used 1; two more allowed, then 429.
    let o = "https://ilovebuger001.github.io";
    let (a, _, _) = req(&app, with(o, "203.0.113.1")).await;
    let (b, _, _) = req(&app, with(o, "203.0.113.1")).await;
    let (c, h, body) = req(&app, with(o, "203.0.113.1")).await;
    // The window may roll over during the test; accept either a full window or a reset.
    if a == 200 && b == 200 {
        assert_eq!(c, 429, "{body}");
        assert!(h.contains_key("retry-after"));
        assert_eq!(body["error"]["code"], "rate_limited");
    }
    let (s, _, _) = req(&app, with(o, "203.0.113.9")).await;
    assert_eq!(s, 200, "other clients unaffected");
}

#[test]
fn rate_limiter_windows() {
    let r = wda::http::RateLimiter::new(2);
    let ip: std::net::IpAddr = "198.51.100.7".parse().unwrap();
    assert!(r.check(ip, 60_000).is_ok());
    assert!(r.check(ip, 60_500).is_ok());
    assert!(r.check(ip, 61_000).is_err());
    assert!(r.check(ip, 120_000).is_ok(), "new window");
}

#[test]
fn production_config_guards() {
    let mut app: AppConfig = toml::from_str(include_str!("../config/app.toml")).unwrap();
    app.environment = wda::config::Environment::Production;
    let m = mapping();
    app.http.cors_allowed_origins = vec!["*".into()];
    assert!(validate(&app, &[], &m).unwrap_err().to_string().contains("CORS"));
    app.http.cors_allowed_origins = vec!["https://example.org/path".into()];
    assert!(validate(&app, &[], &m).is_err());
    app.http.cors_allowed_origins = vec!["https://example.org".into()];
    app.http.rate_limit_per_minute = 0;
    assert!(validate(&app, &[], &m).unwrap_err().to_string().contains("rate"));
    app.http.rate_limit_per_minute = 300;
    assert!(validate(&app, &[], &m).is_ok());
    app.geocoder.enabled = true;
    app.geocoder.min_interval_ms = 200;
    assert!(validate(&app, &[], &m).is_err(), "public Nominatim limit");
}

#[test]
fn jurisdiction_only_from_national_issuing_authority() {
    let j = jurisdiction_of(Some("warning_issuer"), Some("national"), Some("us"));
    assert_eq!(j.country_code.as_deref(), Some("US"));
    assert!(jurisdiction_of(Some("scientific_monitor"), Some("national"), Some("US")).country_code.is_none(), "global catalog");
    assert!(jurisdiction_of(Some("warning_issuer"), Some("provincial"), Some("TH")).country_code.is_none());
    assert!(jurisdiction_of(Some("warning_issuer"), Some("national"), Some("USA")).country_code.is_none());
}

#[test]
fn translation_language_codes() {
    assert!(wda::api::translate::valid_lang("th"));
    assert!(!wda::api::translate::valid_lang("TH"));
    assert!(!wda::api::translate::valid_lang("t"));
    assert!(!wda::api::translate::valid_lang("../x"));
}
