//! `/api/v1/search?q=&limit=` — areas and disasters only (spec §64.4, §66.1).
//! Searches the configured gazetteer and the in-memory alert index. Never a
//! general web search; never invents an alert or declares an area safe.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize)]
pub struct Place {
    pub name_th: String,
    pub name_en: String,
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Gazetteer {
    #[serde(default)]
    pub places: Vec<Place>,
}

pub const MAX_Q: usize = 64;
pub const MAX_LIMIT: usize = 10;

pub async fn search(State(st): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Result<Json<Value>, ApiError> {
    let raw = q.get("q").map(|s| s.trim()).unwrap_or("");
    if raw.is_empty() || raw.chars().count() > MAX_Q {
        return Err(ApiError::bad_request("invalid_query", "q must be 1-64 characters."));
    }
    let limit = match q.get("limit") {
        Some(l) => l.parse::<usize>().map_err(|_| ApiError::bad_request("invalid_limit", "limit must be an integer."))?.clamp(1, MAX_LIMIT),
        None => 5,
    };
    let needle = raw.to_lowercase();
    let areas: Vec<Value> = st
        .gazetteer
        .places
        .iter()
        .filter(|p| p.name_th.to_lowercase().contains(&needle) || p.name_en.to_lowercase().contains(&needle))
        .take(limit)
        .map(|p| json!({ "name_th": p.name_th, "name_en": p.name_en, "lat": p.lat, "lon": p.lon, "zoom": p.zoom }))
        .collect();
    let mut disasters: Vec<(u8, i64, Value)> = {
        let idx = st.index.read().map_err(|_| ApiError::internal())?;
        idx.alerts
            .values()
            .filter(|a| a.status.is_live())
            .filter(|a| {
                let mut hay: Vec<String> = vec![a.hazard_type.clone(), a.source_agency.to_lowercase()];
                for v in &a.language_variants {
                    hay.extend(v.headline.iter().cloned());
                    hay.extend(v.event.iter().cloned());
                    hay.extend(v.area_desc.iter().cloned());
                }
                hay.iter().any(|h| h.to_lowercase().contains(&needle))
            })
            .map(|a| {
                let v = a.language_variants.first();
                let point = a.geometry_parts.iter().find_map(|g| g.representative_point());
                (
                    a.display_priority.rank(),
                    -a.source_published_at_ms,
                    json!({
                        "alert_id": a.alert_id,
                        "headline": v.and_then(|v| v.headline.clone().or_else(|| v.event.clone())),
                        "hazard_type": a.hazard_type,
                        "display_priority": a.display_priority,
                        "source_agency": a.source_agency,
                        "point": point.map(|p| json!({ "lat": p.lat_deg, "lon": p.lon_deg })),
                    }),
                )
            })
            .collect()
    };
    disasters.sort_by_key(|d| (d.0, d.1));
    let disasters: Vec<Value> = disasters.into_iter().take(limit).map(|d| d.2).collect();
    let coordinates = super::geocode::parse_coordinates(raw).map(|(lat, lon)| json!({ "lat": lat, "lon": lon }));
    let countries: Vec<Value> = st
        .reference
        .contacts
        .countries
        .iter()
        .filter(|c| c.name.to_lowercase().contains(&needle) || c.code.eq_ignore_ascii_case(raw))
        .take(limit)
        .map(|c| json!({ "code": c.code, "name": c.name, "coverage": c.coverage }))
        .collect();
    Ok(Json(json!({ "areas": areas, "countries": countries, "disasters": disasters, "coordinates": coordinates })))
}
