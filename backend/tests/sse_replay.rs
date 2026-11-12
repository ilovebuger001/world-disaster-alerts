//! Phase 2 exit condition / spec §15.4 SSE and recovery tests.
mod common;
use common::*;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use wda::geo::spatial_index::AlertIndex;
use wda::ingest::source_registry::SourceHealth;
use wda::realtime::replay::{start, ReplayConfig, StreamItem};
use wda::storage::redb::{spawn_writer, Db, WriterConfig};

async fn next(rx: &mut tokio::sync::mpsc::Receiver<StreamItem>) -> StreamItem {
    tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("timely").expect("item")
}

fn seq(i: StreamItem) -> u64 {
    match i {
        StreamItem::Event(e) => e.event_sequence,
        StreamItem::Reset { reason, .. } => panic!("unexpected reset: {reason}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_after_cursor_then_live_without_gaps_or_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open(&dir.path().join("s.redb")).unwrap());
    let index = Arc::new(RwLock::new(AlertIndex::default()));
    let (bus, _) = broadcast::channel(4);
    let (writer, _join) =
        spawn_writer(db.clone(), Arc::new(mapping()), index, bus.clone(), WriterConfig { queue: 8, event_log_retention: 1000 });
    let msgs = |f: &str| wda::ingest::cap::parse("c", "https://example.invalid", &cap(f), NOW_MS).unwrap().messages;
    writer.ingest("c", "C", msgs("alert.xml"), SourceHealth::default()).await.unwrap(); // seq 1
    writer.ingest("c", "C", msgs("update.xml"), SourceHealth::default()).await.unwrap(); // seq 2

    // Client that saw seq 1 reconnects (Last-Event-ID / after = 1).
    let mut rx = start(db.clone(), &bus, 1, ReplayConfig { max_replay_events: 100, client_queue: 16 });
    assert_eq!(seq(next(&mut rx).await), 2, "durable replay of the missed event");
    // An event committed after connecting arrives live exactly once.
    writer.ingest("c", "C", msgs("cancel.xml"), SourceHealth::default()).await.unwrap(); // seq 3
    assert_eq!(seq(next(&mut rx).await), 3);
    // Burst larger than the broadcast buffer (4) forces Lagged → durable replay.
    for i in 0..10u32 {
        writer.health("c", SourceHealth { consecutive_failures: i, ..Default::default() }, true).await.unwrap();
    }
    let mut got = Vec::new();
    while got.len() < 10 {
        got.push(seq(next(&mut rx).await));
    }
    assert_eq!(got, (4..=13).collect::<Vec<u64>>(), "lag recovered from durable log, ordered, no duplicates");
}

#[tokio::test(flavor = "multi_thread")]
async fn cursor_older_than_retention_forces_reset() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open(&dir.path().join("r.redb")).unwrap());
    let index = Arc::new(RwLock::new(AlertIndex::default()));
    let (bus, _) = broadcast::channel(16);
    let (writer, _join) =
        spawn_writer(db.clone(), Arc::new(mapping()), index, bus.clone(), WriterConfig { queue: 8, event_log_retention: 3 });
    for i in 0..6u32 {
        writer.health("c", SourceHealth { consecutive_failures: i, ..Default::default() }, true).await.unwrap();
    }
    let mut rx = start(db.clone(), &bus, 1, ReplayConfig { max_replay_events: 100, client_queue: 16 });
    match next(&mut rx).await {
        StreamItem::Reset { reason, high_water } => {
            assert_eq!(reason, "cursor_older_than_retained_history");
            assert_eq!(high_water, 6);
        }
        other => panic!("expected reset, got {other:?}"),
    }
    // Cursor ahead of the server (e.g. restored backup) also resets.
    let mut rx = start(db.clone(), &bus, 99, ReplayConfig { max_replay_events: 100, client_queue: 16 });
    assert!(matches!(next(&mut rx).await, StreamItem::Reset { .. }));
    // Backlog larger than the bounded replay budget resets instead of flooding.
    let mut rx = start(db.clone(), &bus, 3, ReplayConfig { max_replay_events: 2, client_queue: 16 });
    assert!(matches!(next(&mut rx).await, StreamItem::Reset { .. }));
}
