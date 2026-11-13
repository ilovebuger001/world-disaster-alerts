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
    #[serde(default = "default_gazetteer")]
    pub gazetteer_file: PathBuf,
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
    #[serde(default)]
    pub http: HttpConfig,
    #[serde(default)]
    pub geocoder: GeocoderConfig,
    #[serde(default)]
    pub reference: ReferenceFiles,
}

/// Edge protections applied by the Rust server itself.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    /// Exact origins allowed to call the API cross-origin (e.g. the GitHub
    /// Pages site). Empty = same-origin only. `*` is rejected in production.
    pub cors_allowed_origins: Vec<String>,
    /// Requests per client IP per minute for `/api/*` (0 disables; not allowed in production).
    pub rate_limit_per_minute: u32,
    /// Use the right-most `X-Forwarded-For` hop as the client IP. Enable only
    /// behind a reverse proxy that sets it (Caddy in docker-compose does).
    pub trust_forwarded_for: bool,
    /// Send `Strict-Transport-Security`. Enable when served over HTTPS.
    pub hsts: bool,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self { cors_allowed_origins: Vec::new(), rate_limit_per_minute: 300, trust_forwarded_for: false, hsts: false }
    }
}

/// Address search through a Nominatim-compatible geocoder, called only by the
/// server (usage policy: max 1 request/s, identifying User-Agent, no autocomplete).
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct GeocoderConfig {
    pub enabled: bool,
    pub base_url: String,
    pub min_interval_ms: u64,
    pub cache_entries: usize,
}

impl Default for GeocoderConfig {
    fn default() -> Self {
        Self { enabled: false, base_url: "https://nominatim.openstreetmap.org".into(), min_interval_ms: 1100, cache_entries: 2000 }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ReferenceFiles {
    pub contacts_file: PathBuf,
    pub guides_file: PathBuf,
    pub facilities_file: PathBuf,
}

impl Default for ReferenceFiles {
    fn default() -> Self {
        Self {
            contacts_file: PathBuf::from("reference/emergency_contacts.json"),
            guides_file: PathBuf::from("reference/guides.json"),
            facilities_file: PathBuf::from("reference/facilities.json"),
        }
    }
}

fn default_gazetteer() -> PathBuf {
    PathBuf::from("config/gazetteer.toml")
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
    #[error("reference data: {0}")]
    Reference(#[from] crate::reference::ReferenceError),
}

pub struct Loaded {
    pub app: AppConfig,
    pub sources: Vec<SourceConfig>,
    pub mapping: SeverityMapping,
    pub gazetteer: crate::api::search::Gazetteer,
    pub reference: crate::reference::Reference,
}

fn read_toml<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, ConfigError> {
    let s = std::fs::read_to_string(p).map_err(|e| ConfigError::Read(p.to_path_buf(), e))?;
    toml::from_str(&s).map_err(|e| ConfigError::Toml(p.to_path_buf(), e))
}

pub fn load(path: &Path) -> Result<Loaded, ConfigError> {
    let mut app: AppConfig = read_toml(path)?;
    apply_env(&mut app);
    let base = path.parent().and_then(Path::parent).unwrap_or(Path::new("."));
    let resolve = |p: &PathBuf| if p.is_absolute() { p.clone() } else { base.join(p) };
    let sources: SourcesFile = read_toml(&resolve(&app.sources_file))?;
    let mapping: SeverityMapping = read_toml(&resolve(&app.severity_mapping_file))?;
    let gazetteer: crate::api::search::Gazetteer = read_toml(&resolve(&app.gazetteer_file))?;
    app.data_dir = resolve(&app.data_dir);
    app.frontend_dir = resolve(&app.frontend_dir);
    let r = &app.reference;
    let reference = crate::reference::Reference::load(&resolve(&r.contacts_file), &resolve(&r.guides_file), &resolve(&r.facilities_file))?;
    validate(&app, &sources.sources, &mapping)?;
    Ok(Loaded { app, sources: sources.sources, mapping, gazetteer, reference })
}

/// Environment overrides for deployment (no secrets are read here; none are needed).
pub fn apply_env(app: &mut AppConfig) {
    if let Ok(b) = std::env::var("WDA_BIND") {
        app.bind = b;
    }
    if let Ok(o) = std::env::var("WDA_CORS_ORIGINS") {
        app.http.cors_allowed_origins =
            o.split(',').map(|s| s.trim().trim_end_matches('/').to_string()).filter(|s| !s.is_empty()).collect();
    }
    if let Ok(v) = std::env::var("WDA_TRUST_FORWARDED_FOR") {
        app.http.trust_forwarded_for = v == "1" || v.eq_ignore_ascii_case("true");
    }
    if let Ok(v) = std::env::var("WDA_HSTS") {
        app.http.hsts = v == "1" || v.eq_ignore_ascii_case("true");
    }
    if let Ok(v) = std::env::var("WDA_GEOCODER_ENABLED") {
        app.geocoder.enabled = v == "1" || v.eq_ignore_ascii_case("true");
    }
}

pub fn validate(app: &AppConfig, sources: &[SourceConfig], mapping: &SeverityMapping) -> Result<(), ConfigError> {
    mapping.validate().map_err(|e| ConfigError::Invalid(e.to_string()))?;
    if app.environment == Environment::Production && app.allow_private_destinations {
        return Err(ConfigError::Invalid("allow_private_destinations must be false in production".into()));
    }
    if app.bind.parse::<std::net::SocketAddr>().is_err() {
        return Err(ConfigError::Invalid(format!("bind address '{}' is not host:port", app.bind)));
    }
    for o in &app.http.cors_allowed_origins {
        let ok = o == "*" || ((o.starts_with("https://") || o.starts_with("http://")) && !o.ends_with('/') && o.matches('/').count() == 2);
        if !ok {
            return Err(ConfigError::Invalid(format!("CORS origin '{o}' must be scheme://host[:port] without a path")));
        }
    }
    if app.environment == Environment::Production {
        if app.http.cors_allowed_origins.iter().any(|o| o == "*" || o.starts_with("http://")) {
            return Err(ConfigError::Invalid("production CORS origins must be explicit https:// origins".into()));
        }
        if app.http.rate_limit_per_minute == 0 {
            return Err(ConfigError::Invalid("rate limiting must be enabled in production".into()));
        }
    }
    if app.geocoder.enabled && !app.geocoder.base_url.starts_with("https://") {
        return Err(ConfigError::Invalid("geocoder.base_url must be https".into()));
    }
    if app.geocoder.min_interval_ms < 1000 && app.geocoder.base_url.contains("nominatim.openstreetmap.org") {
        return Err(ConfigError::Invalid("the public Nominatim service allows at most 1 request per second".into()));
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
