use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Preferences {
    pub switcher_id: Uuid,
    pub preferences: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

pub async fn get(pool: &PgPool, user_id: Uuid, switcher_id: Uuid) -> Result<Option<Preferences>> {
    Ok(sqlx::query_as("SELECT switcher_id, preferences, updated_at FROM switcher_preferences WHERE user_id = $1 AND switcher_id = $2")
        .bind(user_id).bind(switcher_id).fetch_optional(pool).await?)
}

pub async fn put(
    pool: &PgPool,
    user_id: Uuid,
    switcher_id: Uuid,
    preferences: &serde_json::Value,
) -> Result<Preferences> {
    Ok(sqlx::query_as("INSERT INTO switcher_preferences (user_id, switcher_id, preferences) VALUES ($1, $2, $3) ON CONFLICT (user_id, switcher_id) DO UPDATE SET preferences = EXCLUDED.preferences, updated_at = now() RETURNING switcher_id, preferences, updated_at")
        .bind(user_id).bind(switcher_id).bind(preferences).fetch_one(pool).await?)
}

pub async fn for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Preferences>> {
    Ok(sqlx::query_as("SELECT switcher_id, preferences, updated_at FROM switcher_preferences WHERE user_id = $1 ORDER BY switcher_id")
        .bind(user_id).fetch_all(pool).await?)
}
