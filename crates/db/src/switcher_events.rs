//! The activity feed: what happened on a switcher and who did it.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SwitcherEventRow {
    pub id: Uuid,
    pub switcher_id: Uuid,
    pub kind: String,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[tracing::instrument(skip(pool, data))]
pub async fn insert(
    pool: &PgPool,
    switcher_id: Uuid,
    kind: &str,
    data: serde_json::Value,
) -> Result<SwitcherEventRow> {
    let event = sqlx::query_as::<_, SwitcherEventRow>(
        "
        INSERT INTO switcher_events (switcher_id, kind, data)
        VALUES ($1, $2, $3)
        RETURNING id, switcher_id, kind, data, created_at
        ",
    )
    .bind(switcher_id)
    .bind(kind)
    .bind(data)
    .fetch_one(pool)
    .await?;

    Ok(event)
}

/// The newest events first; `before` pages further back.
#[tracing::instrument(skip(pool))]
pub async fn list(
    pool: &PgPool,
    switcher_id: Uuid,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<SwitcherEventRow>> {
    let events = sqlx::query_as::<_, SwitcherEventRow>(
        "
        SELECT id, switcher_id, kind, data, created_at
        FROM switcher_events
        WHERE switcher_id = $1 AND ($2::timestamptz IS NULL OR created_at < $2)
        ORDER BY created_at DESC
        LIMIT $3
        ",
    )
    .bind(switcher_id)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(events)
}

/// Events in `[from, to]`, oldest first.
#[tracing::instrument(skip(pool))]
pub async fn between(
    pool: &PgPool,
    switcher_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<SwitcherEventRow>> {
    let events = sqlx::query_as::<_, SwitcherEventRow>(
        "
        SELECT id, switcher_id, kind, data, created_at
        FROM switcher_events
        WHERE switcher_id = $1 AND created_at >= $2 AND created_at <= $3
        ORDER BY created_at
        LIMIT $4
        ",
    )
    .bind(switcher_id)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(events)
}

/// Always keep this many days of activity, even on plans with less history.
pub const MIN_RETENTION_DAYS: i32 = 30;

/// Deletes activity older than the workspace plan's history (at least
/// [`MIN_RETENTION_DAYS`]), plus a day so a stream's events outlive it.
#[tracing::instrument(skip(pool))]
pub async fn delete_expired(pool: &PgPool) -> Result<u64> {
    let deleted = sqlx::query(
        "DELETE FROM switcher_events
         USING switchers, workspaces, workspace_plans plans
         WHERE switchers.id = switcher_events.switcher_id
           AND workspaces.id = switchers.workspace_id
           AND plans.workspace_id = workspaces.id
           AND switcher_events.created_at
               < now() - make_interval(days => GREATEST(plans.history_days, $1) + 1)",
    )
    .bind(MIN_RETENTION_DAYS)
    .execute(pool)
    .await?;
    Ok(deleted.rows_affected())
}
