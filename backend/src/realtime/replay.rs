//! No-gap replay + live delivery (spec §10.2, §25.4, §25.5, §41.6).
//!
//! 1. subscribe to the live broadcast first;
//! 2. read the durable high-water mark R;
//! 3. replay durable events (after, R];
//! 4. consume live events, skipping anything <= last delivered;
//! 5. on broadcast lag, replay again from the durable log;
//! 6. if the cursor predates retained history (or the backlog is larger than
//!    the bounded replay budget), emit `stream.reset` and stop.
//!
//! Each client gets a bounded queue; a slow client gets a reset instead of
//! unbounded buffering (spec §25.3, §56 item 5).

use super::event::ApiEvent;
use crate::storage::redb::Db;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

#[derive(Clone, Debug, PartialEq)]
pub enum StreamItem {
    Event(ApiEvent),
    Reset { reason: String, high_water: u64 },
}

pub struct ReplayConfig {
    pub max_replay_events: u64,
    pub client_queue: usize,
}

fn durable_gap(db: &Db, last: u64, max: u64) -> Result<Result<Vec<ApiEvent>, StreamItem>, anyhow::Error> {
    let high = db.high_water()?;
    let oldest = db.oldest_retained()?;
    if last > high {
        return Ok(Err(StreamItem::Reset { reason: "cursor_ahead_of_server".into(), high_water: high }));
    }
    if last + 1 < oldest {
        return Ok(Err(StreamItem::Reset { reason: "cursor_older_than_retained_history".into(), high_water: high }));
    }
    if high - last > max {
        return Ok(Err(StreamItem::Reset { reason: "replay_backlog_too_large".into(), high_water: high }));
    }
    Ok(Ok(db.read_events(last, high, max as usize)?))
}

/// Starts the per-client producer and returns the bounded receiver.
pub fn start(db: Arc<Db>, bus: &broadcast::Sender<ApiEvent>, after: u64, cfg: ReplayConfig) -> mpsc::Receiver<StreamItem> {
    let (tx, rx) = mpsc::channel(cfg.client_queue);
    let mut live = bus.subscribe(); // (1) subscribe before reading the durable range
    tokio::spawn(async move {
        let mut last = after;
        // Sends one item; returns false when the client is gone or too slow.
        async fn push(tx: &mpsc::Sender<StreamItem>, item: StreamItem) -> bool {
            match tx.try_send(item) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // Slow consumer: tell it to resync, then close.
                    let _ = tx.send(StreamItem::Reset { reason: "client_too_slow".into(), high_water: 0 }).await;
                    false
                }
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            }
        }
        let replay = |last: u64| {
            let db = db.clone();
            let max = cfg.max_replay_events;
            async move { tokio::task::spawn_blocking(move || durable_gap(&db, last, max)).await }
        };
        // (2)+(3)
        match replay(last).await {
            Ok(Ok(Ok(events))) => {
                for e in events {
                    let seq = e.event_sequence;
                    if !push(&tx, StreamItem::Event(e)).await {
                        return;
                    }
                    last = seq;
                }
            }
            Ok(Ok(Err(reset))) => {
                let _ = push(&tx, reset).await;
                return;
            }
            _ => {
                let _ = push(&tx, StreamItem::Reset { reason: "replay_failed".into(), high_water: 0 }).await;
                return;
            }
        }
        // (4)/(5)
        loop {
            let next = tokio::select! {
                r = live.recv() => r,
                _ = tx.closed() => return,
            };
            match next {
                Ok(e) => {
                    if e.event_sequence <= last {
                        continue;
                    }
                    if e.event_sequence > last + 1 {
                        // A gap (e.g. events committed between replay and subscribe ordering): fill from durable log.
                        match replay(last).await {
                            Ok(Ok(Ok(events))) => {
                                for d in events {
                                    let seq = d.event_sequence;
                                    if seq <= last {
                                        continue;
                                    }
                                    if !push(&tx, StreamItem::Event(d)).await {
                                        return;
                                    }
                                    last = seq;
                                }
                                if e.event_sequence <= last {
                                    continue;
                                }
                            }
                            Ok(Ok(Err(reset))) => {
                                let _ = push(&tx, reset).await;
                                return;
                            }
                            _ => return,
                        }
                    }
                    let seq = e.event_sequence;
                    if !push(&tx, StreamItem::Event(e)).await {
                        return;
                    }
                    last = seq;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => match replay(last).await {
                    Ok(Ok(Ok(events))) => {
                        for d in events {
                            let seq = d.event_sequence;
                            if !push(&tx, StreamItem::Event(d)).await {
                                return;
                            }
                            last = seq;
                        }
                    }
                    Ok(Ok(Err(reset))) => {
                        let _ = push(&tx, reset).await;
                        return;
                    }
                    _ => return,
                },
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    rx
}
