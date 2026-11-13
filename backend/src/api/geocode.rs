//! Address / place search through a Nominatim-compatible geocoder.
//!
//! Called only by this server, only on an explicit search (no autocomplete),
//! serialized to at most one upstream request per `min_interval_ms`, with an
//! identifying User-Agent and a response cache, per the OSMF Nominatim usage
//! policy. Disabled unless configured. Coordinates typed by the user are parsed
//! locally and never sent upstream.

use crate::config::GeocoderConfig;
use crate::error::ApiError;
use crate::ingest::http_client::SafeResolver;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ATTRIBUTION: &str = "Search data © OpenStreetMap contributors (ODbL), via Nominatim";

pub struct Geocoder {
    client: reqwest::Client,
    base_url: String,
    min_interval: Duration,
    cache_entries: usize,
    last: tokio::sync::Mutex<Option<Instant>>,
    cache: Mutex<HashMap<String, Value>>,
}

#[derive(Deserialize)]
struct NomItem {
    display_name: String,
    lat: String,
    lon: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    boundingbox: Option<Vec<String>>,
}

impl Geocoder {
    pub fn new(cfg: &GeocoderConfig, user_agent: &str) -> reqwest::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .https_only(true)
            .dns_resolver(Arc::new(SafeResolver { allow_private: false }))
            .build()?;
        Ok(Self {
            client,
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            min_interval: Duration::from_millis(cfg.min_interval_ms),
            cache_entries: cfg.cache_entries,
            last: tokio::sync::Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub async fn search(&self, q: &str, lang: &str) -> Result<Value, ApiError> {
        let key = format!("{lang}\u{1f}{}", q.to_lowercase());
        if let Some(v) = self.cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Ok(v);
        }
        // One upstream request at a time, spaced by min_interval. Give up rather
        // than queue indefinitely when many users search at once.
        let mut last = tokio::time::timeout(Duration::from_secs(4), self.last.lock()).await.map_err(|_| {
            ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "geocoder_busy", "Address search is busy. Try again in a few seconds.")
        })?;
        if let Some(t) = *last {
            let wait = self.min_interval.saturating_sub(t.elapsed());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        let res = self
            .client
            .get(format!("{}/search", self.base_url))
            .query(&[("q", q), ("format", "jsonv2"), ("limit", "5"), ("accept-language", lang)])
            .send()
            .await;
        *last = Some(Instant::now());
        drop(last);
        let unavailable = || ApiError::new(StatusCode::BAD_GATEWAY, "geocoder_unavailable", "Address search is temporarily unavailable.");
        let res = res.map_err(|e| {
            tracing::warn!(error = %e, "geocoder request failed");
            unavailable()
        })?;
        if !res.status().is_success() {
            tracing::warn!(status = res.status().as_u16(), "geocoder returned an error");
            return Err(unavailable());
        }
        let bytes = res.bytes().await.map_err(|_| unavailable())?;
        if bytes.len() > 256 * 1024 {
            return Err(unavailable());
        }
        let items: Vec<NomItem> = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        let out: Vec<Value> = items
            .into_iter()
            .filter_map(|i| {
                let lat = i.lat.parse::<f64>().ok().filter(|v| (-90.0..=90.0).contains(v))?;
                let lon = i.lon.parse::<f64>().ok().filter(|v| (-180.0..=180.0).contains(v))?;
                let bbox = i.boundingbox.and_then(|b| {
                    let n: Vec<f64> = b.iter().filter_map(|s| s.parse().ok()).collect();
                    (n.len() == 4).then(|| json!({ "south": n[0], "north": n[1], "west": n[2], "east": n[3] }))
                });
                Some(json!({ "name": i.display_name, "lat": lat, "lon": lon, "category": i.category, "kind": i.kind, "bbox": bbox }))
            })
            .collect();
        let v = json!({ "items": out, "attribution": ATTRIBUTION });
        if let Ok(mut c) = self.cache.lock() {
            if c.len() >= self.cache_entries {
                c.clear();
            }
            c.insert(key, v.clone());
        }
        Ok(v)
    }
}

/// Parses "lat, lon" (decimal degrees, comma or whitespace separated).
pub fn parse_coordinates(s: &str) -> Option<(f64, f64)> {
    let parts: Vec<&str> = s.split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    if parts.len() != 2 {
        return None;
    }
    let lat: f64 = parts[0].parse().ok()?;
    let lon: f64 = parts[1].parse().ok()?;
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && lat.is_finite() && lon.is_finite()).then_some((lat, lon))
}

pub async fn geocode(State(st): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Result<Json<Value>, ApiError> {
    let raw = q.get("q").map(|s| s.trim()).unwrap_or("");
    if raw.chars().count() < 3 || raw.chars().count() > 120 {
        return Err(ApiError::bad_request("invalid_query", "q must be 3-120 characters."));
    }
    if let Some((lat, lon)) = parse_coordinates(raw) {
        return Ok(Json(
            json!({ "items": [{ "name": format!("{lat:.5}, {lon:.5}"), "lat": lat, "lon": lon, "kind": "coordinates" }], "attribution": null }),
        ));
    }
    let lang = q
        .get("lang")
        .map(|s| s.as_str())
        .filter(|l| l.len() <= 12 && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .unwrap_or("en");
    match &st.geocoder {
        Some(g) => g.search(raw, lang).await.map(Json),
        None => Err(ApiError::new(StatusCode::NOT_IMPLEMENTED, "geocoder_disabled", "Address search is not enabled on this server.")),
    }
}
