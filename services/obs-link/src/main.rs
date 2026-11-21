mod app;
mod attribution;
mod audio;
mod belabox;
mod config;
mod decide;
mod error;
mod history;
mod metrics;
mod moblin;
mod ownership;
mod status;
mod stream_servers;
mod switcher;

use common::shutdown::{self, ShutdownManager};
use error::Result;

/// `tokio::spawn`, keeping the current span, so what a connection starts logs
/// its `switcher.id` too.
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(tracing::Instrument::in_current_span(future))
}

#[tokio::main]
async fn main() -> Result<()> {
    // Logs to stdout, and over OTLP when configured (see the `telemetry` crate).
    let telemetry = telemetry::init("obs-link");
    tracing::info!("Starting obs-link service...");

    // BELABOX Cloud's WebSocket TLS and reqwest both use rustls; pick its provider once.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let config = config::load_config()?;
    tracing::info!(host = %config.host, port = config.port, "Config loaded");

    let shutdown_manager = ShutdownManager::new();

    let shutdown_handle = shutdown_manager.handle();
    let app_handle = tokio::spawn(app::run(config, shutdown_handle));

    let failed = shutdown::run_until_stopped(shutdown_manager, app_handle).await;
    telemetry.shutdown_with_timeout(std::time::Duration::from_secs(5));

    if failed {
        std::process::exit(1);
    }
    Ok(())
}
