//! Storage-agnostic lifecycle engine (spec §5.3, §5.4, §39.3 single-writer
//! state transition). Source adapters build records; this module owns the
//! rules for identity, idempotency, ordering and status derivation.

use super::alert::{AlertRecord, AlertStatus};
use super::message::SourceMessage;
use super::severity::SeverityMapping;
use sha2::{Digest, Sha256};

/// Read/write view used inside one serialized write transaction.
/// Implemented by the redb writer and by an in-memory store for tests.
pub trait LifecycleStore {
    fn get_alert(&mut self, id: &str) -> anyhow::Result<Option<AlertRecord>>;
    fn put_alert(&mut self, a: &AlertRecord) -> anyhow::Result<()>;
    fn get_message(&mut self, key: &str) -> anyhow::Result<Option<SourceMessage>>;
    fn put_message(&mut self, m: &SourceMessage) -> anyhow::Result<()>;
    /// Reference index: CAP `(sender,identifier,sent)` keys and provider ids → alert id.
    fn get_ref(&mut self, key: &str) -> anyhow::Result<Option<String>>;
    fn put_ref(&mut self, key: &str, alert_id: &str) -> anyhow::Result<()>;
}

/// Context for one ingestion batch.
pub struct ApplyCtx<'a> {
    pub source_id: &'a str,
    pub source_agency: &'a str,
    pub mapping: &'a SeverityMapping,
    pub now_ms: i64,
}

/// Why a message did not create/modify a public record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipReason {
    DuplicateMessage,
    /// Same message identity with different bytes: CAP messages are immutable.
    ConflictingDuplicate,
    NotActual(String),
    NotPublic(String),
    NonAlertMessageType(String),
    OlderThanApplied,
    AlreadyCancelled,
    NoUsableContent,
}

#[derive(Debug, Default)]
pub struct ApplyOutcome {
    /// Records whose revision changed in this application (to persist + publish).
    pub changed: Vec<AlertRecord>,
    pub skipped: Vec<(String, SkipReason)>,
}

pub fn stable_id(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            h.update([0x1f]);
        }
        h.update(p.as_bytes());
    }
    hex::encode(&h.finalize()[..12])
}

pub fn content_hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Derive the canonical status from source facts and the clock.
/// `expired` = the source said so (stated expiry passed); a freshness timeout
/// never rewrites expiry (spec §5.4). Staleness is reported separately.
pub fn derive_status(
    now_ms: i64,
    start_ms: Option<i64>,
    onset_ms: Option<i64>,
    expires_ms: Option<i64>,
    cancelled: bool,
    withdrawn: bool,
) -> AlertStatus {
    if cancelled {
        return AlertStatus::Cancelled;
    }
    if withdrawn {
        return AlertStatus::Withdrawn;
    }
    if let Some(e) = expires_ms {
        if e <= now_ms {
            return AlertStatus::Expired;
        }
    }
    let begins = match (start_ms, onset_ms) {
        (_, Some(o)) => Some(o),
        (Some(s), None) => Some(s),
        _ => None,
    };
    if let Some(b) = begins {
        if b > now_ms {
            return AlertStatus::Upcoming;
        }
    }
    AlertStatus::Active
}

/// Re-evaluate time-based status (expiry worker). Returns the new record when
/// the status changed; history is never erased.
pub fn tick_status(a: &AlertRecord, now_ms: i64) -> Option<AlertRecord> {
    let withdrawn = a.status == AlertStatus::Withdrawn;
    let s = derive_status(now_ms, a.start_at_ms, a.onset_at_ms, a.expires_at_ms, a.cancelled, withdrawn);
    if s != a.status {
        let mut n = a.clone();
        n.status = s;
        n.revision += 1;
        n.last_seen_at_ms = a.last_seen_at_ms;
        Some(n)
    } else {
        None
    }
}

/// Shared duplicate check + message persistence. Returns `Some(reason)` when
/// the message must not be applied again.
pub fn record_message(store: &mut dyn LifecycleStore, msg: &SourceMessage) -> anyhow::Result<Option<SkipReason>> {
    if let Some(prev) = store.get_message(&msg.message_key)? {
        return Ok(Some(if prev.content_hash == msg.content_hash {
            SkipReason::DuplicateMessage
        } else {
            SkipReason::ConflictingDuplicate
        }));
    }
    store.put_message(msg)?;
    Ok(None)
}

/// Appends a message key to history without changing revision (older message).
pub fn append_history_only(store: &mut dyn LifecycleStore, existing: &AlertRecord, message_key: &str) -> anyhow::Result<()> {
    if !existing.message_history.iter().any(|k| k == message_key) {
        let mut a = existing.clone();
        a.message_history.push(message_key.to_string());
        store.put_alert(&a)?;
    }
    Ok(())
}

/// In-memory implementation used by unit/integration tests.
#[derive(Default)]
pub struct MemStore {
    pub alerts: std::collections::HashMap<String, AlertRecord>,
    pub messages: std::collections::HashMap<String, SourceMessage>,
    pub refs: std::collections::HashMap<String, String>,
}

impl LifecycleStore for MemStore {
    fn get_alert(&mut self, id: &str) -> anyhow::Result<Option<AlertRecord>> {
        Ok(self.alerts.get(id).cloned())
    }
    fn put_alert(&mut self, a: &AlertRecord) -> anyhow::Result<()> {
        self.alerts.insert(a.alert_id.clone(), a.clone());
        Ok(())
    }
    fn get_message(&mut self, key: &str) -> anyhow::Result<Option<SourceMessage>> {
        Ok(self.messages.get(key).cloned())
    }
    fn put_message(&mut self, m: &SourceMessage) -> anyhow::Result<()> {
        self.messages.insert(m.message_key.clone(), m.clone());
        Ok(())
    }
    fn get_ref(&mut self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self.refs.get(key).cloned())
    }
    fn put_ref(&mut self, key: &str, alert_id: &str) -> anyhow::Result<()> {
        self.refs.insert(key.to_string(), alert_id.to_string());
        Ok(())
    }
}
