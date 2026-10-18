//! What staff did: actions from Admin (`staff_actions`), and together with
//! support work inside workspaces (`support_access_log`) the audit log staff
//! read in Admin.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

/// An action from Admin, as recorded.
#[derive(Debug, Clone, Default)]
pub struct NewAction<'a> {
    pub staff_user_id: Uuid,
    pub staff_name: &'a str,
    /// A stable snake_case code, e.g. `suspend_workspace`.
    pub action: &'a str,
    pub workspace_id: Option<Uuid>,
    pub workspace_name: Option<&'a str>,
    pub switcher_id: Option<Uuid>,
    pub switcher_name: Option<&'a str>,
    pub target_user_id: Option<Uuid>,
    pub reason: Option<&'a str>,
    pub details: serde_json::Value,
}

#[tracing::instrument(skip(pool))]
pub async fn record(pool: &PgPool, action: &NewAction<'_>) -> Result<()> {
    let details = if action.details.is_object() {
        action.details.clone()
    } else {
        serde_json::json!({})
    };
    sqlx::query(
        "INSERT INTO staff_actions
             (staff_user_id, staff_name, action, workspace_id, workspace_name,
              switcher_id, switcher_name, target_user_id, reason, details)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(action.staff_user_id)
    .bind(action.staff_name)
    .bind(action.action)
    .bind(action.workspace_id)
    .bind(action.workspace_name)
    .bind(action.switcher_id)
    .bind(action.switcher_name)
    .bind(action.target_user_id)
    .bind(action.reason)
    .bind(details)
    .execute(pool)
    .await?;

    Ok(())
}

/// One line of the staff audit log.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub staff_name: String,
    /// `admin` (from Admin) or `support` (inside a workspace).
    pub kind: String,
    /// The action code (admin) or the API route (support).
    pub action: String,
    /// The HTTP method (support only).
    pub method: Option<String>,
    pub workspace_id: Option<Uuid>,
    pub workspace_name: Option<String>,
    pub workspace_slug: Option<String>,
    pub switcher_name: Option<String>,
    pub target_user_id: Option<Uuid>,
    /// The target user's newest linked account's name; `None` once deleted.
    pub target_user_name: Option<String>,
    pub reason: Option<String>,
    /// Support: the level it was done under, and whether break-glass.
    pub grant_level: Option<crate::support::SupportLevel>,
    pub break_glass: bool,
}

/// Staff actions and support work, newest first; `before` pages further back.
/// `kind` narrows it to `admin` or `support`.
#[tracing::instrument(skip(pool))]
pub async fn audit(
    pool: &PgPool,
    kind: Option<&str>,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<AuditEntry>> {
    let entries = sqlx::query_as::<_, AuditEntry>(
        "SELECT * FROM (
             SELECT actions.id, actions.created_at, actions.staff_name, 'admin' AS kind,
                    actions.action, NULL::text AS method, actions.workspace_id,
                    COALESCE(workspaces.name, actions.workspace_name) AS workspace_name,
                    workspaces.url_slug AS workspace_slug, actions.switcher_name,
                    actions.target_user_id,
                    (SELECT provider_username FROM oauth2_connections
                     WHERE user_id = actions.target_user_id
                     ORDER BY updated_at DESC LIMIT 1) AS target_user_name,
                    actions.reason, NULL::support_access_level AS grant_level,
                    false AS break_glass
             FROM staff_actions AS actions
             LEFT JOIN workspaces ON workspaces.id = actions.workspace_id
             UNION ALL
             SELECT log.id, log.created_at, log.staff_name, 'support', log.route, log.method,
                    log.workspace_id, workspaces.name, workspaces.url_slug, switchers.name,
                    NULL, NULL, NULL, support_grants.level,
                    COALESCE(support_grants.break_glass, false)
             FROM support_access_log AS log
             JOIN workspaces ON workspaces.id = log.workspace_id
             LEFT JOIN switchers ON switchers.id = log.switcher_id
             LEFT JOIN support_grants ON support_grants.id = log.grant_id
             -- Admin actions are mirrored into workspace logs, and listed above already.
             WHERE log.route NOT LIKE '/admin/%'
         ) AS entries
         WHERE ($1::text IS NULL OR kind = $1)
           AND ($2::timestamptz IS NULL OR created_at < $2)
         ORDER BY created_at DESC
         LIMIT $3",
    )
    .bind(kind)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(entries)
}

/// How long staff actions are kept, like the support access log.
pub const RETENTION_DAYS: i32 = crate::support::RETENTION_DAYS;

#[tracing::instrument(skip(pool))]
pub async fn delete_expired(pool: &PgPool) -> Result<u64> {
    let deleted = sqlx::query(
        "DELETE FROM staff_actions WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(RETENTION_DAYS)
    .execute(pool)
    .await?;
    Ok(deleted.rows_affected())
}
