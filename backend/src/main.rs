use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    wda::observability::logging::init();
    let path = PathBuf::from(std::env::var("WDA_CONFIG").unwrap_or_else(|_| "config/app.toml".into()));
    // Configuration is validated before any port is bound.
    let loaded = wda::config::load(&path)?;
    if loaded.app.environment == wda::config::Environment::Development {
        tracing::warn!("development environment: sources pending legal review may be enabled; not for public redistribution");
    }
    let r = loaded.reference.coverage_counts();
    tracing::info!(contacts = ?r, guides = loaded.reference.guides.guides.len(), facilities = loaded.reference.facilities.facilities.len(), "reference data loaded");
    let running = wda::app::start(&loaded, true)?;
    let app = wda::app::router_with(running.state.clone(), Some(loaded.app.frontend_dir.clone()), loaded.app.http.clone());
    let listener = tokio::net::TcpListener::bind(&loaded.app.bind).await?;
    tracing::info!(bind = %loaded.app.bind, "listening");
    let shutdown = running.shutdown.clone();
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutdown requested");
            let _ = shutdown.send(true);
        })
        .await?;
    // Drain the writer: dropping the last handles closes its queue.
    drop(running.writer);
    drop(running.state);
    let _ = running.writer_thread.join();
    Ok(())
}
