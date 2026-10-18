//! Example: List the OBS scenes using the obs-actions crate.
//!
//! Usage:
//!   cargo run --example scene_list

use obs_actions::{ObsClient, error::Result};

#[tokio::main]
async fn main() -> Result<()> {
    let client = ObsClient::new_with_default_config().await?;

    println!("Fetching scene list...");

    let response = client.scene_list().await?;

    println!("✓ Scene list retrieved successfully!");
    println!("  Message: {:#?}", response);

    Ok(())
}
