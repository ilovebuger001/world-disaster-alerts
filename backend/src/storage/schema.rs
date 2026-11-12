//! redb logical tables (spec §3.3). Translation tables arrive with Phase 4.
//! Values are JSON-encoded records; keys are stable identifiers.

use redb::TableDefinition;

pub const SCHEMA_VERSION: u64 = 1;

pub const ALERTS: TableDefinition<&str, &[u8]> = TableDefinition::new("alerts");
pub const MESSAGES: TableDefinition<&str, &[u8]> = TableDefinition::new("messages");
/// CAP `(sender, identifier, sent)` keys and provider ids → alert id.
pub const CAP_REFS: TableDefinition<&str, &str> = TableDefinition::new("cap_refs");
pub const SOURCE_STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("source_state");
/// Monotonic sequence (u64 keys are ordered numerically) → API event.
pub const EVENT_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("event_log");
pub const METADATA: TableDefinition<&str, u64> = TableDefinition::new("metadata");

pub const META_SCHEMA: &str = "schema_version";
pub const META_SEQ: &str = "current_sequence";
pub const META_OLDEST: &str = "oldest_retained_sequence";
