use db::{DbConfig, create_pool_with_options, users};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a connection pool to the PostgreSQL database
    let pool = create_pool_with_options(&DbConfig {
        database_url: "postgres://postgres:postgres@localhost:5432/localdb",
        max_connections: 1,
    })
    .await?;

    // Create a new user in the database
    let user = users::create(&pool).await?;

    println!("Created user: {}", user.id);
    Ok(())
}
