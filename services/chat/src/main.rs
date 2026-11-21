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

    let failed = shutdown::run_until_stopped(shutdown_manager, app_handle).await;
    telemetry.shutdown_with_timeout(std::time::Duration::from_secs(5));

    if failed {
        std::process::exit(1);
    }
    Ok(())
}
