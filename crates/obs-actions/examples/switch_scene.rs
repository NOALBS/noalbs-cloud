//! Example: Switch scene on OBS using the obs-actions crate.
//!
//! Usage:
//!   cargo run --example switch_scene

use obs_actions::{ObsClient, error::Result};

#[tokio::main]
async fn main() -> Result<()> {
    let client = ObsClient::new_with_default_config().await?;

    println!("Switching scene...");

    client.switch_scene("Scene 59").await?;

    println!("✓ Scene switched successfully!");

    Ok(())
}
