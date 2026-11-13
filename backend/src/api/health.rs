use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use std::sync::atomic::Ordering;

/// Process liveness only.
pub async fn live() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "live" }))
}

/// Readiness: database opened, index built, core workers initialized.
pub async fn ready(State(st): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    if st.ready.load(Ordering::Acquire) {
        let live = st.index.read().map_err(|_| ApiError::internal())?.live_count();
        Ok(Json(serde_json::json!({ "status": "ready", "indexed_live_alerts": live })))
    } else {
        Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "not_ready", "Service is starting."))
    }
}

/// Prometheus text metrics. In production this requires `Authorization: Bearer
/// <WDA_METRICS_TOKEN>`; without a configured token it is not served there.
pub async fn metrics(State(st): State<AppState>, headers: axum::http::HeaderMap) -> Result<axum::response::Response, ApiError> {
    use crate::http::Metrics as M;
    use axum::response::IntoResponse;
    let token = std::env::var("WDA_METRICS_TOKEN").ok().filter(|t| t.len() >= 16);
    let given = headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    let allowed = match &token {
        Some(t) => given.is_some_and(|g| constant_eq(g.as_bytes(), t.as_bytes())),
        None => st.api.environment != crate::config::Environment::Production,
    };
    if !allowed {
        return Err(ApiError::not_found("Not found."));
    }
    let mut out = String::new();
    let mut line = |name: &str, help: &str, kind: &str, rows: Vec<(String, f64)>| {
        out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
        for (labels, v) in rows {
            out.push_str(&format!("{name}{labels} {v}\n"));
        }
    };
    let m = &st.metrics;
    line("wda_api_requests_total", "API requests received", "counter", vec![(String::new(), M::get(&m.api_requests) as f64)]);
    line(
        "wda_api_errors_total",
        "API error responses by class",
        "counter",
        vec![("{class=\"4xx\"}".into(), M::get(&m.api_errors_4xx) as f64), ("{class=\"5xx\"}".into(), M::get(&m.api_errors_5xx) as f64)],
    );
    line(
        "wda_rate_limited_total",
        "Requests rejected by the rate limiter",
        "counter",
        vec![(String::new(), M::get(&m.rate_limited) as f64)],
    );
    let (live, seq, srcs) = {
        let idx = st.index.read().map_err(|_| ApiError::internal())?;
        let srcs: Vec<(String, u32, Option<i64>, String)> = st
            .sources
            .iter()
            .filter(|s| s.enabled)
            .map(|s| {
                let h = idx.sources.get(&s.source_id);
                let fresh =
                    h.map(|h| serde_json::to_value(h.freshness()).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default());
                (
                    s.source_id.clone(),
                    h.map_or(0, |h| h.consecutive_failures),
                    h.and_then(|h| h.last_http_success_at_ms),
                    fresh.unwrap_or_else(|| "unknown".into()),
                )
            })
            .collect();
        (idx.live_count(), idx.seq, srcs)
    };
    line("wda_live_alerts", "Live alerts in the index", "gauge", vec![(String::new(), live as f64)]);
    line("wda_event_sequence", "Last committed event sequence", "gauge", vec![(String::new(), seq as f64)]);
    let sse: usize = st.sse.per_ip.lock().map(|m| m.values().sum()).unwrap_or(0);
    line("wda_sse_clients", "Open SSE connections", "gauge", vec![(String::new(), sse as f64)]);
    line(
        "wda_source_live",
        "1 when the source freshness is live",
        "gauge",
        srcs.iter().map(|(id, _, _, f)| (format!("{{source=\"{id}\"}}"), if f == "live" { 1.0 } else { 0.0 })).collect(),
    );
    line(
        "wda_source_consecutive_failures",
        "Consecutive fetch failures per source",
        "gauge",
        srcs.iter().map(|(id, n, _, _)| (format!("{{source=\"{id}\"}}"), *n as f64)).collect(),
    );
    line(
        "wda_source_last_success_seconds",
        "Unix time of the last successful fetch",
        "gauge",
        srcs.iter().map(|(id, _, t, _)| (format!("{{source=\"{id}\"}}"), t.map_or(0.0, |t| t as f64 / 1000.0))).collect(),
    );
    Ok(([(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")], out).into_response())
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
