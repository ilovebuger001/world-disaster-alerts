//! Named JSON events (spec §10.1, §56 item 4). Compact references; clients
//! fetch full detail from the REST API.

use serde::{Deserialize, Serialize};

pub const ALERT_UPSERT: &str = "alert.upsert";
pub const ALERT_REMOVE: &str = "alert.remove";
pub const SOURCE_STATUS: &str = "source.status";
pub const STREAM_RESET: &str = "stream.reset";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiEvent {
    /// Monotonic event sequence (also the SSE `id`).
    pub event_sequence: u64,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hazard_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
    pub committed_at_ms: i64,
}
