//! Support access: a workspace letting NOALBS staff in for a while
//! (`support_grants`), staff letting themselves in for an emergency
//! (break-glass grants), and what staff did in workspaces they aren't a member
//! of (`support_access_log`), which the workspace can read.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "support_access_level", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    /// Switchers, their settings, live data and history.
    View,
    /// Also switching scenes, starting/stopping and changing settings.
    Control,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SupportGrant {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub level: SupportLevel,
    pub granted_by: Option<Uuid>,
    /// The granter's newest linked account's name.
    pub granted_by_name: Option<String>,
    /// Opened by staff without the workspace's permission.
    pub break_glass: bool,
    /// The staff member who opened a break-glass grant, and their name.
    pub requested_by: Option<Uuid>,
    pub requested_by_name: Option<String>,
    /// Why (break-glass only).
    pub reason: Option<String>,
    /// When an owner or admin marked a break-glass grant as seen.
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A user's display name, as `display_name` in the API picks it.
macro_rules! user_name {
    ($user:literal) => {
        concat!(
            "(SELECT provider_username FROM oauth2_connections WHERE user_id = ",
            $user,
            " ORDER BY updated_at DESC LIMIT 1)"
        )
    };
}

macro_rules! grant_columns {
    () => {
        concat!(
            "support_grants.id, support_grants.workspace_id, support_grants.level, ",
            "support_grants.granted_by, ",
            user_name!("support_grants.granted_by"),
            " AS granted_by_name, ",
            "support_grants.break_glass, support_grants.requested_by, ",
            user_name!("support_grants.requested_by"),
            " AS requested_by_name, ",
            "support_grants.reason, support_grants.acknowledged_at, ",
            "support_grants.expires_at, support_grants.revoked_at, support_grants.created_at"
        )
    };
}

/// Grants in force now, the workspace's own first, then newest first.
#[tracing::instrument(skip(pool))]
pub async fn active_grants(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<SupportGrant>> {
    let grants = sqlx::query_as::<_, SupportGrant>(concat!(
        "SELECT ",
        grant_columns!(),
        " FROM support_grants
         WHERE workspace_id = $1 AND revoked_at IS NULL AND expires_at > now()
         ORDER BY break_glass, created_at DESC"
    ))
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;

    Ok(grants)
}

/// The workspace's own grant in force now, if any (not break-glass).
#[tracing::instrument(skip(pool))]
pub async fn active_grant(pool: &PgPool, workspace_id: Uuid) -> Result<Option<SupportGrant>> {
    Ok(active_grants(pool, workspace_id)
        .await?
        .into_iter()
        .find(|grant| !grant.break_glass))
}

/// Lets support in until `expires_at`, replacing the workspace's grant in force.
#[tracing::instrument(skip(pool))]
pub async fn grant(
    pool: &PgPool,
    workspace_id: Uuid,
    level: SupportLevel,
    granted_by: Uuid,
    expires_at: DateTime<Utc>,
) -> Result<SupportGrant> {
    let mut tx = pool.begin().await?;

    // Two grants at once would otherwise both stay in force.
    sqlx::query("SELECT 1 FROM workspaces WHERE id = $1 FOR UPDATE")
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;
    revoke_active(&mut *tx, workspace_id).await?;
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO support_grants (workspace_id, level, granted_by, expires_at)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
    )
    .bind(workspace_id)
    .bind(level)
    .bind(granted_by)
    .bind(expires_at)
    .fetch_one(&mut *tx)
    .await?;
    let grant = get(&mut *tx, id).await?;

    tx.commit().await?;
    Ok(grant)
}

/// Staff let themselves in without the workspace's permission, to look or to act
/// on switchers (`level`), with a reason. It
/// leaves the workspace's own grant as it is.
#[tracing::instrument(skip(pool, reason))]
pub async fn break_glass(
    pool: &PgPool,
    workspace_id: Uuid,
    requested_by: Uuid,
    level: SupportLevel,
    reason: &str,
    expires_at: DateTime<Utc>,
) -> Result<SupportGrant> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO support_grants
             (workspace_id, level, break_glass, requested_by, reason, expires_at)
         VALUES ($1, $5, true, $2, $3, $4)
         RETURNING id",
    )
    .bind(workspace_id)
    .bind(requested_by)
    .bind(reason)
    .bind(expires_at)
    .bind(level)
    .fetch_one(pool)
    .await?;

    get(pool, id).await
}

async fn get(executor: impl sqlx::PgExecutor<'_>, id: Uuid) -> Result<SupportGrant> {
    let grant = sqlx::query_as::<_, SupportGrant>(concat!(
        "SELECT ",
        grant_columns!(),
        " FROM support_grants WHERE id = $1"
    ))
    .bind(id)
    .fetch_one(executor)
    .await?;

    Ok(grant)
}

/// Break-glass grants owners and admins still have to see: not acknowledged,
/// or from the last `days`. Newest first.
#[tracing::instrument(skip(pool))]
pub async fn break_glass_grants(
    pool: &PgPool,
    workspace_id: Uuid,
    days: i32,
) -> Result<Vec<SupportGrant>> {
    let grants = sqlx::query_as::<_, SupportGrant>(concat!(
        "SELECT ",
        grant_columns!(),
        " FROM support_grants
         WHERE workspace_id = $1 AND break_glass
           AND (acknowledged_at IS NULL OR created_at > now() - make_interval(days => $2))
         ORDER BY created_at DESC"
    ))
    .bind(workspace_id)
    .bind(days)
    .fetch_all(pool)
    .await?;

    Ok(grants)
}

/// Marks every break-glass grant of the workspace as seen.
#[tracing::instrument(skip(pool))]
pub async fn acknowledge_break_glass(pool: &PgPool, workspace_id: Uuid) -> Result<u64> {
    let acknowledged = sqlx::query(
        "UPDATE support_grants SET acknowledged_at = now()
         WHERE workspace_id = $1 AND break_glass AND acknowledged_at IS NULL",
    )
    .bind(workspace_id)
    .execute(pool)
    .await?;

    Ok(acknowledged.rows_affected())
}

/// Ends the workspace's own grant in force; whether there was one. Break-glass
/// grants run their (short) course.
#[tracing::instrument(skip(pool))]
pub async fn revoke(pool: &PgPool, workspace_id: Uuid) -> Result<bool> {
    Ok(revoke_active(pool, workspace_id).await? > 0)
}

async fn revoke_active(executor: impl sqlx::PgExecutor<'_>, workspace_id: Uuid) -> Result<u64> {
    let revoked = sqlx::query(
        "UPDATE support_grants SET revoked_at = now()
         WHERE workspace_id = $1 AND NOT break_glass
           AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(workspace_id)
    .execute(executor)
    .await?;

    Ok(revoked.rows_affected())
}

/// A request staff made in a workspace.
#[derive(Debug, Clone)]
pub struct Access<'a> {
    pub workspace_id: Uuid,
    pub grant_id: Option<Uuid>,
    pub staff_user_id: Uuid,
    pub staff_name: &'a str,
    pub method: &'a str,
    pub route: &'a str,
    pub switcher_id: Option<Uuid>,
}

/// How often the same read (staff, route, switcher, grant) is logged: live
/// pages poll, and each request would bury the rest.
pub const READ_LOG_INTERVAL_MINUTES: i32 = 60;

/// Logs a request; reads (`GET`) at most once per [`READ_LOG_INTERVAL_MINUTES`].
#[tracing::instrument(skip(pool))]
pub async fn log_access(pool: &PgPool, access: &Access<'_>) -> Result<()> {
    sqlx::query(
        "INSERT INTO support_access_log
             (workspace_id, grant_id, staff_user_id, staff_name, method, route, switcher_id)
         SELECT $1, $2, $3, $4, $5, $6, $7
         WHERE $5 <> 'GET' OR NOT EXISTS (
             SELECT 1 FROM support_access_log
             WHERE workspace_id = $1
               AND grant_id IS NOT DISTINCT FROM $2
               AND staff_user_id = $3
               AND method = $5
               AND route = $6
               AND switcher_id IS NOT DISTINCT FROM $7
               AND created_at > now() - make_interval(mins => $8)
         )",
    )
    .bind(access.workspace_id)
    .bind(access.grant_id)
    .bind(access.staff_user_id)
    .bind(access.staff_name)
    .bind(access.method)
    .bind(access.route)
    .bind(access.switcher_id)
    .bind(READ_LOG_INTERVAL_MINUTES)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AccessLogEntry {
    pub id: Uuid,
    pub staff_name: String,
    pub method: String,
    pub route: String,
    pub switcher_id: Option<Uuid>,
    /// The switcher's name now; `None` once it's deleted.
    pub switcher_name: Option<String>,
    /// The level of the grant that allowed it; `None` = without permission.
    pub grant_level: Option<SupportLevel>,
    /// Whether that grant was break-glass (staff's own, not the workspace's).
    pub break_glass: bool,
    /// The level of the workspace's grant in force then, even when it didn't
    /// cover this (before grants were required); `None` = none was given.
    pub grant_in_force: Option<SupportLevel>,
    pub created_at: DateTime<Utc>,
}

/// The newest entries first; `before` pages further back.
#[tracing::instrument(skip(pool))]
pub async fn log(
    pool: &PgPool,
    workspace_id: Uuid,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<AccessLogEntry>> {
    let entries = sqlx::query_as::<_, AccessLogEntry>(
        "SELECT log.id, log.staff_name, log.method, log.route, log.switcher_id,
                switchers.name AS switcher_name, support_grants.level AS grant_level,
                COALESCE(support_grants.break_glass, false) AS break_glass,
                in_force.level AS grant_in_force, log.created_at
         FROM support_access_log AS log
         LEFT JOIN switchers ON switchers.id = log.switcher_id
         LEFT JOIN support_grants ON support_grants.id = log.grant_id
         LEFT JOIN LATERAL (
             SELECT level FROM support_grants AS grant_then
             WHERE grant_then.workspace_id = log.workspace_id
               AND NOT grant_then.break_glass
               AND grant_then.created_at <= log.created_at
               AND LEAST(grant_then.expires_at, COALESCE(grant_then.revoked_at, grant_then.expires_at))
                   > log.created_at
             ORDER BY grant_then.created_at DESC
             LIMIT 1
         ) AS in_force ON true
         WHERE log.workspace_id = $1 AND ($2::timestamptz IS NULL OR log.created_at < $2)
         ORDER BY log.created_at DESC
         LIMIT $3",
    )
    .bind(workspace_id)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(entries)
}

/// How long the log and ended grants are kept.
pub const RETENTION_DAYS: i32 = 365;

/// Deletes log entries and ended grants older than [`RETENTION_DAYS`].
#[tracing::instrument(skip(pool))]
pub async fn delete_expired(pool: &PgPool) -> Result<u64> {
    let log = sqlx::query(
        "DELETE FROM support_access_log WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(RETENTION_DAYS)
    .execute(pool)
    .await?;
    let grants = sqlx::query(
        "DELETE FROM support_grants
         WHERE LEAST(expires_at, COALESCE(revoked_at, expires_at))
               < now() - make_interval(days => $1)",
    )
    .bind(RETENTION_DAYS)
    .execute(pool)
    .await?;

    Ok(log.rows_affected() + grants.rows_affected())
}
