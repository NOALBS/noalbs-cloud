use db::{create_pool_with_options, run_migrations};

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let config = db::DbConfig {
        database_url: &database_url,
        max_connections: 5,
    };
    let pool = create_pool_with_options(&config)
        .await
        .expect("Failed to connect to database");
    run_migrations(&pool).await.expect("Migration failed");
    println!("Migrations applied successfully.");
}
