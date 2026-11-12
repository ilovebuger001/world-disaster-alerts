//! redb persistence and the single authoritative writer (spec §2.2, §3.3,
//! §24, §39.3, §40.3, §40.4).
//!
//! Ordering rule: one write transaction commits the alert revisions, message
//! history, reference index, source checkpoint and event-log records with
//! their sequence numbers (`Durability::Immediate`, the redb default). Only
//! after commit does the writer update the in-memory index and then publish
//! to live subscribers — on its own thread, so publication order equals
//! commit order.

use super::schema::*;
use crate::domain::alert::AlertRecord;
use crate::domain::lifecycle::{tick_status, ApplyCtx, LifecycleStore};
use crate::domain::message::{MessageBody, SourceMessage};
use crate::domain::severity::SeverityMapping;
use crate::geo::spatial_index::AlertIndex;
use crate::ingest::source_registry::SourceHealth;
use crate::realtime::event::{ApiEvent, ALERT_REMOVE, ALERT_UPSERT, SOURCE_STATUS};
use anyhow::{anyhow, Context};
use redb::{Database, ReadableTable, ReadableTableMetadata, Table};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::sync::{broadcast, mpsc, oneshot};

pub struct Db {
    db: Database,
}

impl Db {
    /// Open/create and verify the schema version. A mismatch stops startup
    /// (migrations are explicit, see migrations/redb-schema-version.md).
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let db = Database::create(path).with_context(|| format!("open redb {}", path.display()))?;
        let tx = db.begin_write()?;
        {
            tx.open_table(ALERTS)?;
            tx.open_table(MESSAGES)?;
            tx.open_table(CAP_REFS)?;
            tx.open_table(SOURCE_STATE)?;
            tx.open_table(EVENT_LOG)?;
            let mut meta = tx.open_table(METADATA)?;
            let current = meta.get(META_SCHEMA)?.map(|v| v.value());
            match current {
                None => {
                    meta.insert(META_SCHEMA, SCHEMA_VERSION)?;
                    meta.insert(META_SEQ, 0u64)?;
                    meta.insert(META_OLDEST, 1u64)?;
                }
                Some(v) if v == SCHEMA_VERSION => {}
                Some(v) => return Err(anyhow!("database schema {v} != supported {SCHEMA_VERSION}; run migration")),
            }
        }
        tx.commit()?;
        Ok(Self { db })
    }

    pub fn high_water(&self) -> anyhow::Result<u64> {
        let tx = self.db.begin_read()?;
        let meta = tx.open_table(METADATA)?;
        Ok(meta.get(META_SEQ)?.map(|v| v.value()).unwrap_or(0))
    }

    pub fn oldest_retained(&self) -> anyhow::Result<u64> {
        let tx = self.db.begin_read()?;
        let meta = tx.open_table(METADATA)?;
        Ok(meta.get(META_OLDEST)?.map(|v| v.value()).unwrap_or(1))
    }

    /// Durable events with `after < seq <= upto`, at most `limit`.
    pub fn read_events(&self, after: u64, upto: u64, limit: usize) -> anyhow::Result<Vec<ApiEvent>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(EVENT_LOG)?;
        let mut out = Vec::new();
        if after >= upto {
            return Ok(out);
        }
        for item in t.range((after + 1)..=upto)? {
            let (_, v) = item?;
            out.push(serde_json::from_slice(v.value())?);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    pub fn load_alerts(&self) -> anyhow::Result<Vec<AlertRecord>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(ALERTS)?;
        let mut out = Vec::with_capacity(t.len()? as usize);
        for item in t.iter()? {
            let (_, v) = item?;
            out.push(serde_json::from_slice(v.value())?);
        }
        Ok(out)
    }

    pub fn get_alert(&self, id: &str) -> anyhow::Result<Option<AlertRecord>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(ALERTS)?;
        Ok(match t.get(id)? {
            Some(v) => Some(serde_json::from_slice(v.value())?),
            None => None,
        })
    }

    pub fn get_message(&self, key: &str) -> anyhow::Result<Option<SourceMessage>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(MESSAGES)?;
        Ok(match t.get(key)? {
            Some(v) => Some(serde_json::from_slice(v.value())?),
            None => None,
        })
    }

    pub fn load_source_health(&self) -> anyhow::Result<HashMap<String, SourceHealth>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(SOURCE_STATE)?;
        let mut out = HashMap::new();
        for item in t.iter()? {
            let (k, v) = item?;
            out.insert(k.value().to_string(), serde_json::from_slice(v.value())?);
        }
        Ok(out)
    }
}

/// LifecycleStore over open tables of one write transaction.
struct TxStore<'a, 't> {
    alerts: &'a mut Table<'t, &'static str, &'static [u8]>,
    messages: &'a mut Table<'t, &'static str, &'static [u8]>,
    refs: &'a mut Table<'t, &'static str, &'static str>,
}

impl LifecycleStore for TxStore<'_, '_> {
    fn get_alert(&mut self, id: &str) -> anyhow::Result<Option<AlertRecord>> {
        Ok(match self.alerts.get(id)? {
            Some(v) => Some(serde_json::from_slice(v.value())?),
            None => None,
        })
    }
    fn put_alert(&mut self, a: &AlertRecord) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(a)?;
        self.alerts.insert(a.alert_id.as_str(), bytes.as_slice())?;
        Ok(())
    }
    fn get_message(&mut self, key: &str) -> anyhow::Result<Option<SourceMessage>> {
        Ok(match self.messages.get(key)? {
            Some(v) => Some(serde_json::from_slice(v.value())?),
            None => None,
        })
    }
    fn put_message(&mut self, m: &SourceMessage) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(m)?;
        self.messages.insert(m.message_key.as_str(), bytes.as_slice())?;
        Ok(())
    }
    fn get_ref(&mut self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self.refs.get(key)?.map(|v| v.value().to_string()))
    }
    fn put_ref(&mut self, key: &str, alert_id: &str) -> anyhow::Result<()> {
        self.refs.insert(key, alert_id)?;
        Ok(())
    }
}

pub enum WriteCmd {
    /// Parsed messages from one successful fetch + the source checkpoint to advance.
    Ingest {
        source_id: String,
        source_agency: String,
        messages: Vec<SourceMessage>,
        health: SourceHealth,
        reply: oneshot::Sender<anyhow::Result<Committed>>,
    },
    /// Source health only (304, failures, freshness changes).
    Health { source_id: String, health: SourceHealth, emit_status_event: bool, reply: oneshot::Sender<anyhow::Result<Committed>> },
    /// Time-based lifecycle transitions (expiry/onset).
    Tick { now_ms: i64, reply: oneshot::Sender<anyhow::Result<Committed>> },
}

#[derive(Debug, Default)]
pub struct Committed {
    pub changed: Vec<AlertRecord>,
    pub events: Vec<ApiEvent>,
    pub skipped: usize,
    /// Source checkpoint committed in this transaction (mirrored into the index).
    pub source: Option<(String, SourceHealth)>,
}

#[derive(Clone)]
pub struct WriterHandle {
    tx: mpsc::Sender<WriteCmd>,
}

impl WriterHandle {
    pub async fn send(&self, cmd: WriteCmd) -> anyhow::Result<()> {
        self.tx.send(cmd).await.map_err(|_| anyhow!("storage writer stopped"))
    }

    pub async fn ingest(
        &self,
        source_id: &str,
        source_agency: &str,
        messages: Vec<SourceMessage>,
        health: SourceHealth,
    ) -> anyhow::Result<Committed> {
        let (reply, rx) = oneshot::channel();
        self.send(WriteCmd::Ingest { source_id: source_id.into(), source_agency: source_agency.into(), messages, health, reply }).await?;
        rx.await.map_err(|_| anyhow!("writer dropped reply"))?
    }

    pub async fn health(&self, source_id: &str, health: SourceHealth, emit_status_event: bool) -> anyhow::Result<Committed> {
        let (reply, rx) = oneshot::channel();
        self.send(WriteCmd::Health { source_id: source_id.into(), health, emit_status_event, reply }).await?;
        rx.await.map_err(|_| anyhow!("writer dropped reply"))?
    }

    pub async fn tick(&self, now_ms: i64) -> anyhow::Result<Committed> {
        let (reply, rx) = oneshot::channel();
        self.send(WriteCmd::Tick { now_ms, reply }).await?;
        rx.await.map_err(|_| anyhow!("writer dropped reply"))?
    }
}

pub struct WriterConfig {
    pub queue: usize,
    pub event_log_retention: u64,
}

/// Spawn the dedicated storage thread (spec §40.4).
pub fn spawn_writer(
    db: Arc<Db>,
    mapping: Arc<SeverityMapping>,
    index: Arc<RwLock<AlertIndex>>,
    bus: broadcast::Sender<ApiEvent>,
    cfg: WriterConfig,
) -> (WriterHandle, std::thread::JoinHandle<()>) {
    let (tx, mut rx) = mpsc::channel::<WriteCmd>(cfg.queue);
    let join = std::thread::Builder::new()
        .name("storage-writer".into())
        .spawn(move || {
            while let Some(cmd) = rx.blocking_recv() {
                let (result, reply) = match cmd {
                    WriteCmd::Ingest { source_id, source_agency, messages, health, reply } => {
                        (commit_ingest(&db, &mapping, &source_id, &source_agency, &messages, &health, cfg.event_log_retention), reply)
                    }
                    WriteCmd::Health { source_id, health, emit_status_event, reply } => {
                        (commit_health(&db, &source_id, &health, emit_status_event, cfg.event_log_retention), reply)
                    }
                    WriteCmd::Tick { now_ms, reply } => (commit_tick(&db, now_ms, cfg.event_log_retention), reply),
                };
                if let Ok(c) = &result {
                    // 2. index after commit
                    {
                        let mut idx = index.write().expect("index lock poisoned");
                        let last = c.events.last().map(|e| e.event_sequence).unwrap_or(idx.seq);
                        idx.apply(&c.changed, last);
                        if let Some((id, h)) = &c.source {
                            idx.set_source(id, h.clone());
                        }
                    }
                    // 3. publish last (no receivers is fine)
                    for e in &c.events {
                        let _ = bus.send(e.clone());
                    }
                }
                let _ = reply.send(result);
            }
            tracing::info!("storage writer drained and stopped");
        })
        .expect("spawn storage writer");
    (WriterHandle { tx }, join)
}

fn alert_event(seq: u64, a: &AlertRecord, now_ms: i64) -> ApiEvent {
    ApiEvent {
        event_sequence: seq,
        event_type: if a.status.is_live() { ALERT_UPSERT } else { ALERT_REMOVE }.to_string(),
        alert_id: Some(a.alert_id.clone()),
        revision: Some(a.revision),
        status: Some(a.status.as_str().to_string()),
        display_priority: Some(
            serde_json::to_value(a.display_priority).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        ),
        hazard_type: Some(a.hazard_type.clone()),
        source_id: Some(a.source_id.clone()),
        freshness: None,
        committed_at_ms: now_ms,
    }
}

/// Appends events with consecutive sequence numbers and applies retention.
fn append_events(tx: &redb::WriteTransaction, mut make: impl FnMut(u64) -> Vec<ApiEvent>, retention: u64) -> anyhow::Result<Vec<ApiEvent>> {
    let mut meta = tx.open_table(METADATA)?;
    let mut log = tx.open_table(EVENT_LOG)?;
    let start = meta.get(META_SEQ)?.map(|v| v.value()).unwrap_or(0);
    let events = make(start + 1);
    let mut seq = start;
    for e in &events {
        seq += 1;
        debug_assert_eq!(e.event_sequence, seq);
        let bytes = serde_json::to_vec(e)?;
        log.insert(seq, bytes.as_slice())?;
    }
    meta.insert(META_SEQ, seq)?;
    // Bounded retention: drop the oldest entries beyond the retention window.
    let mut oldest = meta.get(META_OLDEST)?.map(|v| v.value()).unwrap_or(1);
    let mut removed = 0;
    while seq >= oldest && seq - oldest + 1 > retention && removed < 10_000 {
        if log.pop_first()?.is_none() {
            break;
        }
        oldest += 1;
        removed += 1;
    }
    meta.insert(META_OLDEST, oldest)?;
    Ok(events)
}

fn commit_ingest(
    db: &Db,
    mapping: &SeverityMapping,
    source_id: &str,
    source_agency: &str,
    messages: &[SourceMessage],
    health: &SourceHealth,
    retention: u64,
) -> anyhow::Result<Committed> {
    let now = crate::domain::now_ms();
    let tx = db.db.begin_write()?;
    let mut changed: Vec<AlertRecord> = Vec::new();
    let mut skipped = 0;
    {
        let mut alerts = tx.open_table(ALERTS)?;
        let mut msgs = tx.open_table(MESSAGES)?;
        let mut refs = tx.open_table(CAP_REFS)?;
        let mut store = TxStore { alerts: &mut alerts, messages: &mut msgs, refs: &mut refs };
        let ctx = ApplyCtx { source_id, source_agency, mapping, now_ms: now };
        for m in messages {
            let outcome = match &m.body {
                MessageBody::Cap(c) => crate::ingest::cap::lifecycle::apply_cap(&mut store, m, c, &ctx)?,
                MessageBody::Usgs(u) => crate::ingest::usgs::apply_usgs(&mut store, m, u, &ctx)?,
            };
            skipped += outcome.skipped.len();
            for r in outcome.changed {
                // Several messages in one batch may touch the same alert: keep the last.
                changed.retain(|c| c.alert_id != r.alert_id);
                changed.push(r);
            }
        }
        let mut st = tx.open_table(SOURCE_STATE)?;
        let hb = serde_json::to_vec(health)?;
        st.insert(source_id, hb.as_slice())?;
    }
    let events =
        append_events(&tx, |first| changed.iter().enumerate().map(|(i, a)| alert_event(first + i as u64, a, now)).collect(), retention)?;
    tx.commit()?;
    Ok(Committed { changed, events, skipped, source: Some((source_id.to_string(), health.clone())) })
}

fn commit_health(db: &Db, source_id: &str, health: &SourceHealth, emit: bool, retention: u64) -> anyhow::Result<Committed> {
    let now = crate::domain::now_ms();
    let tx = db.db.begin_write()?;
    {
        let mut st = tx.open_table(SOURCE_STATE)?;
        let hb = serde_json::to_vec(health)?;
        st.insert(source_id, hb.as_slice())?;
    }
    let events = if emit {
        append_events(
            &tx,
            |first| {
                vec![ApiEvent {
                    event_sequence: first,
                    event_type: SOURCE_STATUS.into(),
                    alert_id: None,
                    revision: None,
                    status: None,
                    display_priority: None,
                    hazard_type: None,
                    source_id: Some(source_id.into()),
                    freshness: serde_json::to_value(health.freshness()).ok().and_then(|v| v.as_str().map(str::to_string)),
                    committed_at_ms: now,
                }]
            },
            retention,
        )?
    } else {
        vec![]
    };
    tx.commit()?;
    Ok(Committed { changed: vec![], events, skipped: 0, source: Some((source_id.to_string(), health.clone())) })
}

fn commit_tick(db: &Db, now_ms: i64, retention: u64) -> anyhow::Result<Committed> {
    let tx = db.db.begin_write()?;
    let mut changed = Vec::new();
    {
        let mut alerts = tx.open_table(ALERTS)?;
        let mut updates = Vec::new();
        for item in alerts.iter()? {
            let (_, v) = item?;
            let a: AlertRecord = serde_json::from_slice(v.value())?;
            if a.status.is_live() {
                if let Some(n) = tick_status(&a, now_ms) {
                    updates.push(n);
                }
            }
        }
        for n in updates {
            let b = serde_json::to_vec(&n)?;
            alerts.insert(n.alert_id.as_str(), b.as_slice())?;
            changed.push(n);
        }
    }
    let events =
        append_events(&tx, |first| changed.iter().enumerate().map(|(i, a)| alert_event(first + i as u64, a, now_ms)).collect(), retention)?;
    tx.commit()?;
    Ok(Committed { changed, events, skipped: 0, source: None })
}
