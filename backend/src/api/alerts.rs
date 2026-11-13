//! `/api/v1/alerts` viewport + list and `/api/v1/alerts/{id}` details (spec §9.2, §9.3, §56).

use super::sources::overall_freshness;
use crate::domain::alert::{AlertRecord, AlertStatus, DisplayPriority};
use crate::domain::geometry::GeoPoint;
use crate::error::ApiError;
use crate::geo::validate::Viewport;
use crate::geo::viewport::{decode_cursor, query, Filters, DEFAULT_PAGE};
use crate::ingest::source_registry::FreshnessState;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::Json;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

fn num(q: &HashMap<String, String>, k: &str) -> Result<f64, ApiError> {
    q.get(k)
        .ok_or_else(|| ApiError::bad_request("invalid_viewport", format!("Missing parameter '{k}'.")))?
        .parse::<f64>()
        .map_err(|_| ApiError::bad_request("invalid_viewport", format!("Parameter '{k}' must be a number.")))
}

fn list(q: &HashMap<String, String>, k: &str) -> Option<Vec<String>> {
    q.get(k).map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
}

pub fn parse_filters(q: &HashMap<String, String>) -> Result<Filters, ApiError> {
    let mut f = Filters::default();
    if let Some(t) = list(q, "types") {
        if t.len() > 32 || t.iter().any(|s| s.len() > 40) {
            return Err(ApiError::bad_request("invalid_filter", "Too many or too long hazard types."));
        }
        f.hazard_types = Some(t.into_iter().collect());
    }
    if let Some(p) = list(q, "priority") {
        let mut set = HashSet::new();
        for v in p {
            set.insert(
                DisplayPriority::parse(&v).ok_or_else(|| ApiError::bad_request("invalid_filter", format!("Unknown priority '{v}'.")))?,
            );
        }
        f.priorities = Some(set);
    }
    if let Some(s) = list(q, "status") {
        let mut set = HashSet::new();
        for v in s {
            let st: AlertStatus = serde_json::from_value(Value::String(v.clone()))
                .map_err(|_| ApiError::bad_request("invalid_filter", format!("Unknown status '{v}'.")))?;
            set.insert(st);
        }
        f.statuses = Some(set);
    }
    if let Some(s) = q.get("since_ms") {
        f.since_ms = Some(s.parse().map_err(|_| ApiError::bad_request("invalid_filter", "since_ms must be an integer."))?);
    }
    Ok(f)
}

fn source_freshness(st: &AppState, source_id: &str) -> FreshnessState {
    st.index.read().ok().and_then(|i| i.sources.get(source_id).map(|h| h.freshness())).unwrap_or(FreshnessState::Unknown)
}

pub fn summary(a: &AlertRecord, point: Option<GeoPoint>, freshness: FreshnessState) -> Value {
    let v = a.language_variants.first();
    json!({
        "alert_id": a.alert_id,
        "revision": a.revision,
        "status": a.status,
        "hazard_type": a.hazard_type,
        "display_priority": a.display_priority,
        "headline": v.and_then(|v| v.headline.clone().or_else(|| v.event.clone())),
        "language": v.map(|v| v.language.clone()),
        "area_desc": v.map(|v| v.area_desc.clone()).unwrap_or_default(),
        "source_id": a.source_id,
        "source_agency": a.source_agency,
        "source_url": a.source_url,
        "source_published_at_ms": a.source_published_at_ms,
        "source_updated_at_ms": a.source_updated_at_ms,
        "fetched_at_ms": a.fetched_at_ms,
        "expires_at_ms": a.expires_at_ms,
        "source_freshness": freshness,
        "native": a.native,
        "point": point.map(|p| json!({ "lat": p.lat_deg, "lon": p.lon_deg })),
    })
}

pub async fn viewport(State(st): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Result<Json<Value>, ApiError> {
    let v = Viewport {
        west: num(&q, "west")?,
        south: num(&q, "south")?,
        east: num(&q, "east")?,
        north: num(&q, "north")?,
        zoom: num(&q, "zoom")?,
    }
    .validate()
    .map_err(|e| ApiError::bad_request("invalid_viewport", e.to_string()))?;
    let filters = parse_filters(&q)?;
    let page = match q.get("limit") {
        Some(l) => l.parse::<usize>().map_err(|_| ApiError::bad_request("invalid_limit", "limit must be a positive integer."))?,
        None => DEFAULT_PAGE,
    };
    let cursor = match q.get("cursor") {
        Some(c) => Some(decode_cursor(c).ok_or_else(|| ApiError::bad_request("invalid_cursor", "Cursor is invalid."))?),
        None => None,
    };
    let (overall, _) = overall_freshness(&st);
    let (res, fresh_map) = {
        let idx = st.index.read().map_err(|_| ApiError::internal())?;
        let r = query(&idx, &v, &filters, page, cursor);
        let fm: HashMap<String, FreshnessState> = idx.sources.iter().map(|(k, h)| (k.clone(), h.freshness())).collect();
        (r, fm)
    };
    let items: Vec<Value> =
        res.items.iter().map(|(a, p)| summary(a, *p, fresh_map.get(&a.source_id).copied().unwrap_or(FreshnessState::Unknown))).collect();
    Ok(Json(json!({
        "generated_at_ms": crate::domain::now_ms(),
        "event_sequence": res.event_sequence,
        "freshness": overall,
        "clustered": res.clustered,
        "candidates_capped": res.candidates_capped,
        "total_matching": res.total_matching,
        "features": res.features,
        "items": items,
        "next_cursor": res.next_cursor,
    })))
}

pub async fn detail(State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    if id.len() > 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request("invalid_alert_id", "Alert id is invalid."));
    }
    let (rec, seq) = {
        let idx = st.index.read().map_err(|_| ApiError::internal())?;
        (idx.alerts.get(&id).cloned(), idx.seq)
    };
    let rec = rec.ok_or_else(|| ApiError::not_found("This alert was not found or is no longer available."))?;
    let src = st.sources.iter().find(|s| s.source_id == rec.source_id);
    let j = crate::reference::jurisdiction_of(
        src.map(|s| s.authority_role.as_str()),
        src.and_then(|s| s.jurisdiction_level.as_deref()),
        src.and_then(|s| s.jurisdiction_id.as_deref()),
    );
    let contacts = j.country_code.as_deref().and_then(|c| st.reference.contacts.country(c)).map(super::reference::country_json);
    Ok(Json(json!({
        "jurisdiction": j,
        "country_contacts": contacts,
        "event_sequence": seq,
        "alert": &*rec,
        "source_freshness": source_freshness(&st, &rec.source_id),
        "source": src.map(|s| json!({
            "source_id": s.source_id,
            "agency_name": s.agency_name,
            "official_homepage": s.official_homepage,
            "terms_url": s.terms_url,
            "attribution": s.attribution_template,
            "authority_role": s.authority_role,
            "public_redistribution": s.public_redistribution(),
        })),
        // Machine translation is on request only (GET .../translate), never replaces the original.
        "translation_available": st.translator.is_some(),
        "signature_verified": false,
    })))
}
