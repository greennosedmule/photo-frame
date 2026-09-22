mod api;
mod app;
mod assets;
mod auth;
mod config;
mod media;
mod problem;
#[cfg(test)]
mod tests;
mod upload;

use std::sync::Arc;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Invalid configuration is a startup failure, never a silent default.
    let config =
        config::Config::from_env().map_err(|e| anyhow::anyhow!("invalid configuration: {e}"))?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(&config.log_level).context("invalid LOG_LEVEL")?)
        .init();

    let store = photoframe_store::connect(&config.database_url).await?;
    store.migrate().await?;

    // Either method grants management access.
    let mut providers: Vec<Box<dyn auth::AuthProvider>> = Vec::new();
    if let Some(password) = &config.admin_password {
        providers.push(Box::new(auth::BasicAuth::new(
            config.admin_username.clone(),
            password.clone(),
        )));
    }
    if !config.admin_allowed_cidrs.is_empty() {
        providers.push(Box::new(auth::CidrAuth::new(
            config.admin_allowed_cidrs.clone(),
            config.trusted_proxy_cidrs.clone(),
        )));
    }
    let auth = Arc::new(auth::AnyOf(providers));
    let router = app::router(app::AppState {
        store,
        auth,
        layout: photoframe_store::Layout::new(config.library_root.clone()),
        config: Arc::new(config.clone()),
    });

    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %config.bind_addr, "photoframe-web listening");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// Drain connections on SIGTERM (Kubernetes) or Ctrl-C (local dev).
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    tracing::info!("shutting down");
}
