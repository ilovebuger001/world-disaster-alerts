use tracing_subscriber::EnvFilter;

/// Structured logs. Never log precise user locations, credentials or secrets.
pub fn init() {
    let filter = EnvFilter::try_from_env("WDA_LOG").unwrap_or_else(|_| EnvFilter::new("info,tower_http=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_target(false).try_init();
}
