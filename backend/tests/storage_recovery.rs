//! Phase 1 exit condition: CAP updates/cancellations and USGS updates are
//! idempotent, persisted, and correctly represented after restart (spec §16,
//! §15.4 "restart rebuilds the spatial index and continues sequence numbering").
mod common;
use common::*;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use wda::domain::alert::AlertStatus;
use wda::domain::message::SourceMessage;
use wda::geo::spatial_index::AlertIndex;
use wda::ingest::source_registry::SourceHealth;
use wda::realtime::event::ApiEvent;
use wda::storage::redb::{spawn_writer, Db, WriterConfig, WriterHandle};

struct Node {
    db: Arc<Db>,
    index: Arc<RwLock<AlertIndex>>,
    writer: WriterHandle,
    join: std::thread::JoinHandle<()>,
    bus: broadcast::Sender<ApiEvent>,
}

fn boot(dir: &std::path::Path, retention: u64) -> Node {
    let db = Arc::new(Db::open(&dir.join("t.redb")).unwrap());
    let index =
        Arc::new(RwLock::new(AlertIndex::rebuild(db.load_alerts().unwrap(), db.high_water().unwrap(), db.load_source_health().unwrap())));
    let (bus, _) = broadcast::channel(64);
    let (writer, join) = spawn_writer(
        db.clone(),
        Arc::new(mapping()),
        index.clone(),
        bus.clone(),
        WriterConfig { queue: 16, event_log_retention: retention },
    );
    Node { db, index, writer, join, bus }
}

fn shutdown(n: Node) {
    let Node { db, index, writer, join, bus } = n;
    drop(writer);
    join.join().unwrap();
    drop((db, index, bus));
}

fn cap_msgs(name: &str) -> Vec<SourceMessage> {
    wda::ingest::cap::parse("sample-cap", "https://example.invalid/feed.xml", &cap(name), NOW_MS).unwrap().messages
}
fn usgs_msgs(name: &str) -> Vec<SourceMessage> {
    wda::ingest::usgs::parse("usgs-earthquakes-all-day", "https://earthquake.usgs.gov/x", &geojson(name), NOW_MS).unwrap().messages
}

#[tokio::test(flavor = "multi_thread")]
async fn lifecycle_survives_restart_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let n = boot(dir.path(), 100_000);
    let mut rx = n.bus.subscribe();
    let h = SourceHealth { etag: Some("\"v1\"".into()), ..Default::default() };

    let c1 = n.writer.ingest("sample-cap", "Sample", cap_msgs("alert.xml"), h.clone()).await.unwrap();
    assert_eq!(c1.events.len(), 1);
    assert_eq!(c1.events[0].event_sequence, 1);
    let alert_id = c1.changed[0].alert_id.clone();
    // publish happens after commit and index update
    let live = rx.recv().await.unwrap();
    assert_eq!(live.event_sequence, 1);
    assert!(n.index.read().unwrap().alerts.contains_key(&alert_id));

    n.writer.ingest("sample-cap", "Sample", cap_msgs("update.xml"), h.clone()).await.unwrap();
    let c3 = n.writer.ingest("sample-cap", "Sample", cap_msgs("cancel.xml"), h.clone()).await.unwrap();
    assert_eq!(c3.events[0].event_type, "alert.remove");

    let u1 = n.writer.ingest("usgs-earthquakes-all-day", "USGS", usgs_msgs("usgs_v1.json"), h.clone()).await.unwrap();
    assert_eq!(u1.changed.len(), 3, "two located events + one null-geometry event; polygon feature skipped");
    let u2 = n.writer.ingest("usgs-earthquakes-all-day", "USGS", usgs_msgs("usgs_v2.json"), h.clone()).await.unwrap();
    assert_eq!(u2.changed.len(), 2);
    let u3 = n.writer.ingest("usgs-earthquakes-all-day", "USGS", usgs_msgs("usgs_preferred_id_change.json"), h.clone()).await.unwrap();
    assert_eq!(u3.changed.len(), 1);
    let quake_id = u3.changed[0].alert_id.clone();
    assert_eq!(u3.changed[0].revision, 3, "preferred-id change updates the same event");

    let seq_before = n.db.high_water().unwrap();
    // Re-ingesting everything is a no-op.
    for f in ["alert.xml", "update.xml", "cancel.xml"] {
        let c = n.writer.ingest("sample-cap", "Sample", cap_msgs(f), h.clone()).await.unwrap();
        assert!(c.events.is_empty(), "{f} duplicate produced events");
    }
    for f in ["usgs_v1.json", "usgs_v2.json", "usgs_preferred_id_change.json"] {
        let c = n.writer.ingest("usgs-earthquakes-all-day", "USGS", usgs_msgs(f), h.clone()).await.unwrap();
        assert!(c.events.is_empty(), "{f} duplicate produced events");
    }
    assert_eq!(n.db.high_water().unwrap(), seq_before);
    shutdown(n);

    // ---- restart ----
    let n = boot(dir.path(), 100_000);
    {
        let idx = n.index.read().unwrap();
        assert_eq!(idx.seq, seq_before, "index rebuilt at the committed sequence");
        let a = &idx.alerts[&alert_id];
        assert_eq!(a.status, AlertStatus::Cancelled);
        assert_eq!(a.revision, 3);
        assert_eq!(a.message_history.len(), 3);
        let q = &idx.alerts[&quake_id];
        assert_eq!(q.native.get("magnitude").map(String::as_str), Some("5.7"));
        assert_eq!(q.revision, 3);
        let deleted = idx.alerts.values().find(|a| a.source_record_id == "ak0261xyz").unwrap();
        assert_eq!(deleted.status, AlertStatus::Withdrawn);
        assert_eq!(idx.sources["sample-cap"].etag.as_deref(), Some("\"v1\""), "checkpoint persisted with records");
        // cancelled/withdrawn records are not in the live map index
        assert_eq!(idx.live_count(), 1, "only the located Chile quake is live on the map (unlocated has no box)");
    }
    // sequence numbering continues
    let more = n.writer.ingest("sample-cap", "Sample", cap_msgs("feed_multi.xml"), SourceHealth::default()).await.unwrap();
    assert_eq!(more.events[0].event_sequence, seq_before + 1);
    // durable log replays everything after a cursor
    let ev = n.db.read_events(seq_before - 2, seq_before + 1, 100).unwrap();
    assert_eq!(ev.iter().map(|e| e.event_sequence).collect::<Vec<_>>(), vec![seq_before - 1, seq_before, seq_before + 1]);
    shutdown(n);
}

#[tokio::test(flavor = "multi_thread")]
async fn event_log_retention_advances_oldest() {
    let dir = tempfile::tempdir().unwrap();
    let n = boot(dir.path(), 2);
    let h = SourceHealth::default();
    n.writer.ingest("sample-cap", "S", cap_msgs("alert.xml"), h.clone()).await.unwrap();
    n.writer.ingest("sample-cap", "S", cap_msgs("update.xml"), h.clone()).await.unwrap();
    n.writer.ingest("sample-cap", "S", cap_msgs("cancel.xml"), h.clone()).await.unwrap();
    assert_eq!(n.db.high_water().unwrap(), 3);
    assert_eq!(n.db.oldest_retained().unwrap(), 2);
    assert_eq!(n.db.read_events(0, 3, 10).unwrap().len(), 2);
    shutdown(n);
}

#[tokio::test(flavor = "multi_thread")]
async fn tick_expires_without_erasing_history() {
    let dir = tempfile::tempdir().unwrap();
    let n = boot(dir.path(), 100);
    let c = n.writer.ingest("sample-cap", "S", cap_msgs("alert.xml"), SourceHealth::default()).await.unwrap();
    let id = c.changed[0].alert_id.clone();
    // fixture expires 2099-01-01; tick after that
    let t = n.writer.tick(4_070_908_800_001).await.unwrap();
    assert_eq!(t.changed.len(), 1);
    assert_eq!(t.events[0].event_type, "alert.remove");
    let a = n.db.get_alert(&id).unwrap().unwrap();
    assert_eq!(a.status, AlertStatus::Expired);
    assert_eq!(a.message_history.len(), 1);
    assert_eq!(n.index.read().unwrap().live_count(), 0);
    shutdown(n);
}
