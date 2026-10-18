mod app;
mod cache;
mod commands;
mod config;
mod error;
mod events;
mod handler;
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
    let app_handle = tokio::spawn(async move {
        if let Err(error) = app::run(config, shutdown_handle).await {
            tracing::error!(%error, "Application error");
        }
    });

    tokio::select! {
        () = shutdown::stop_requested() => {
            tracing::info!("Shutdown signal received. Exiting...");
        }
        _ = app_handle => {
            tracing::info!("Application task completed. Exiting...");
        }
    }

    shutdown_manager.shutdown().await;
    telemetry.shutdown();

    Ok(())
}
