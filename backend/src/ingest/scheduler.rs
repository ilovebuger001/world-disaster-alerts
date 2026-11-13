//! Source scheduler / worker pool, health worker and expiry worker
//! (spec §3.1 task model, §4.3 fetch policy, §38.1 jitter, §77.5 freshness).

use super::http_client::{fetch_once, FetchError, FetchOutcome};
use super::source_registry::{compute_freshness, SourceConfig, SourceFormat, SourceHealth};
use crate::domain::lifecycle::content_hash;
use crate::domain::now_ms;
use crate::geo::spatial_index::AlertIndex;
use crate::storage::redb::WriterHandle;
use std::hash::{BuildHasher, Hasher};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{watch, Semaphore};

#[derive(Clone)]
pub struct IngestRuntime {
    pub client: reqwest::Client,
    pub writer: WriterHandle,
    pub index: Arc<RwLock<AlertIndex>>,
    pub in_flight: Arc<Semaphore>,
    pub allow_private: bool,
}

/// Random value in [0, max) without an extra dependency.
pub fn jitter_ms(max: u64) -> u64 {
    if max == 0 {
        return 0;
    }
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_i64(now_ms());
    h.finish() % max
}

/// Exponential backoff with jitter, capped (spec §4.3).
pub fn backoff_delay(err: &FetchError, failures: u32, poll_s: u64) -> Duration {
    let base = poll_s.max(15);
    let secs = match err {
        FetchError::Status { code: 429, retry_after_s } => retry_after_s.unwrap_or(base).max(base),
        e if e.is_permanent() => (base * 10).max(3600), // avoid hammering a misconfigured endpoint
        _ => (base.saturating_mul(1u64 << failures.min(6))).min(1800),
    };
    Duration::from_millis(secs * 1000 + jitter_ms(secs * 250))
}

fn current_health(rt: &IngestRuntime, id: &str) -> SourceHealth {
    rt.index.read().expect("index lock").sources.get(id).cloned().unwrap_or_default()
}

/// One poll cycle. Returns the delay before the next cycle.
pub async fn poll_once(rt: &IngestRuntime, src: &SourceConfig) -> Duration {
    let _permit = rt.in_flight.acquire().await.expect("semaphore");
    let mut h = current_health(rt, &src.source_id);
    let started = now_ms();
    h.last_attempt_at_ms = Some(started);
    let poll = Duration::from_secs(src.poll_interval_seconds) + Duration::from_millis(jitter_ms(src.poll_interval_seconds * 100));
    let result = fetch_once(&rt.client, src, h.etag.as_deref(), h.last_modified.as_deref(), rt.allow_private).await;
    let delay;
    match result {
        Ok(FetchOutcome::NotModified) => {
            h.last_http_success_at_ms = Some(now_ms());
            h.consecutive_failures = 0;
            h.last_error_code = None;
            h.last_error_summary = None;
            h.freshness_state = Some(compute_freshness(&h, src.expected_freshness_seconds, now_ms()));
            if let Err(e) = rt.writer.health(&src.source_id, h, false).await {
                tracing::error!(source = %src.source_id, error = %e, "persist health failed");
            }
            delay = poll;
        }
        Ok(FetchOutcome::Body { bytes, etag, last_modified }) => {
            let hash = content_hash(&bytes);
            let fetched = now_ms();
            h.last_http_success_at_ms = Some(fetched);
            if h.last_content_hash.as_deref() == Some(hash.as_str()) {
                // Content unchanged (server ignored conditional headers).
                h.consecutive_failures = 0;
                h.etag = etag;
                h.last_modified = last_modified;
                h.freshness_state = Some(compute_freshness(&h, src.expected_freshness_seconds, fetched));
                let _ = rt.writer.health(&src.source_id, h, false).await;
                return poll;
            }
            let parsed = match src.format {
                SourceFormat::Cap => super::cap::parse(&src.source_id, &src.feed_url, &bytes, fetched)
                    .map(|b| (b.messages, b.rejected))
                    .map_err(|e| e.to_string()),
                SourceFormat::GeojsonUsgs => super::usgs::parse(&src.source_id, &src.feed_url, &bytes, fetched)
                    .map(|b| (b.messages, b.rejected))
                    .map_err(|e| e.to_string()),
                SourceFormat::GeojsonNws => super::nws::parse(&src.source_id, &src.feed_url, &bytes, fetched)
                    .map(|b| (b.messages, b.rejected))
                    .map_err(|e| e.to_string()),
            };
            match parsed {
                Ok((messages, rejected)) => {
                    h.consecutive_failures = 0;
                    h.last_error_code = None;
                    h.last_error_summary = rejected.first().map(|r| format!("{} record(s) rejected, first: {r}", rejected.len()));
                    h.rejected_records_last_fetch = rejected.len() as u32;
                    h.last_content_change_at_ms = Some(fetched);
                    if !messages.is_empty() {
                        h.last_valid_record_at_ms = Some(fetched);
                    }
                    h.last_content_hash = Some(hash);
                    // Checkpoint advances in the same transaction as the records (spec §40.3).
                    h.etag = etag;
                    h.last_modified = last_modified;
                    h.freshness_state = Some(compute_freshness(&h, src.expected_freshness_seconds, fetched));
                    match rt.writer.ingest(&src.source_id, &src.agency_name, messages, h).await {
                        Ok(c) => {
                            tracing::info!(source = %src.source_id, changed = c.changed.len(), events = c.events.len(), skipped = c.skipped, "ingested")
                        }
                        Err(e) => tracing::error!(source = %src.source_id, error = %e, "commit failed"),
                    }
                }
                Err(e) => {
                    // Keep last valid records; do not advance the checkpoint.
                    h.consecutive_failures += 1;
                    h.last_error_code = Some("parse".into());
                    h.last_error_summary = Some(e.chars().take(300).collect());
                    h.freshness_state = Some(compute_freshness(&h, src.expected_freshness_seconds, fetched));
                    tracing::warn!(source = %src.source_id, error = %e, "parse failed");
                    let _ = rt.writer.health(&src.source_id, h, false).await;
                }
            }
            delay = poll;
        }
        Err(e) => {
            h.consecutive_failures += 1;
            h.last_error_code = Some(e.code());
            h.last_error_summary = Some(e.to_string().chars().take(300).collect());
            h.freshness_state = Some(compute_freshness(&h, src.expected_freshness_seconds, now_ms()));
            tracing::warn!(source = %src.source_id, code = %e.code(), failures = h.consecutive_failures, "fetch failed");
            delay = backoff_delay(&e, h.consecutive_failures, src.poll_interval_seconds);
            let _ = rt.writer.health(&src.source_id, h, false).await;
        }
    }
    delay
}

pub fn spawn_sources(rt: IngestRuntime, sources: Vec<SourceConfig>, shutdown: watch::Receiver<bool>) {
    for src in sources.into_iter().filter(|s| s.enabled) {
        let rt = rt.clone();
        let mut stop = shutdown.clone();
        tokio::spawn(async move {
            // Randomized start avoids synchronized polling (spec §38.1).
            let mut delay = Duration::from_millis(jitter_ms(src.poll_interval_seconds.min(30) * 1000));
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = stop.changed() => break,
                }
                delay = poll_once(&rt, &src).await;
            }
            tracing::info!(source = %src.source_id, "source worker stopped");
        });
    }
}

/// Recomputes freshness and publishes `source.status` when it changes.
pub fn spawn_health_worker(rt: IngestRuntime, sources: Vec<SourceConfig>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = shutdown.changed() => break,
            }
            for s in sources.iter().filter(|s| s.enabled) {
                let h = current_health(&rt, &s.source_id);
                let f = compute_freshness(&h, s.expected_freshness_seconds, now_ms());
                if h.freshness_state != Some(f) {
                    let mut n = h;
                    n.freshness_state = Some(f);
                    if let Err(e) = rt.writer.health(&s.source_id, n, true).await {
                        tracing::error!(error = %e, "health persist failed");
                    }
                }
            }
        }
    });
}

/// Applies time-based lifecycle transitions; never erases history.
pub fn spawn_expiry_worker(writer: WriterHandle, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = shutdown.changed() => break,
            }
            match writer.tick(now_ms()).await {
                Ok(c) if !c.changed.is_empty() => tracing::info!(changed = c.changed.len(), "lifecycle transitions"),
                Ok(_) => {}
                Err(e) => tracing::error!(error = %e, "expiry tick failed"),
            }
        }
    });
}
