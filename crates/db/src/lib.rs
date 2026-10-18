use sqlx::{PgPool, Pool, Postgres, migrate::Migrator, postgres::PgPoolOptions};

pub mod admin;
pub mod billing;
pub mod chat;
pub mod chat_commands;
pub mod error;
pub mod invites;
pub mod notices;
pub mod oauth2_connections;
pub mod organization_audit;
pub mod organizations;
pub mod plans;
pub mod prices;
pub mod sessions;
pub mod staff_actions;
pub mod stream_markers;
pub mod stream_servers;
pub mod streams;
pub mod support;
pub mod switcher_events;
pub mod switchers;
pub mod users;
pub mod workspaces;

pub use error::Error;

use crate::error::Result;

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub type DbPool = Pool<Postgres>;

pub struct DbConfig<'a> {
    pub database_url: &'a str,
    pub max_connections: u32,
}

pub async fn create_pool_with_options(config: &DbConfig<'_>) -> Result<DbPool> {
    Ok(PgPoolOptions::new()
        .max_connections(config.max_connections)
        .connect(config.database_url)
        .await?)
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    MIGRATOR.run(pool).await?;
    Ok(())
}
