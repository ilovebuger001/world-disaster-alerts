//! `/api/v1/meta` and `/api/v1/sources` (spec §9.1, §64.7 two status concepts, §66.4).

use crate::config::Environment;
use crate::error::ApiError;
use crate::ingest::source_registry::FreshnessState;
use crate::state::AppState;
use crate::storage::schema::SCHEMA_VERSION;
use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Overall application/feed freshness. Never "live" when only some sources update.
pub fn overall_freshness(st: &AppState) -> (String, BTreeMap<String, usize>) {
    let idx = match st.index.read() {
        Ok(i) => i,
        Err(_) => return ("unknown".into(), BTreeMap::new()),
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut n = 0;
    for s in st.sources.iter().filter(|s| s.enabled) {
        n += 1;
        let f = idx.sources.get(&s.source_id).map(|h| h.freshness()).unwrap_or(FreshnessState::Unknown);
        let k = serde_json::to_value(f).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        *counts.entry(k).or_insert(0) += 1;
    }
    let fresh = counts.get("fresh").copied().unwrap_or(0);
    let delayed = counts.get("delayed").copied().unwrap_or(0);
    let overall = if n == 0 {
        "no_sources"
    } else if fresh == n {
        "live"
    } else if fresh > 0 {
        "partial"
    } else if delayed > 0 {
        "delayed"
    } else {
        "stale"
    };
    (overall.into(), counts)
}

pub async fn meta(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let seq = st.index.read().map_err(|_| ApiError::internal())?.seq;
    let (overall, counts) = overall_freshness(&st);
    Ok(Json(json!({
        "server_time_ms": crate::domain::now_ms(),
        "api_version": "v1",
        "schema_version": SCHEMA_VERSION,
        "event_sequence": seq,
        "environment": match st.api.environment { Environment::Development => "development", Environment::Production => "production" },
        "freshness": { "overall": overall, "sources_by_state": counts },
        "coverage_note": "Coverage is limited to the configured sources. Absence of an alert is not evidence of safety."
    })))
}

pub async fn sources(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let idx = st.index.read().map_err(|_| ApiError::internal())?;
    let items: Vec<Value> = st
        .sources
        .iter()
        .map(|s| {
            let h = idx.sources.get(&s.source_id).cloned().unwrap_or_default();
            json!({
                "source_id": s.source_id,
                "agency_name": s.agency_name,
                "country_or_region": s.country_or_region,
                "official_homepage": s.official_homepage,
                "format": s.format,
                "hazard_types": s.hazard_types,
                "geographic_coverage": s.geographic_coverage,
                "enabled": s.enabled,
                "authority_role": s.authority_role,
                "publisher_class": s.publisher_class,
                "coverage_state": s.coverage_state,
                "onboarding_state": s.onboarding_state,
                "redistribution_status": s.redistribution_status,
                "public_redistribution": s.public_redistribution(),
                "terms_url": s.terms_url,
                "attribution": s.attribution_template,
                "expected_freshness_seconds": s.expected_freshness_seconds,
                "health": {
                    "freshness_state": h.freshness(),
                    "last_attempt_at_ms": h.last_attempt_at_ms,
                    "last_http_success_at_ms": h.last_http_success_at_ms,
                    "last_content_change_at_ms": h.last_content_change_at_ms,
                    "last_valid_record_at_ms": h.last_valid_record_at_ms,
                    "last_error_code": h.last_error_code,
                    "last_error_summary": h.last_error_summary,
                    "consecutive_failures": h.consecutive_failures,
                    "rejected_records_last_fetch": h.rejected_records_last_fetch
                }
            })
        })
        .collect();
    Ok(Json(json!({ "generated_at_ms": crate::domain::now_ms(), "sources": items })))
}
