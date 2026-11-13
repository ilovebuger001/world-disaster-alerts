//! Notification eligibility (server side). The browser asks which live alerts
//! qualify for a location; it still applies the user's permission, its own
//! de-duplication store and a rate limit before showing anything.

use crate::domain::alert::{AlertRecord, AlertStatus, DisplayPriority};
use crate::domain::geometry::{haversine_km, point_in_polygon, GeoPoint, GeometryPart};
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const MAX_RADIUS_KM: f64 = 500.0;
pub const MAX_ITEMS: usize = 20;

/// Distance from `p` to the alert's area: 0 when inside a polygon or circle,
/// otherwise the smallest distance to a representative point / circle edge.
/// `None` when the alert has no usable geometry (never guessed).
pub fn distance_km(a: &AlertRecord, p: &GeoPoint) -> Option<f64> {
    let mut best: Option<f64> = None;
    for g in &a.geometry_parts {
        let d = match g {
            GeometryPart::Point { point } => haversine_km(point, p),
            GeometryPart::Circle { center, radius_km } => (haversine_km(center, p) - radius_km).max(0.0),
            GeometryPart::Polygon { exterior, holes } => {
                if point_in_polygon(p, exterior, holes) {
                    0.0
                } else {
                    exterior.iter().map(|v| haversine_km(v, p)).fold(f64::INFINITY, f64::min)
                }
            }
            _ => continue,
        };
        best = Some(best.map_or(d, |b: f64| b.min(d)));
    }
    best
}

/// An alert may trigger a notification only when it is active or upcoming,
/// not cancelled, not expired, at or above the requested priority and within range.
pub fn eligible(a: &AlertRecord, p: &GeoPoint, radius_km: f64, min: DisplayPriority, now_ms: i64) -> Option<f64> {
    if !matches!(a.status, AlertStatus::Active | AlertStatus::Upcoming) || a.cancelled {
        return None;
    }
    if a.expires_at_ms.is_some_and(|e| e <= now_ms) {
        return None;
    }
    if a.display_priority.rank() > min.rank() {
        return None;
    }
    distance_km(a, p).filter(|d| *d <= radius_km)
}

pub async fn eligible_alerts(State(st): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Result<Json<Value>, ApiError> {
    let f = |k: &str| q.get(k).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite());
    let (lat, lon) = match (f("lat"), f("lon")) {
        (Some(a), Some(b)) => (a, b),
        _ => return Err(ApiError::bad_request("invalid_location", "lat and lon are required numbers.")),
    };
    let p = GeoPoint::new(lat, lon).map_err(|_| ApiError::bad_request("invalid_location", "Coordinates are out of range."))?;
    let radius = f("radius_km").unwrap_or(50.0);
    if !(0.0..=MAX_RADIUS_KM).contains(&radius) {
        return Err(ApiError::bad_request("invalid_radius", "radius_km must be between 0 and 500."));
    }
    let min = match q.get("min_priority") {
        Some(s) => DisplayPriority::parse(s).ok_or_else(|| ApiError::bad_request("invalid_priority", "Unknown priority."))?,
        None => DisplayPriority::Warning,
    };
    let now = crate::domain::now_ms();
    let (mut items, seq) = {
        let idx = st.index.read().map_err(|_| ApiError::internal())?;
        let items: Vec<(f64, std::sync::Arc<AlertRecord>)> =
            idx.alerts.values().filter_map(|a| eligible(a, &p, radius, min, now).map(|d| (d, a.clone()))).collect();
        (items, idx.seq)
    };
    items.sort_by(|x, y| x.1.display_priority.rank().cmp(&y.1.display_priority.rank()).then(x.0.total_cmp(&y.0)));
    let out: Vec<Value> = items
        .iter()
        .take(MAX_ITEMS)
        .map(|(d, a)| {
            let v = a.language_variants.first();
            json!({
                "alert_id": a.alert_id,
                "revision": a.revision,
                "dedupe_key": format!("{}:{}", a.alert_id, a.revision),
                "headline": v.and_then(|v| v.headline.clone().or_else(|| v.event.clone())),
                "hazard_type": a.hazard_type,
                "display_priority": a.display_priority,
                "status": a.status,
                "source_agency": a.source_agency,
                "distance_km": (d * 10.0).round() / 10.0,
                "expires_at_ms": a.expires_at_ms,
            })
        })
        .collect();
    Ok(Json(json!({ "event_sequence": seq, "radius_km": radius, "min_priority": min, "items": out })))
}
