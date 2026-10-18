use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub whitelisted: bool,
    pub staff: bool,
    pub last_workspace_id: Option<Uuid>,
    /// Set by staff: the user can't sign in.
    pub disabled_at: Option<DateTime<Utc>>,
    pub disabled_reason: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[tracing::instrument(skip(pool))]
pub async fn create(pool: &PgPool) -> Result<User> {
    let user = sqlx::query_as::<_, User>(
        "
        INSERT INTO users (
            whitelisted,
            staff
        )
        VALUES ($1, $2)
        RETURNING *
        ",
    )
    .bind(false)
    .bind(false)
    .fetch_one(pool)
    .await?;

    Ok(user)
}

// TODO: Of course we're going to paginate this at some point, but for now we
// just want to be able to list all users.
#[tracing::instrument(skip(pool))]
pub async fn list(pool: &PgPool) -> Result<Vec<User>> {
    let users = sqlx::query_as::<_, User>("SELECT * FROM users")
        .fetch_all(pool)
        .await?;

    Ok(users)
}

#[tracing::instrument(skip(pool))]
pub async fn get_by_id(pool: &PgPool, id: Uuid) -> Result<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;

    Ok(user)
}

/// Remembers the workspace a user opened last. Only sets it if they are a member.
#[tracing::instrument(skip(pool))]
pub async fn set_last_workspace(pool: &PgPool, user_id: Uuid, workspace_id: Uuid) -> Result<bool> {
    let result = sqlx::query(
        "
        UPDATE users SET last_workspace_id = $2
        WHERE id = $1
          AND EXISTS (
              SELECT 1 FROM workspace_members WHERE workspace_id = $2 AND user_id = $1
          )
        ",
    )
    .bind(user_id)
    .bind(workspace_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// The slug of the user's last workspace, if they are still a member of it.
#[tracing::instrument(skip(pool))]
pub async fn last_workspace_slug(pool: &PgPool, user_id: Uuid) -> Result<Option<String>> {
    let slug = sqlx::query_scalar::<_, String>(
        "
        SELECT workspaces.url_slug
        FROM users
        JOIN workspaces ON workspaces.id = users.last_workspace_id
        JOIN workspace_members AS members
          ON members.workspace_id = workspaces.id AND members.user_id = users.id
        WHERE users.id = $1
        ",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(slug)
}

/// Deletes an account: its sign-in connections, sessions and memberships go with
/// it; what it created or linked for a workspace stays, without it. `false` if it
/// didn't exist. Delete the workspaces it owns first.
#[tracing::instrument(skip(pool))]
pub async fn delete(pool: &PgPool, user_id: Uuid) -> Result<bool> {
    let deleted = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(deleted.rows_affected() > 0)
}

/// Disables the user (`Some(reason)`: no sign-in, every session ended) or
/// enables them again (`None`). `false` if there's no such user.
#[tracing::instrument(skip(pool, reason))]
pub async fn set_disabled(pool: &PgPool, user_id: Uuid, reason: Option<&str>) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let updated = sqlx::query(
        "UPDATE users
         SET disabled_at = CASE WHEN $2::text IS NULL THEN NULL ELSE COALESCE(disabled_at, now()) END,
             disabled_reason = $2
         WHERE id = $1",
    )
    .bind(user_id)
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    if reason.is_some() {
        crate::sessions::delete_all_for_user(&mut *tx, user_id).await?;
    }
    tx.commit().await?;

    Ok(updated.rows_affected() > 0)
}
