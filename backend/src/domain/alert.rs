//! Normalized public alert record (spec §5.1 entity 2, fields §5.2).

use super::geometry::GeometryPart;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type AlertId = String;

/// Canonical lifecycle (spec §5.2 `status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertStatus {
    Active,
    Upcoming,
    Expired,
    Cancelled,
    Withdrawn,
    Stale,
}

impl AlertStatus {
    /// Records that belong on the live map/index.
    pub fn is_live(self) -> bool {
        matches!(self, AlertStatus::Active | AlertStatus::Upcoming | AlertStatus::Stale)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            AlertStatus::Active => "active",
            AlertStatus::Upcoming => "upcoming",
            AlertStatus::Expired => "expired",
            AlertStatus::Cancelled => "cancelled",
            AlertStatus::Withdrawn => "withdrawn",
            AlertStatus::Stale => "stale",
        }
    }
}

/// Deterministic display grouping (spec §7). Never a CAP replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayPriority {
    Emergency,
    Warning,
    Watch,
    Advisory,
    Information,
    Unknown,
}

impl DisplayPriority {
    /// Lower rank = higher priority.
    pub fn rank(self) -> u8 {
        self as u8
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "emergency" => Self::Emergency,
            "warning" => Self::Warning,
            "watch" => Self::Watch,
            "advisory" => Self::Advisory,
            "information" => Self::Information,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }
}

/// One language block of original wording (spec §5.2 `language_variants[]`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LanguageVariant {
    pub language: String,
    pub event: Option<String>,
    pub headline: Option<String>,
    pub description: Option<String>,
    pub instruction: Option<String>,
    pub sender_name: Option<String>,
    pub web: Option<String>,
    pub area_desc: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlertRecord {
    pub alert_id: AlertId,
    pub source_id: String,
    pub source_record_id: String,
    pub source_agency: String,
    pub source_url: String,
    pub source_published_at_ms: i64,
    pub source_updated_at_ms: Option<i64>,
    pub fetched_at_ms: i64,
    pub first_seen_at_ms: i64,
    pub last_seen_at_ms: i64,
    /// Monotonically increasing per alert.
    pub revision: u64,
    pub status: AlertStatus,
    /// Original CAP/source status (verbatim).
    pub source_status: String,
    /// Original CAP msgType or provider equivalent (verbatim).
    pub message_type: String,
    pub scope: String,
    pub language_variants: Vec<LanguageVariant>,
    /// Normalized display grouping; raw provider code kept in `hazard_raw`.
    pub hazard_type: String,
    pub hazard_raw: String,
    pub severity_raw: Option<String>,
    pub urgency_raw: Option<String>,
    pub certainty_raw: Option<String>,
    pub severity_mapping_version: String,
    pub display_priority: DisplayPriority,
    pub response_types: Vec<String>,
    pub start_at_ms: Option<i64>,
    pub onset_at_ms: Option<i64>,
    pub expires_at_ms: Option<i64>,
    pub geometry_parts: Vec<GeometryPart>,
    /// Provider-native values kept in provider terminology (e.g. magnitude, depth).
    pub native: BTreeMap<String, String>,
    /// Key of the message that produced the current revision.
    pub original_payload_ref: String,
    /// All message keys applied to this alert, oldest first (history is kept).
    pub message_history: Vec<String>,
    pub content_hash: String,
    /// `sent`/`updated` time (ms) of the newest applied message: used to
    /// reject older messages that arrive out of order.
    pub applied_message_time_ms: i64,
    /// Set when a cancellation was applied (status stays cancelled thereafter).
    pub cancelled: bool,
}
