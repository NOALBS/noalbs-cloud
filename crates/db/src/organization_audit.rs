//! An organization's audit log: what its team changed (and what its customers did
//! in their clients), plus what NOALBS staff did to the organization.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

/// Who acted, for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    /// The organization's team.
    Team,
    /// A customer, in their client workspace.
    Customer,
}

impl ActorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Customer => "customer",
        }
    }
}

/// An entry to record.
#[derive(Debug, Clone)]
pub struct NewEntry<'a> {
    pub organization_id: Uuid,
    pub actor_user_id: Uuid,
    /// As they were called then.
    pub actor_name: &'a str,
    pub actor_kind: ActorKind,
    /// A stable snake_case code, e.g. `client_deleted`.
    pub action: &'a str,
    pub subject: Option<&'a str>,
    pub details: serde_json::Value,
}

#[tracing::instrument(skip(pool, entry), fields(action = entry.action))]
pub async fn record(pool: &PgPool, entry: &NewEntry<'_>) -> Result<()> {
    let details = if entry.details.is_object() {
        entry.details.clone()
    } else {
        serde_json::json!({})
    };
    sqlx::query(
        "INSERT INTO organization_audit
             (organization_id, actor_user_id, actor_name, actor_kind, action, subject, details)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(entry.organization_id)
    .bind(entry.actor_user_id)
    .bind(entry.actor_name)
    .bind(entry.actor_kind.as_str())
    .bind(entry.action)
    .bind(entry.subject)
    .bind(details)
    .execute(pool)
    .await?;

    Ok(())
}

/// One line of the log.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Entry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub actor_name: String,
    /// `team`, `customer`, or `staff` for NOALBS staff.
    pub actor_kind: String,
    pub action: String,
    pub subject: Option<String>,
    pub details: serde_json::Value,
}

/// The newest entries first (`before` pages back), the organization's own and what
/// staff did to it. Staff are named only as NOALBS.
#[tracing::instrument(skip(pool))]
pub async fn list(
    pool: &PgPool,
    organization_id: Uuid,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<Entry>> {
    let entries = sqlx::query_as::<_, Entry>(
        "
        SELECT id, created_at, actor_name, actor_kind, action, subject, details
        FROM organization_audit
        WHERE organization_id = $1 AND ($2::timestamptz IS NULL OR created_at < $2)
        UNION ALL
        SELECT id, created_at, 'NOALBS', 'staff', action, workspace_name,
               CASE WHEN reason IS NULL THEN details ELSE details || jsonb_build_object('reason', reason) END
        FROM staff_actions
        WHERE details->>'organization_id' = $1::text
          AND ($2::timestamptz IS NULL OR created_at < $2)
        ORDER BY created_at DESC
        LIMIT $3
        ",
    )
    .bind(organization_id)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(entries)
}

/// How long entries are kept, like the staff log.
pub const RETENTION_DAYS: i32 = crate::support::RETENTION_DAYS;

#[tracing::instrument(skip(pool))]
pub async fn delete_expired(pool: &PgPool) -> Result<u64> {
    let deleted = sqlx::query(
        "DELETE FROM organization_audit WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(RETENTION_DAYS)
    .execute(pool)
    .await?;
    Ok(deleted.rows_affected())
}
