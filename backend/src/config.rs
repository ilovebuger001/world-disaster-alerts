//! Application configuration. Validated before any network port is bound (spec §31).

use crate::domain::severity::SeverityMapping;
use crate::ingest::source_registry::{SourceConfig, SourcesFile};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Development,
    Production,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AppConfig {
    pub environment: Environment,
    pub bind: String,
    pub data_dir: PathBuf,
    pub sources_file: PathBuf,
    pub severity_mapping_file: PathBuf,
    #[serde(default = "default_frontend")]
    pub frontend_dir: PathBuf,
    pub user_agent: String,
    #[serde(default = "d4")]
    pub max_in_flight_fetches: usize,
    #[serde(default = "d_retention")]
    pub event_log_retention: u64,
    #[serde(default = "d_replay")]
    pub max_replay_events: u64,
    #[serde(default = "d_sse_total")]
    pub sse_max_connections: usize,
    #[serde(default = "d_sse_ip")]
    pub sse_max_per_ip: usize,
    #[serde(default = "d_queue")]
    pub sse_client_queue: usize,
    #[serde(default = "d_heartbeat")]
    pub sse_heartbeat_seconds: u64,
    #[serde(default = "d_writer_queue")]
    pub writer_queue: usize,
    /// Only for local tests/mocks. Never enable in production (SSRF, spec §4.3).
    #[serde(default)]
    pub allow_private_destinations: bool,
}

fn default_frontend() -> PathBuf {
    PathBuf::from("frontend")
}
fn d4() -> usize {
    4
}
fn d_retention() -> u64 {
    100_000
}
fn d_replay() -> u64 {
    1_000
}
fn d_sse_total() -> usize {
    1_000
}
fn d_sse_ip() -> usize {
    8
}
fn d_queue() -> usize {
    64
}
fn d_heartbeat() -> u64 {
    20
}
fn d_writer_queue() -> usize {
    256
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {0}: {1}")]
    Read(PathBuf, std::io::Error),
    #[error("invalid TOML in {0}: {1}")]
    Toml(PathBuf, toml::de::Error),
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

pub struct Loaded {
    pub app: AppConfig,
    pub sources: Vec<SourceConfig>,
    pub mapping: SeverityMapping,
}

fn read_toml<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, ConfigError> {
    let s = std::fs::read_to_string(p).map_err(|e| ConfigError::Read(p.to_path_buf(), e))?;
    toml::from_str(&s).map_err(|e| ConfigError::Toml(p.to_path_buf(), e))
}

pub fn load(path: &Path) -> Result<Loaded, ConfigError> {
    let mut app: AppConfig = read_toml(path)?;
    if let Ok(b) = std::env::var("WDA_BIND") {
        app.bind = b;
    }
    let base = path.parent().and_then(Path::parent).unwrap_or(Path::new("."));
    let resolve = |p: &PathBuf| if p.is_absolute() { p.clone() } else { base.join(p) };
    let sources: SourcesFile = read_toml(&resolve(&app.sources_file))?;
    let mapping: SeverityMapping = read_toml(&resolve(&app.severity_mapping_file))?;
    app.data_dir = resolve(&app.data_dir);
    app.frontend_dir = resolve(&app.frontend_dir);
    validate(&app, &sources.sources, &mapping)?;
    Ok(Loaded { app, sources: sources.sources, mapping })
}

pub fn validate(app: &AppConfig, sources: &[SourceConfig], mapping: &SeverityMapping) -> Result<(), ConfigError> {
    mapping.validate().map_err(|e| ConfigError::Invalid(e.to_string()))?;
    if app.environment == Environment::Production && app.allow_private_destinations {
        return Err(ConfigError::Invalid("allow_private_destinations must be false in production".into()));
    }
    if app.bind.parse::<std::net::SocketAddr>().is_err() {
        return Err(ConfigError::Invalid(format!("bind address '{}' is not host:port", app.bind)));
    }
    let mut seen = std::collections::HashSet::new();
    for s in sources {
        if !seen.insert(s.source_id.clone()) {
            return Err(ConfigError::Invalid(format!("duplicate source_id {}", s.source_id)));
        }
        s.validate(app.environment, app.allow_private_destinations)
            .map_err(|e| ConfigError::Invalid(format!("source {}: {e}", s.source_id)))?;
    }
    Ok(())
}
