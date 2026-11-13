//! Shared application state.

use crate::config::Environment;
use crate::geo::spatial_index::AlertIndex;
use crate::ingest::source_registry::SourceConfig;
use crate::realtime::event::ApiEvent;
use crate::storage::redb::Db;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, Semaphore};

pub struct SseLimits {
    pub total: Arc<Semaphore>,
    pub per_ip: Mutex<HashMap<IpAddr, usize>>,
    pub max_per_ip: usize,
}

pub struct ApiConfig {
    pub environment: Environment,
    pub max_replay_events: u64,
    pub client_queue: usize,
    pub heartbeat_seconds: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub index: Arc<RwLock<AlertIndex>>,
    pub db: Arc<Db>,
    pub bus: broadcast::Sender<ApiEvent>,
    pub sources: Arc<Vec<SourceConfig>>,
    /// Database opened, index built, core workers initialized.
    pub ready: Arc<AtomicBool>,
    pub sse: Arc<SseLimits>,
    pub api: Arc<ApiConfig>,
    pub gazetteer: Arc<crate::api::search::Gazetteer>,
    pub reference: Arc<crate::reference::Reference>,
    /// `None` when address search is disabled in configuration.
    pub geocoder: Option<Arc<crate::api::geocode::Geocoder>>,
    /// `None` when machine translation is disabled.
    pub translator: Option<Arc<crate::api::translate::Translator>>,
    pub metrics: Arc<crate::http::Metrics>,
}
