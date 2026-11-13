//! Router and startup wiring (spec §2.1: one binary, one embedded database,
//! frontend served by Axum as static assets).

use crate::api;
use crate::config::HttpConfig;
use crate::config::Loaded;
use crate::geo::spatial_index::AlertIndex;
use crate::http::Edge;
use crate::ingest::http_client::build_client;
use crate::ingest::scheduler::{spawn_expiry_worker, spawn_health_worker, spawn_sources, IngestRuntime};
use crate::realtime::event::ApiEvent;
use crate::state::{ApiConfig, AppState, SseLimits};
use crate::storage::redb::{spawn_writer, Db, WriterConfig, WriterHandle};
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use axum::Router;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, watch, Semaphore};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

pub fn router(state: AppState, frontend_dir: Option<std::path::PathBuf>) -> Router {
    router_with(state, frontend_dir, HttpConfig::default())
}

pub fn router_with(state: AppState, frontend_dir: Option<std::path::PathBuf>, http: HttpConfig) -> Router {
    let metrics = state.metrics.clone();
    let api = Router::new()
        .route("/health/live", get(api::health::live))
        .route("/health/ready", get(api::health::ready))
        .route("/api/v1/meta", get(api::sources::meta))
        .route("/api/v1/sources", get(api::sources::sources))
        .route("/api/v1/alerts", get(api::alerts::viewport))
        .route("/api/v1/alerts/:alert_id", get(api::alerts::detail))
        .route("/api/v1/search", get(api::search::search))
        .route("/api/v1/stream", get(api::stream::stream))
        .route("/api/v1/alerts/:alert_id/facilities", get(api::reference::facilities))
        .route("/api/v1/contacts", get(api::reference::contacts))
        .route("/api/v1/contacts/:country", get(api::reference::contact))
        .route("/api/v1/guides", get(api::reference::guides))
        .route("/api/v1/guides/:hazard", get(api::reference::guide))
        .route("/api/v1/geocode", get(api::geocode::geocode))
        .route("/api/v1/geocode/reverse", get(api::geocode::reverse))
        .route("/api/v1/notify/eligible", get(api::notify::eligible_alerts))
        .route("/api/v1/alerts-unmapped", get(api::alerts::unmapped))
        .route("/api/v1/alerts/:alert_id/translate", get(api::translate::translate_alert))
        .route("/api/v1/alerts/:alert_id/related", get(api::notify::related))
        .route("/health/metrics", get(api::health::metrics))
        .with_state(state);
    let app = match frontend_dir {
        Some(d) if d.is_dir() => api.fallback_service(ServeDir::new(d)),
        _ => api,
    };
    let edge = Arc::new(Edge::new(http, metrics));
    app.layer(axum::middleware::from_fn_with_state(edge, crate::http::edge))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(TraceLayer::new_for_http())
}

pub struct Running {
    pub state: AppState,
    pub writer: WriterHandle,
    pub shutdown: watch::Sender<bool>,
    pub writer_thread: std::thread::JoinHandle<()>,
}

/// Opens storage, rebuilds the index from persisted state, starts the writer
/// and workers, then marks the service ready.
pub fn start(loaded: &Loaded, start_ingestion: bool) -> anyhow::Result<Running> {
    let app = &loaded.app;
    let db = Arc::new(Db::open(&app.data_dir.join("wda.redb"))?);
    let started = std::time::Instant::now();
    let records = db.load_alerts()?;
    let seq = db.high_water()?;
    let health = db.load_source_health()?;
    let n = records.len();
    let index = Arc::new(RwLock::new(AlertIndex::rebuild(records, seq, health)));
    tracing::info!(records = n, event_sequence = seq, ms = started.elapsed().as_millis() as u64, "index rebuilt from redb");

    let (bus, _) = broadcast::channel::<ApiEvent>(1024);
    let mapping = Arc::new(loaded.mapping.clone());
    let (writer, writer_thread) = spawn_writer(
        db.clone(),
        mapping,
        index.clone(),
        bus.clone(),
        WriterConfig { queue: app.writer_queue, event_log_retention: app.event_log_retention },
    );
    let (shutdown, shutdown_rx) = watch::channel(false);
    let ready = Arc::new(AtomicBool::new(false));
    let state = AppState {
        index: index.clone(),
        db,
        bus,
        sources: Arc::new(loaded.sources.clone()),
        ready: ready.clone(),
        sse: Arc::new(SseLimits {
            total: Arc::new(Semaphore::new(app.sse_max_connections)),
            per_ip: Mutex::new(HashMap::new()),
            max_per_ip: app.sse_max_per_ip,
        }),
        api: Arc::new(ApiConfig {
            environment: app.environment,
            max_replay_events: app.max_replay_events,
            client_queue: app.sse_client_queue,
            heartbeat_seconds: app.sse_heartbeat_seconds,
        }),
        gazetteer: Arc::new(loaded.gazetteer.clone()),
        reference: Arc::new(loaded.reference.clone()),
        translator: if app.translation.enabled {
            Some(Arc::new(crate::api::translate::Translator::new(&app.translation, &app.user_agent)?))
        } else {
            None
        },
        metrics: Arc::new(crate::http::Metrics::default()),
        geocoder: if app.geocoder.enabled {
            Some(Arc::new(crate::api::geocode::Geocoder::new(&app.geocoder, &app.user_agent)?))
        } else {
            None
        },
    };
    if start_ingestion {
        let client = build_client(&app.user_agent, &loaded.sources, app.allow_private_destinations)?;
        let rt = IngestRuntime {
            client,
            writer: writer.clone(),
            index,
            in_flight: Arc::new(Semaphore::new(app.max_in_flight_fetches)),
            allow_private: app.allow_private_destinations,
        };
        spawn_sources(rt.clone(), loaded.sources.clone(), shutdown_rx.clone());
        spawn_health_worker(rt, loaded.sources.clone(), shutdown_rx.clone());
        spawn_expiry_worker(writer.clone(), shutdown_rx);
    }
    ready.store(true, Ordering::Release);
    Ok(Running { state, writer, shutdown, writer_thread })
}
