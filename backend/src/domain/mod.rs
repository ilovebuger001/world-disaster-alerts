//! Canonical domain model (spec §5, §6, §7).
pub mod alert;
pub mod geometry;
pub mod lifecycle;
pub mod message;
pub mod severity;

/// Parse an RFC 3339 / CAP date-time (always timezone-aware) to UTC Unix ms.
/// Never interpreted as local machine time (spec §5.4).
pub fn parse_ts_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s.trim()).ok().map(|d| d.timestamp_millis())
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
