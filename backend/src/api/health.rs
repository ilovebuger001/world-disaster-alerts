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
