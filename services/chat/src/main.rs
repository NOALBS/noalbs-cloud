mod app;
mod cache;
mod commands;
mod config;
mod error;
mod events;
mod handler;
mod i18n;
mod inbound;
mod kick;
mod kv;
mod messages;
mod metrics;
mod notify;
mod reconcile;
mod reply;
mod tokens;
mod twitch;
mod youtube;

use common::shutdown::{self, ShutdownManager};
use error::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Logs to stdout, and over OTLP when configured (see the `telemetry` crate).
    let telemetry = telemetry::init("chat");
    tracing::info!("Starting chat service...");

    let config = config::load_config()?;
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
