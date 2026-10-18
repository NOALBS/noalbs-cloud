//! Notices: messages staff show to every signed-in user.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Notice {
    pub id: Uuid,
    pub message: String,
    /// `info`, `warning` or `critical`.
    pub severity: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub dismissible: bool,
    pub created_by_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

macro_rules! notice_columns {
    () => {
        "id, message, severity, starts_at, ends_at, dismissible, created_by_name, created_at"
    };
}

/// What staff set on a notice.
#[derive(Debug, Clone)]
pub struct NoticeFields<'a> {
    pub message: &'a str,
    pub severity: &'a str,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub dismissible: bool,
}

/// The notices in force now, oldest first.
#[tracing::instrument(skip(pool))]
pub async fn active(pool: &PgPool) -> Result<Vec<Notice>> {
    let notices = sqlx::query_as::<_, Notice>(concat!(
        "SELECT ",
        notice_columns!(),
        " FROM notices
          WHERE starts_at <= now() AND (ends_at IS NULL OR ends_at > now())
          ORDER BY starts_at, created_at"
    ))
    .fetch_all(pool)
    .await?;
    Ok(notices)
}

/// Every notice, newest first (staff).
#[tracing::instrument(skip(pool))]
pub async fn list(pool: &PgPool, limit: i64) -> Result<Vec<Notice>> {
    let notices = sqlx::query_as::<_, Notice>(concat!(
        "SELECT ",
        notice_columns!(),
        " FROM notices ORDER BY starts_at DESC, created_at DESC LIMIT $1"
    ))
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(notices)
}

#[tracing::instrument(skip(pool, fields))]
pub async fn create(
    pool: &PgPool,
    fields: &NoticeFields<'_>,
    created_by: Uuid,
    created_by_name: &str,
) -> Result<Notice> {
    let notice = sqlx::query_as::<_, Notice>(concat!(
        "INSERT INTO notices
             (message, severity, starts_at, ends_at, dismissible, created_by, created_by_name)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING ",
        notice_columns!()
    ))
    .bind(fields.message)
    .bind(fields.severity)
    .bind(fields.starts_at)
    .bind(fields.ends_at)
    .bind(fields.dismissible)
    .bind(created_by)
    .bind(created_by_name)
    .fetch_one(pool)
    .await?;
    Ok(notice)
}

/// `None` when the notice doesn't exist.
#[tracing::instrument(skip(pool, fields))]
pub async fn update(pool: &PgPool, id: Uuid, fields: &NoticeFields<'_>) -> Result<Option<Notice>> {
    let notice = sqlx::query_as::<_, Notice>(concat!(
        "UPDATE notices
         SET message = $2, severity = $3, starts_at = $4, ends_at = $5, dismissible = $6
         WHERE id = $1 RETURNING ",
        notice_columns!()
    ))
    .bind(id)
    .bind(fields.message)
    .bind(fields.severity)
    .bind(fields.starts_at)
    .bind(fields.ends_at)
    .bind(fields.dismissible)
    .fetch_optional(pool)
    .await?;
    Ok(notice)
}

/// Ends a notice that is showing now. `None` when it doesn't exist, hasn't
/// started (the caller deletes those: it would end before it starts) or has
/// already ended.
#[tracing::instrument(skip(pool))]
pub async fn end_now(pool: &PgPool, id: Uuid) -> Result<Option<Notice>> {
    let notice = sqlx::query_as::<_, Notice>(concat!(
        "UPDATE notices SET ends_at = now()
         WHERE id = $1 AND starts_at < now() AND (ends_at IS NULL OR ends_at > now())
         RETURNING ",
        notice_columns!()
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(notice)
}

#[tracing::instrument(skip(pool))]
pub async fn get(pool: &PgPool, id: Uuid) -> Result<Option<Notice>> {
    let notice = sqlx::query_as::<_, Notice>(concat!(
        "SELECT ",
        notice_columns!(),
        " FROM notices WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(notice)
}

/// Whether it existed.
#[tracing::instrument(skip(pool))]
pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool> {
    let result = sqlx::query("DELETE FROM notices WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
