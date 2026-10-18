use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::Result,
    workspaces::{self, WorkspaceRoleEnum},
};

/// A workspace invite. The token itself is never stored, only its SHA-256.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Invite {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub role: WorkspaceRoleEnum,
    pub invited_by: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub accepted_by: Option<Uuid>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// The columns of [`Invite`], kept as a macro so queries stay `&'static str`.
macro_rules! invite_columns {
    () => {
        "id, workspace_id, role, invited_by, expires_at, accepted_at, accepted_by, revoked_at, created_at"
    };
}

#[tracing::instrument(skip(pool, token_hash))]
pub async fn create(
    pool: &PgPool,
    workspace_id: Uuid,
    role: WorkspaceRoleEnum,
    invited_by: Uuid,
    token_hash: &[u8],
    expires_at: DateTime<Utc>,
) -> Result<Invite> {
    let invite = sqlx::query_as::<_, Invite>(concat!(
        "INSERT INTO workspace_invites (workspace_id, role, invited_by, token_hash, expires_at) ",
        "VALUES ($1, $2, $3, $4, $5) RETURNING ",
        invite_columns!()
    ))
    .bind(workspace_id)
    .bind(role)
    .bind(invited_by)
    .bind(token_hash)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    Ok(invite)
}

/// Invites that can still be accepted: not used, revoked or expired.
#[tracing::instrument(skip(pool))]
pub async fn list_open(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<Invite>> {
    let invites = sqlx::query_as::<_, Invite>(concat!(
        "SELECT ",
        invite_columns!(),
        " FROM workspace_invites",
        " WHERE workspace_id = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > now()",
        " ORDER BY created_at DESC"
    ))
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;

    Ok(invites)
}

/// Revokes an open invite. Returns whether there was one to revoke.
#[tracing::instrument(skip(pool))]
pub async fn revoke(pool: &PgPool, workspace_id: Uuid, invite_id: Uuid) -> Result<bool> {
    let result = sqlx::query(
        "
        UPDATE workspace_invites SET revoked_at = now()
        WHERE id = $1 AND workspace_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL
        ",
    )
    .bind(invite_id)
    .bind(workspace_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// What someone opening an invite link sees.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InvitePreview {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
    /// The organization that manages this client workspace and pays for it.
    pub organization_name: Option<String>,
    pub role: WorkspaceRoleEnum,
    pub invited_by_username: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[tracing::instrument(skip(pool, token_hash))]
pub async fn preview_by_token_hash(
    pool: &PgPool,
    token_hash: &[u8],
) -> Result<Option<InvitePreview>> {
    let preview = sqlx::query_as::<_, InvitePreview>(
        "
        SELECT
            invites.id,
            invites.workspace_id,
            workspaces.name AS workspace_name,
            workspaces.url_slug AS workspace_slug,
            organizations.name AS organization_name,
            invites.role,
            inviter.provider_username AS invited_by_username,
            invites.expires_at,
            invites.accepted_at,
            invites.revoked_at
        FROM workspace_invites AS invites
        JOIN workspaces ON workspaces.id = invites.workspace_id
        LEFT JOIN organizations ON organizations.id = workspaces.organization_id
        LEFT JOIN LATERAL (
            SELECT provider_username
            FROM oauth2_connections
            WHERE oauth2_connections.user_id = invites.invited_by
            ORDER BY updated_at DESC
            LIMIT 1
        ) AS inviter ON true
        WHERE invites.token_hash = $1
        ",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;

    Ok(preview)
}

/// The result of trying to accept an invite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptOutcome {
    /// The user joined; carries the workspace's id and slug.
    Joined {
        workspace_id: Uuid,
        url_slug: String,
    },
    /// The user was already a member; the invite is left unused.
    AlreadyMember {
        workspace_id: Uuid,
        url_slug: String,
    },
    NotFound,
    Expired,
    Revoked,
    AlreadyUsed,
    /// The workspace's plan has no room for another member.
    MemberLimitReached,
}

/// Accepts an invite for `user_id` in one transaction.
///
/// The invite and its workspace are locked, so an invite is used at most once
/// and concurrent accepts can't push a workspace past its member limit.
#[tracing::instrument(skip(pool, token_hash))]
pub async fn accept(pool: &PgPool, token_hash: &[u8], user_id: Uuid) -> Result<AcceptOutcome> {
    let mut tx = pool.begin().await?;

    let Some(invite) = sqlx::query_as::<_, Invite>(concat!(
        "SELECT ",
        invite_columns!(),
        " FROM workspace_invites WHERE token_hash = $1 FOR UPDATE"
    ))
    .bind(token_hash)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(AcceptOutcome::NotFound);
    };

    let url_slug =
        sqlx::query_scalar::<_, String>("SELECT url_slug FROM workspaces WHERE id = $1 FOR UPDATE")
            .bind(invite.workspace_id)
            .fetch_one(&mut *tx)
            .await?;

    let already_member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM workspace_members WHERE workspace_id = $1 AND user_id = $2)",
    )
    .bind(invite.workspace_id)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    if already_member {
        return Ok(AcceptOutcome::AlreadyMember {
            workspace_id: invite.workspace_id,
            url_slug,
        });
    }
    if invite.revoked_at.is_some() {
        return Ok(AcceptOutcome::Revoked);
    }
    if invite.accepted_at.is_some() {
        return Ok(AcceptOutcome::AlreadyUsed);
    }
    if invite.expires_at <= Utc::now() {
        return Ok(AcceptOutcome::Expired);
    }

    let members = workspaces::member_count(&mut *tx, invite.workspace_id).await?;
    let max_members = crate::plans::for_workspace(&mut *tx, invite.workspace_id)
        .await?
        .map_or(0, |plan| i64::from(plan.max_workspace_members));
    if members >= max_members {
        return Ok(AcceptOutcome::MemberLimitReached);
    }

    workspaces::add_member(&mut *tx, invite.workspace_id, user_id, invite.role).await?;
    sqlx::query("UPDATE workspace_invites SET accepted_at = now(), accepted_by = $2 WHERE id = $1")
        .bind(invite.id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(AcceptOutcome::Joined {
        workspace_id: invite.workspace_id,
        url_slug,
    })
}
