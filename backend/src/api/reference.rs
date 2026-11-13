//! Emergency contacts, preparedness guides and event-linked facilities.

use crate::error::ApiError;
use crate::reference::{tel_uri, Country};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

const CACHE: &str = "public, max-age=3600";

pub fn country_json(c: &Country) -> Value {
    let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
    if let Some(list) = v.get_mut("contacts").and_then(Value::as_array_mut) {
        for (item, k) in list.iter_mut().zip(&c.contacts) {
            item["tel_uri"] = tel_uri(k).map(Value::String).unwrap_or(Value::Null);
        }
    }
    v
}

/// Whole directory (every ISO 3166-1 entry), for the offline cache.
pub async fn contacts(State(st): State<AppState>) -> impl IntoResponse {
    let f = &st.reference.contacts;
    let body = json!({
        "schema_version": f.schema_version,
        "dataset_version": f.dataset_version,
        "country_list_source": f.country_list_source,
        "verification_levels": f.verification_levels,
        "coverage": st.reference.coverage_counts(),
        "countries": f.countries.iter().map(country_json).collect::<Vec<_>>(),
    });
    ([(header::CACHE_CONTROL, CACHE)], Json(body))
}

pub async fn contact(State(st): State<AppState>, Path(code): Path<String>) -> Result<impl IntoResponse, ApiError> {
    if code.len() != 2 || !code.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Err(ApiError::bad_request("invalid_country", "Country must be an ISO 3166-1 alpha-2 code."));
    }
    let c =
        st.reference.contacts.country(&code).ok_or_else(|| ApiError::not_found("This country or territory is not in the directory."))?;
    Ok((
        [(header::CACHE_CONTROL, CACHE)],
        Json(json!({ "dataset_version": st.reference.contacts.dataset_version, "country": country_json(c) })),
    ))
}

pub async fn guides(State(st): State<AppState>) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, CACHE)], Json(serde_json::to_value(&st.reference.guides).unwrap_or(Value::Null)))
}

pub async fn guide(
    State(st): State<AppState>,
    Path(hazard): Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    if hazard.len() > 40 || !hazard.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        return Err(ApiError::bad_request("invalid_hazard", "Hazard must be a lowercase identifier."));
    }
    let g = &st.reference.guides;
    let short =
        |k: &str| q.get(k).map(String::as_str).filter(|v| v.len() <= 8 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    let body = match g.guide_for(&hazard, short("lang"), short("country")) {
        Some(x) => json!({ "status": "available", "notice": g.notice, "guide": x, "alternatives": g.guides_for(&hazard).len() - 1 }),
        None => {
            let reason = g
                .unavailable
                .iter()
                .find(|u| u.hazard == hazard)
                .map(|u| u.reason.clone())
                .unwrap_or_else(|| "No reviewed official guidance is available for this hazard yet.".into());
            json!({ "status": "unavailable", "hazard": hazard, "reason": reason })
        }
    };
    Ok(([(header::CACHE_CONTROL, CACHE)], Json(body)))
}

/// Facilities an official source has linked to this specific event.
pub async fn facilities(State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    if id.len() > 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request("invalid_alert_id", "Alert id is invalid."));
    }
    let rec = {
        let idx = st.index.read().map_err(|_| ApiError::internal())?;
        idx.alerts.get(&id).cloned()
    };
    let rec = rec.ok_or_else(|| ApiError::not_found("This alert was not found or is no longer available."))?;
    let now = crate::domain::now_ms();
    let items: Vec<_> = if rec.status.is_live() {
        st.reference.facilities.for_event(&rec.alert_id, &rec.source_id, &rec.source_record_id, now)
    } else {
        Vec::new()
    };
    let status = if !rec.status.is_live() {
        "event_not_active"
    } else if items.is_empty() {
        "none_officially_linked"
    } else {
        "available"
    };
    Ok(Json(json!({
        "alert_id": rec.alert_id,
        "status": status,
        "dataset_version": st.reference.facilities.dataset_version,
        "notice": st.reference.facilities.notice,
        "items": items,
    })))
}
