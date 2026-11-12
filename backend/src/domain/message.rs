//! Immutable source messages (spec §5.1 entity 1) with provenance (§4.5).

use super::geometry::GeoPoint;
use serde::{Deserialize, Serialize};

/// Exact CAP reference triple `(sender, identifier, sent)` (spec §5.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapRef {
    pub sender: String,
    pub identifier: String,
    /// Original `sent` string exactly as published.
    pub sent: String,
}

impl CapRef {
    /// Storage key. Uses a separator that cannot appear in CAP identifiers
    /// (CAP forbids spaces, commas and restricted characters in these fields).
    pub fn key(&self) -> String {
        format!("{}\u{1f}{}\u{1f}{}", self.sender, self.identifier, self.sent)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapArea {
    pub area_desc: String,
    /// Each polygon is a closed ring converted from CAP `[lat,lon]` pairs.
    pub polygons: Vec<Vec<GeoPoint>>,
    pub circles: Vec<CapCircle>,
    /// `(valueName, value)` preserved verbatim.
    pub geocodes: Vec<(String, String)>,
    pub altitude: Option<String>,
    pub ceiling: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapCircle {
    pub center: GeoPoint,
    pub radius_km: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CapInfo {
    pub language: String,
    pub categories: Vec<String>,
    pub event: String,
    pub response_types: Vec<String>,
    pub urgency: String,
    pub severity: String,
    pub certainty: String,
    pub audience: Option<String>,
    pub event_codes: Vec<(String, String)>,
    pub effective: Option<String>,
    pub onset: Option<String>,
    pub expires: Option<String>,
    pub sender_name: Option<String>,
    pub headline: Option<String>,
    pub description: Option<String>,
    pub instruction: Option<String>,
    pub web: Option<String>,
    pub contact: Option<String>,
    pub parameters: Vec<(String, String)>,
    pub areas: Vec<CapArea>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapMessage {
    pub identifier: String,
    pub sender: String,
    pub sent: String,
    pub sent_ms: i64,
    /// Actual | Exercise | System | Test | Draft (verbatim).
    pub status: String,
    /// Alert | Update | Cancel | Ack | Error (verbatim).
    pub msg_type: String,
    pub source: Option<String>,
    /// Public | Restricted | Private (verbatim).
    pub scope: String,
    pub restriction: Option<String>,
    pub addresses: Option<String>,
    pub codes: Vec<String>,
    pub note: Option<String>,
    pub references: Vec<CapRef>,
    pub incidents: Option<String>,
    pub infos: Vec<CapInfo>,
    /// Signature presence only; we do NOT verify it, so no verification claim (§4.5).
    pub has_signature: bool,
}

impl CapMessage {
    pub fn cap_ref(&self) -> CapRef {
        CapRef { sender: self.sender.clone(), identifier: self.identifier.clone(), sent: self.sent.clone() }
    }
}

/// USGS ComCat GeoJSON event (spec §4.2, §7.1 earthquakes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsgsEvent {
    pub id: String,
    /// All associated ids (`ids` property), used to follow preferred-id changes.
    pub ids: Vec<String>,
    pub point: Option<GeoPoint>,
    pub depth_km: Option<f64>,
    pub mag: Option<f64>,
    pub mag_type: Option<String>,
    pub place: Option<String>,
    pub title: Option<String>,
    pub time_ms: i64,
    pub updated_ms: i64,
    pub url: Option<String>,
    pub detail: Option<String>,
    /// automatic | reviewed | deleted (verbatim).
    pub status: Option<String>,
    pub event_type: Option<String>,
    pub tsunami: Option<i64>,
    pub alert: Option<String>,
    pub mmi: Option<f64>,
    pub cdi: Option<f64>,
    pub felt: Option<i64>,
    pub sig: Option<i64>,
    pub net: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageBody {
    Cap(CapMessage),
    Usgs(UsgsEvent),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceMessage {
    /// Stable message key (identity), separate from the content hash (§31).
    pub message_key: String,
    pub source_id: String,
    pub provider_record_id: String,
    pub source_url: String,
    pub fetched_at_ms: i64,
    /// SHA-256 of the record's canonical bytes (dedup, not identity).
    pub content_hash: String,
    pub body: MessageBody,
}
