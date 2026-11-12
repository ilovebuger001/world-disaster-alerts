//! CAP 1.2 adapter (spec §4.2 `CapSource`).
pub mod geometry;
pub mod lifecycle;
pub mod parser;

use crate::domain::lifecycle::{content_hash, stable_id};
use crate::domain::message::{MessageBody, SourceMessage};
use parser::{parse_cap_document, CapError, CapLimits};

pub struct ParsedBatch {
    pub messages: Vec<SourceMessage>,
    /// Rejected/quarantined records with reason (never silently coerced).
    pub rejected: Vec<String>,
}

pub fn parse(source_id: &str, source_url: &str, body: &[u8], fetched_at_ms: i64) -> Result<ParsedBatch, CapError> {
    let items = parse_cap_document(body, &CapLimits::default())?;
    let mut messages = Vec::new();
    let mut rejected = Vec::new();
    for item in items {
        match item {
            Ok(cap) => {
                let canonical = serde_json::to_vec(&cap).unwrap_or_default();
                messages.push(SourceMessage {
                    message_key: stable_id(&[source_id, "cap", &cap.cap_ref().key()]),
                    source_id: source_id.to_string(),
                    provider_record_id: cap.identifier.clone(),
                    source_url: source_url.to_string(),
                    fetched_at_ms,
                    content_hash: content_hash(&canonical),
                    body: MessageBody::Cap(cap),
                });
            }
            Err(e) => rejected.push(e.to_string()),
        }
    }
    Ok(ParsedBatch { messages, rejected })
}
