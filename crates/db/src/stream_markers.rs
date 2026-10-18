//! Markers: moments flagged during a stream, listed on its history page.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Marker {
    pub id: Uuid,
    pub stream_id: Uuid,
    pub at: DateTime<Utc>,
    pub description: Option<String>,
    /// Who made it, as the activity feed says it ("Name (Twitch chat)").
    pub created_by: Option<String>,
}

macro_rules! marker_columns {
    () => {
        "id, stream_id, at, description, created_by"
    };
}

#[tracing::instrument(skip(pool))]
pub async fn insert(
    pool: &PgPool,
    stream_id: Uuid,
    at: DateTime<Utc>,
    description: Option<&str>,
    created_by: Option<&str>,
) -> Result<Marker> {
    let marker = sqlx::query_as::<_, Marker>(concat!(
        "INSERT INTO stream_markers (stream_id, at, description, created_by)
         VALUES ($1, $2, $3, $4) RETURNING ",
        marker_columns!()
    ))
    .bind(stream_id)
    .bind(at)
    .bind(description)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(marker)
}

/// A stream's markers, in the order they were made.
#[tracing::instrument(skip(pool))]
pub async fn list(pool: &PgPool, stream_id: Uuid) -> Result<Vec<Marker>> {
    let markers = sqlx::query_as::<_, Marker>(concat!(
        "SELECT ",
        marker_columns!(),
        " FROM stream_markers WHERE stream_id = $1 ORDER BY at, created_at"
    ))
    .bind(stream_id)
    .fetch_all(pool)
    .await?;
    Ok(markers)
}
