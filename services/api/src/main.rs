mod app;
mod auth_cache;
mod billing;
mod config;
mod error;
mod metrics;
mod oauth;
mod permissions;
mod privacy;
mod rate_limit;
mod request_spans;
mod routes;
mod screenshots;
mod server_details;
mod stats_probe;
mod stripe;
mod validate;

use common::shutdown::{self, ShutdownManager};
use error::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // `api --openapi` prints the OpenAPI document and exits, without needing a
    // database or config. The dashboard generates its API types from it.
    if std::env::args().any(|arg| arg == "--openapi") {
        println!("{}", app::openapi().to_pretty_json()?);
        return Ok(());
    }

    // Logs to stdout, and over OTLP when configured (see the `telemetry` crate).
    let telemetry = telemetry::init("api");
    tracing::info!("Starting API service...");

    // Testing a wss:// stats URL and reqwest both use rustls; pick its provider once.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let config = config::load_config()?;
    tracing::info!(host = %config.host, port = config.port, "Config loaded");

    let shutdown_manager = ShutdownManager::new();

    let shutdown_handle = shutdown_manager.handle();
    let app_handle = tokio::spawn(app::run(config, shutdown_handle));

    // An application error (NATS or the database unreachable at startup) ends
    // the process as a failure, so whatever runs us sees it.
    let failed = tokio::select! {
        () = shutdown::stop_requested() => {
            tracing::info!("Shutdown signal received. Exiting...");
            false
        }
        ended = app_handle => {
            match &ended {
                Ok(Ok(())) => tracing::info!("Application task completed. Exiting..."),
                Ok(Err(error)) => tracing::error!(%error, "Application error"),
                Err(error) => tracing::error!(%error, "Application task failed"),
            }
            !matches!(ended, Ok(Ok(())))
        }
    };

    shutdown_manager.shutdown().await;
    telemetry.shutdown();

    if failed {
        std::process::exit(1);
    }
    Ok(())
}
