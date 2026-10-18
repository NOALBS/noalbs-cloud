use db::{DbConfig, create_pool_with_options, users};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a connection pool to the PostgreSQL database
    let pool = create_pool_with_options(&DbConfig {
        database_url: "postgres://postgres:postgres@localhost:5432/localdb",
        max_connections: 1,
    })
    .await?;

    let user = users::create(&pool).await?;
    println!("Created user: {}", user.id);

    let workspace = db::workspaces::create(&pool, user.id, "My Workspace", "my-workspace").await?;
    println!("Created workspace: {}", workspace.id);

    let switcher = db::switchers::create(
        &pool,
        workspace.id,
        "715's Switcher",
        b"dev-connect-token-hash",
    )
    .await?;
    println!("Created switcher: {}", switcher.id);

    let stream_server = db::stream_servers::create(
        &pool,
        switcher.id,
        "BELABOX Cloud",
        db::stream_servers::StreamServerKind::Belabox,
    )
    .await?;
    println!("Created stream server: {}", stream_server.id);

    Ok(())
}
