//! Organizations: companies that manage workspaces ("clients") for their
//! customers. Staff create them and set the pooled limits; the organization's
//! team creates the client workspaces and invites the customers. A client
//! workspace has no owner member: the organization owns it, its team gets the
//! workspace permissions of their role (`organization_role_permissions`), and its
//! limits come from the organization (`workspace_plans`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::Result,
    workspaces::{Workspace, WorkspaceRoleEnum},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum OrganizationRole {
    Member,
    Admin,
}

/// A capability of an organization's team, from `organization_role_permissions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrganizationPermission(&'static str);

impl OrganizationPermission {
    pub const VIEW: Self = Self("org.view");
    pub const MANAGE: Self = Self("org.manage");
    pub const MEMBERS: Self = Self("org.members");

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    /// The pool: switchers in all its workspaces together.
    pub max_switchers: i32,
    /// Per switcher, in every client workspace.
    pub max_stream_servers: i32,
    /// Per client workspace.
    pub max_workspace_members: i32,
    pub history_days: i32,
    /// The organization lets its customers connect a chat bot of their own
    /// (its own setting; the organization's bot speaks otherwise).
    pub custom_chat_bot: bool,
    pub suspended_at: Option<DateTime<Utc>>,
    pub suspended_reason: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What staff set on an organization.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_switchers: i32,
    pub max_stream_servers: i32,
    pub max_workspace_members: i32,
    pub history_days: i32,
}

#[tracing::instrument(skip(pool))]
pub async fn create(
    pool: &PgPool,
    name: &str,
    url_slug: &str,
    limits: Limits,
    created_by: Uuid,
) -> Result<Organization> {
    let organization = sqlx::query_as::<_, Organization>(
        "
        INSERT INTO organizations (name, url_slug, max_switchers, max_stream_servers,
                                   max_workspace_members, history_days, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING *
        ",
    )
    .bind(name)
    .bind(url_slug)
    .bind(limits.max_switchers)
    .bind(limits.max_stream_servers)
    .bind(limits.max_workspace_members)
    .bind(limits.history_days)
    .bind(created_by)
    .fetch_one(pool)
    .await?;

    Ok(organization)
}

/// The result of changing a switcher limit.
#[derive(Debug, Clone)]
pub enum LimitUpdate<T> {
    Updated(T),
    NotFound,
    /// A switcher limit can't go below the switchers already there: running
    /// switchers are never stopped by a limit, so capacity must be freed first.
    BelowUsage {
        used: i64,
    },
}

/// Renames an organization and changes its limits, unless the pool would end up
/// smaller than the switchers its clients already have. Locks the organization
/// row, like [`crate::switchers::create_within_plan`] does for the pool check.
#[tracing::instrument(skip(pool))]
pub async fn update(
    pool: &PgPool,
    organization_id: Uuid,
    name: &str,
    limits: Limits,
) -> Result<LimitUpdate<Organization>> {
    let mut tx = pool.begin().await?;

    let locked =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM organizations WHERE id = $1 FOR UPDATE")
            .bind(organization_id)
            .fetch_optional(&mut *tx)
            .await?;
    if locked.is_none() {
        return Ok(LimitUpdate::NotFound);
    }

    let used = sqlx::query_scalar::<_, i64>(
        "
        SELECT count(*) FROM switchers
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        WHERE workspaces.organization_id = $1
        ",
    )
    .bind(organization_id)
    .fetch_one(&mut *tx)
    .await?;
    if i64::from(limits.max_switchers) < used {
        return Ok(LimitUpdate::BelowUsage { used });
    }

    let organization = sqlx::query_as::<_, Organization>(
        "
        UPDATE organizations
        SET name = $2, max_switchers = $3, max_stream_servers = $4,
            max_workspace_members = $5, history_days = $6
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(organization_id)
    .bind(name)
    .bind(limits.max_switchers)
    .bind(limits.max_stream_servers)
    .bind(limits.max_workspace_members)
    .bind(limits.history_days)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(LimitUpdate::Updated(organization))
}

/// Whether the organization's customers may connect a chat bot of their own.
#[tracing::instrument(skip(pool))]
pub async fn set_custom_chat_bot(
    pool: &PgPool,
    organization_id: Uuid,
    allowed: bool,
) -> Result<Option<Organization>> {
    let organization = sqlx::query_as::<_, Organization>(
        "UPDATE organizations SET custom_chat_bot = $2 WHERE id = $1 RETURNING *",
    )
    .bind(organization_id)
    .bind(allowed)
    .fetch_optional(pool)
    .await?;

    Ok(organization)
}

#[tracing::instrument(skip(pool))]
pub async fn get_by_id(pool: &PgPool, organization_id: Uuid) -> Result<Option<Organization>> {
    let organization =
        sqlx::query_as::<_, Organization>("SELECT * FROM organizations WHERE id = $1")
            .bind(organization_id)
            .fetch_optional(pool)
            .await?;

    Ok(organization)
}

#[tracing::instrument(skip(pool))]
pub async fn get_by_slug(pool: &PgPool, url_slug: &str) -> Result<Option<Organization>> {
    let organization =
        sqlx::query_as::<_, Organization>("SELECT * FROM organizations WHERE url_slug = $1")
            .bind(url_slug)
            .fetch_optional(pool)
            .await?;

    Ok(organization)
}

#[tracing::instrument(skip(pool))]
pub async fn slug_exists(pool: &PgPool, url_slug: &str) -> Result<bool> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM organizations WHERE url_slug = $1)",
    )
    .bind(url_slug)
    .fetch_one(pool)
    .await?;

    Ok(exists)
}

/// An organization with the user's role in it and how much of its pool is used.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrganizationSummary {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    pub max_switchers: i32,
    pub max_stream_servers: i32,
    pub max_workspace_members: i32,
    pub history_days: i32,
    pub custom_chat_bot: bool,
    pub suspended_at: Option<DateTime<Utc>>,
    pub suspended_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    /// `None` when staff look at an organization they aren't on.
    pub role: Option<OrganizationRole>,
    pub workspace_count: i64,
    pub switcher_count: i64,
    pub member_count: i64,
}

macro_rules! summary_select {
    () => {
        "
    SELECT
        organizations.id, organizations.name, organizations.url_slug,
        organizations.max_switchers, organizations.max_stream_servers,
        organizations.max_workspace_members, organizations.history_days,
        organizations.custom_chat_bot, organizations.suspended_at,
        organizations.suspended_reason, organizations.created_at,
        members.role,
        (SELECT count(*) FROM workspaces WHERE workspaces.organization_id = organizations.id)
            AS workspace_count,
        (SELECT count(*) FROM switchers
         JOIN workspaces ON workspaces.id = switchers.workspace_id
         WHERE workspaces.organization_id = organizations.id) AS switcher_count,
        (SELECT count(*) FROM organization_members
         WHERE organization_members.organization_id = organizations.id) AS member_count
    FROM organizations
    LEFT JOIN organization_members AS members
      ON members.organization_id = organizations.id AND members.user_id = $1
"
    };
}

/// The organizations `user_id` is on the team of.
#[tracing::instrument(skip(pool))]
pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<OrganizationSummary>> {
    let organizations = sqlx::query_as::<_, OrganizationSummary>(concat!(
        summary_select!(),
        " WHERE members.user_id IS NOT NULL ORDER BY organizations.name"
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(organizations)
}

/// Every organization, for staff (`user_id` is only used for their own role).
#[tracing::instrument(skip(pool))]
pub async fn list_all(pool: &PgPool, user_id: Uuid) -> Result<Vec<OrganizationSummary>> {
    let organizations = sqlx::query_as::<_, OrganizationSummary>(concat!(
        summary_select!(),
        " ORDER BY organizations.name"
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(organizations)
}

#[tracing::instrument(skip(pool))]
pub async fn get_summary_by_slug(
    pool: &PgPool,
    url_slug: &str,
    user_id: Uuid,
) -> Result<Option<OrganizationSummary>> {
    let organization = sqlx::query_as::<_, OrganizationSummary>(concat!(
        summary_select!(),
        " WHERE organizations.url_slug = $2"
    ))
    .bind(user_id)
    .bind(url_slug)
    .fetch_optional(pool)
    .await?;

    Ok(organization)
}

#[tracing::instrument(skip(pool))]
pub async fn get_summary(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
) -> Result<Option<OrganizationSummary>> {
    let organization = sqlx::query_as::<_, OrganizationSummary>(concat!(
        summary_select!(),
        " WHERE organizations.id = $2"
    ))
    .bind(user_id)
    .bind(organization_id)
    .fetch_optional(pool)
    .await?;

    Ok(organization)
}

/// Suspends (`Some(reason)`) or lifts the suspension (`None`). Returns whether
/// the organization exists.
#[tracing::instrument(skip(pool))]
pub async fn set_suspension(
    pool: &PgPool,
    organization_id: Uuid,
    reason: Option<&str>,
) -> Result<bool> {
    let result = sqlx::query(
        "
        UPDATE organizations
        SET suspended_at = CASE WHEN $2::text IS NULL THEN NULL ELSE COALESCE(suspended_at, now()) END,
            suspended_reason = $2
        WHERE id = $1
        ",
    )
    .bind(organization_id)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Deletes an organization with no client workspaces left (`false` if it has some
/// or doesn't exist): delete or hand over the workspaces first.
#[tracing::instrument(skip(pool))]
pub async fn delete_if_empty(pool: &PgPool, organization_id: Uuid) -> Result<bool> {
    let result = sqlx::query(
        "
        DELETE FROM organizations
        WHERE id = $1
          AND NOT EXISTS (SELECT 1 FROM workspaces WHERE workspaces.organization_id = $1)
        ",
    )
    .bind(organization_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

// --- Team ------------------------------------------------------------------

/// Whether `user_id` is on the team of `organization_id` with `permission`.
#[tracing::instrument(skip(pool))]
pub async fn has_permission(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
    permission: OrganizationPermission,
) -> Result<bool> {
    let allowed = sqlx::query_scalar::<_, bool>(
        "
        SELECT EXISTS (
            SELECT 1
            FROM organization_members AS members
            JOIN organization_role_permissions AS role_permissions
              ON role_permissions.role = members.role
            WHERE members.organization_id = $1
              AND members.user_id = $2
              AND role_permissions.permission = $3
        )
        ",
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(permission.as_str())
    .fetch_one(pool)
    .await?;

    Ok(allowed)
}

#[tracing::instrument(skip(pool))]
pub async fn member_role(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
) -> Result<Option<OrganizationRole>> {
    let role = sqlx::query_scalar::<_, OrganizationRole>(
        "SELECT role FROM organization_members WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(role)
}

pub async fn add_member<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    organization_id: Uuid,
    user_id: Uuid,
    role: OrganizationRole,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role) VALUES ($1, $2, $3)",
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(role)
    .execute(executor)
    .await?;

    Ok(())
}

/// A team member with the profile of their most recently used login.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TeamMember {
    pub user_id: Uuid,
    pub role: OrganizationRole,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    pub provider: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Admins first, then by join date.
#[tracing::instrument(skip(pool))]
pub async fn list_members(pool: &PgPool, organization_id: Uuid) -> Result<Vec<TeamMember>> {
    let members = sqlx::query_as::<_, TeamMember>(
        "
        SELECT
            members.user_id,
            members.role,
            profile.provider_username AS username,
            profile.provider_avatar_url AS avatar_url,
            profile.provider,
            members.created_at
        FROM organization_members AS members
        JOIN organization_roles AS roles ON roles.role = members.role
        LEFT JOIN LATERAL (
            SELECT provider::text, provider_username, provider_avatar_url
            FROM oauth2_connections
            WHERE oauth2_connections.user_id = members.user_id
            ORDER BY updated_at DESC
            LIMIT 1
        ) AS profile ON true
        WHERE members.organization_id = $1
        ORDER BY roles.rank DESC, members.created_at
        ",
    )
    .bind(organization_id)
    .fetch_all(pool)
    .await?;

    Ok(members)
}

/// Changes a member's role, unless that would leave the team without an admin
/// (`Ok(false)` then, or when they aren't on the team). Locks the organization,
/// like [`remove_member`].
#[tracing::instrument(skip(pool))]
pub async fn update_member_role(
    pool: &PgPool,
    organization_id: Uuid,
    user_id: Uuid,
    role: OrganizationRole,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT 1 FROM organizations WHERE id = $1 FOR UPDATE")
        .bind(organization_id)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query(
        "
        UPDATE organization_members SET role = $3
        WHERE organization_id = $1 AND user_id = $2
          AND (role <> 'admin' OR $3 = 'admin' OR EXISTS (
              SELECT 1 FROM organization_members others
              WHERE others.organization_id = $1 AND others.role = 'admin' AND others.user_id <> $2))
        ",
    )
    .bind(organization_id)
    .bind(user_id)
    .bind(role)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(result.rows_affected() > 0)
}

/// Removes someone from the team, unless they're the last admin (`Ok(false)`
/// then, or when they weren't on the team). Locks the organization, so two
/// admins can't remove each other at once.
#[tracing::instrument(skip(pool))]
pub async fn remove_member(pool: &PgPool, organization_id: Uuid, user_id: Uuid) -> Result<bool> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT 1 FROM organizations WHERE id = $1 FOR UPDATE")
        .bind(organization_id)
        .execute(&mut *tx)
        .await?;

    let removed = sqlx::query_scalar::<_, OrganizationRole>(
        "
        DELETE FROM organization_members
        WHERE organization_id = $1 AND user_id = $2
          AND (role <> 'admin' OR EXISTS (
              SELECT 1 FROM organization_members others
              WHERE others.organization_id = $1 AND others.role = 'admin' AND others.user_id <> $2))
        RETURNING role
        ",
    )
    .bind(organization_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(removed.is_some())
}

// --- Client workspaces -----------------------------------------------------

macro_rules! client_select {
    () => {
        "
        SELECT
            workspaces.id, workspaces.name, workspaces.url_slug, workspaces.switcher_allocation,
            (SELECT count(*) FROM switchers WHERE switchers.workspace_id = workspaces.id)
                AS switcher_count,
            (SELECT count(*) FROM workspace_members WHERE workspace_members.workspace_id = workspaces.id)
                AS member_count,
            workspaces.suspended_at, workspaces.created_at
        FROM workspaces
        "
    };
}

/// The condition of a `ClientFilter` (search `$2`, claimed `$3`).
macro_rules! client_match {
    () => {
        "
        ($2::text IS NULL OR workspaces.id::text = $2
            OR workspaces.name ILIKE '%' || $2 || '%' OR workspaces.url_slug ILIKE '%' || $2 || '%')
        AND ($3::bool IS NULL OR $3 = EXISTS (
            SELECT 1 FROM workspace_members WHERE workspace_members.workspace_id = workspaces.id))
        "
    };
}

/// A client workspace as the organization lists it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Client {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    pub switcher_allocation: Option<i32>,
    pub switcher_count: i64,
    /// Customers who joined (the organization's team isn't counted).
    pub member_count: i64,
    pub suspended_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// How a page of clients is picked and ordered.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientFilter<'a> {
    /// Name, slug or ID.
    pub search: Option<&'a str>,
    /// Only clients a customer has (not) joined.
    pub claimed: Option<bool>,
    /// By name instead of newest first.
    pub by_name: bool,
}

/// Which clients of an organization match `filter`: a page of them, and how many
/// match in all.
#[tracing::instrument(skip(pool, filter))]
pub async fn list_clients(
    pool: &PgPool,
    organization_id: Uuid,
    filter: ClientFilter<'_>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<Client>, i64)> {
    let clients = sqlx::query_as::<_, Client>(concat!(
        client_select!(),
        "
        WHERE workspaces.organization_id = $1 AND ",
        client_match!(),
        "
        ORDER BY (CASE WHEN $4 THEN lower(workspaces.name) END), workspaces.created_at DESC,
                 workspaces.id
        LIMIT $5 OFFSET $6
        "
    ))
    .bind(organization_id)
    .bind(filter.search)
    .bind(filter.claimed)
    .bind(filter.by_name)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let total = sqlx::query_scalar::<_, i64>(concat!(
        "
        SELECT count(*) FROM workspaces
        WHERE workspaces.organization_id = $1 AND ",
        client_match!()
    ))
    .bind(organization_id)
    .bind(filter.search)
    .bind(filter.claimed)
    .fetch_one(pool)
    .await?;

    Ok((clients, total))
}

/// How many client workspaces the organization has.
#[tracing::instrument(skip(pool))]
pub async fn client_count(pool: &PgPool, organization_id: Uuid) -> Result<i64> {
    let count =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workspaces WHERE organization_id = $1")
            .bind(organization_id)
            .fetch_one(pool)
            .await?;

    Ok(count)
}

/// One client of the organization (none for another organization's workspace).
#[tracing::instrument(skip(pool))]
pub async fn get_client(
    pool: &PgPool,
    organization_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<Client>> {
    let client = sqlx::query_as::<_, Client>(concat!(
        client_select!(),
        "
        WHERE workspaces.organization_id = $1 AND workspaces.id = $2
        "
    ))
    .bind(organization_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;

    Ok(client)
}

/// Creates a client workspace: no owner (the organization is), no plan.
#[tracing::instrument(skip(pool))]
pub async fn create_client(
    pool: &PgPool,
    organization_id: Uuid,
    created_by: Uuid,
    name: &str,
    url_slug: &str,
    switcher_allocation: Option<i32>,
) -> Result<Workspace> {
    let workspace = sqlx::query_as::<_, Workspace>(
        "
        INSERT INTO workspaces (created_by, name, url_slug, organization_id, switcher_allocation)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        ",
    )
    .bind(created_by)
    .bind(name)
    .bind(url_slug)
    .bind(organization_id)
    .bind(switcher_allocation)
    .fetch_one(pool)
    .await?;

    Ok(workspace)
}

/// Changes a client's name and allocation, unless the allocation would be
/// smaller than the switchers it already has. `NotFound` if `workspace_id` isn't
/// a client of `organization_id`. Locks the workspace row, like
/// [`crate::switchers::create_within_plan`].
#[tracing::instrument(skip(pool))]
pub async fn update_client(
    pool: &PgPool,
    organization_id: Uuid,
    workspace_id: Uuid,
    name: &str,
    switcher_allocation: Option<i32>,
) -> Result<LimitUpdate<Workspace>> {
    let mut tx = pool.begin().await?;

    let locked = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM workspaces WHERE id = $2 AND organization_id = $1 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(workspace_id)
    .fetch_optional(&mut *tx)
    .await?;
    if locked.is_none() {
        return Ok(LimitUpdate::NotFound);
    }

    let used = crate::switchers::count_in_workspace(&mut *tx, workspace_id).await?;
    if switcher_allocation.is_some_and(|allocation| i64::from(allocation) < used) {
        return Ok(LimitUpdate::BelowUsage { used });
    }

    let workspace = sqlx::query_as::<_, Workspace>(
        "
        UPDATE workspaces SET name = $3, switcher_allocation = $4
        WHERE id = $2 AND organization_id = $1
        RETURNING *
        ",
    )
    .bind(organization_id)
    .bind(workspace_id)
    .bind(name)
    .bind(switcher_allocation)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(LimitUpdate::Updated(workspace))
}

/// The ids of an organization's client workspaces, e.g. to tell their switchers to
/// re-check which are paused after the limits change.
#[tracing::instrument(skip(pool))]
pub async fn workspace_ids(pool: &PgPool, organization_id: Uuid) -> Result<Vec<Uuid>> {
    let ids = sqlx::query_scalar::<_, Uuid>("SELECT id FROM workspaces WHERE organization_id = $1")
        .bind(organization_id)
        .fetch_all(pool)
        .await?;

    Ok(ids)
}

/// The workspace's organization, if it's a client workspace.
#[tracing::instrument(skip(pool))]
pub async fn of_workspace(pool: &PgPool, workspace_id: Uuid) -> Result<Option<Organization>> {
    let organization = sqlx::query_as::<_, Organization>(
        "
        SELECT organizations.* FROM organizations
        JOIN workspaces ON workspaces.organization_id = organizations.id
        WHERE workspaces.id = $1
        ",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;

    Ok(organization)
}

/// The role the invite for a customer gives: always admin, never owner.
pub const CUSTOMER_ROLE: WorkspaceRoleEnum = WorkspaceRoleEnum::Admin;

// --- Team invites ----------------------------------------------------------

/// An invite to join an organization's team. Only the token's SHA-256 is stored.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TeamInvite {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub role: OrganizationRole,
    pub invited_by: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub accepted_by: Option<Uuid>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

macro_rules! invite_columns {
    () => {
        "id, organization_id, role, invited_by, expires_at, accepted_at, accepted_by, revoked_at, created_at"
    };
}

/// `invited_by` is `None` for staff, who may not be on the team.
#[tracing::instrument(skip(pool, token_hash))]
pub async fn create_invite(
    pool: &PgPool,
    organization_id: Uuid,
    role: OrganizationRole,
    invited_by: Option<Uuid>,
    token_hash: &[u8],
    expires_at: DateTime<Utc>,
) -> Result<TeamInvite> {
    let invite = sqlx::query_as::<_, TeamInvite>(concat!(
        "INSERT INTO organization_invites (organization_id, role, invited_by, token_hash, expires_at) ",
        "VALUES ($1, $2, $3, $4, $5) RETURNING ",
        invite_columns!()
    ))
    .bind(organization_id)
    .bind(role)
    .bind(invited_by)
    .bind(token_hash)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    Ok(invite)
}

/// Invites that can still be accepted.
#[tracing::instrument(skip(pool))]
pub async fn list_open_invites(pool: &PgPool, organization_id: Uuid) -> Result<Vec<TeamInvite>> {
    let invites = sqlx::query_as::<_, TeamInvite>(concat!(
        "SELECT ",
        invite_columns!(),
        " FROM organization_invites",
        " WHERE organization_id = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > now()",
        " ORDER BY created_at DESC"
    ))
    .bind(organization_id)
    .fetch_all(pool)
    .await?;

    Ok(invites)
}

/// Revokes an open invite. Returns whether there was one to revoke.
#[tracing::instrument(skip(pool))]
pub async fn revoke_invite(pool: &PgPool, organization_id: Uuid, invite_id: Uuid) -> Result<bool> {
    let result = sqlx::query(
        "
        UPDATE organization_invites SET revoked_at = now()
        WHERE id = $1 AND organization_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL
        ",
    )
    .bind(invite_id)
    .bind(organization_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// What someone opening a team invite link sees.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TeamInvitePreview {
    pub organization_name: String,
    pub organization_slug: String,
    pub role: OrganizationRole,
    pub invited_by_username: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub already_member: bool,
}

#[tracing::instrument(skip(pool, token_hash))]
pub async fn preview_invite(
    pool: &PgPool,
    token_hash: &[u8],
    user_id: Uuid,
) -> Result<Option<TeamInvitePreview>> {
    let preview = sqlx::query_as::<_, TeamInvitePreview>(
        "
        SELECT
            organizations.name AS organization_name,
            organizations.url_slug AS organization_slug,
            invites.role,
            inviter.provider_username AS invited_by_username,
            invites.expires_at,
            invites.accepted_at,
            invites.revoked_at,
            EXISTS (SELECT 1 FROM organization_members
                    WHERE organization_members.organization_id = invites.organization_id
                      AND organization_members.user_id = $2) AS already_member
        FROM organization_invites AS invites
        JOIN organizations ON organizations.id = invites.organization_id
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
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(preview)
}

/// The result of trying to accept a team invite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptOutcome {
    Joined {
        url_slug: String,
    },
    /// The user is on the team already; the invite is left unused.
    AlreadyMember {
        url_slug: String,
    },
    NotFound,
    Expired,
    Revoked,
    AlreadyUsed,
}

/// Accepts a team invite for `user_id` in one transaction (the invite is
/// locked, so it's used at most once).
#[tracing::instrument(skip(pool, token_hash))]
pub async fn accept_invite(
    pool: &PgPool,
    token_hash: &[u8],
    user_id: Uuid,
) -> Result<AcceptOutcome> {
    let mut tx = pool.begin().await?;

    let Some(invite) = sqlx::query_as::<_, TeamInvite>(concat!(
        "SELECT ",
        invite_columns!(),
        " FROM organization_invites WHERE token_hash = $1 FOR UPDATE"
    ))
    .bind(token_hash)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(AcceptOutcome::NotFound);
    };

    let url_slug =
        sqlx::query_scalar::<_, String>("SELECT url_slug FROM organizations WHERE id = $1")
            .bind(invite.organization_id)
            .fetch_one(&mut *tx)
            .await?;

    let already_member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM organization_members WHERE organization_id = $1 AND user_id = $2)",
    )
    .bind(invite.organization_id)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    if already_member {
        return Ok(AcceptOutcome::AlreadyMember { url_slug });
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

    add_member(&mut *tx, invite.organization_id, user_id, invite.role).await?;
    sqlx::query(
        "UPDATE organization_invites SET accepted_at = now(), accepted_by = $2 WHERE id = $1",
    )
    .bind(invite.id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(AcceptOutcome::Joined { url_slug })
}

/// Which switchers the given client workspaces have (workspace, switcher), e.g. to
/// read their live status for a page of the client list.
#[tracing::instrument(skip(pool, workspace_ids))]
pub async fn client_switchers(
    pool: &PgPool,
    organization_id: Uuid,
    workspace_ids: &[Uuid],
) -> Result<Vec<(Uuid, Uuid)>> {
    let pairs = sqlx::query_as::<_, (Uuid, Uuid)>(
        "
        SELECT switchers.workspace_id, switchers.id
        FROM switchers
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        WHERE workspaces.organization_id = $1 AND workspaces.id = ANY($2)
        ",
    )
    .bind(organization_id)
    .bind(workspace_ids)
    .fetch_all(pool)
    .await?;

    Ok(pairs)
}

/// A switcher of a client workspace, named for lists across the organization.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClientSwitcher {
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
    pub switcher_id: Uuid,
    pub switcher_name: String,
}

/// Every switcher in the organization's client workspaces, with names.
#[tracing::instrument(skip(pool))]
pub async fn named_client_switchers(
    pool: &PgPool,
    organization_id: Uuid,
) -> Result<Vec<ClientSwitcher>> {
    let switchers = sqlx::query_as::<_, ClientSwitcher>(
        "
        SELECT workspaces.id AS workspace_id, workspaces.name AS workspace_name,
               workspaces.url_slug AS workspace_slug,
               switchers.id AS switcher_id, switchers.name AS switcher_name
        FROM switchers
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        WHERE workspaces.organization_id = $1
        ",
    )
    .bind(organization_id)
    .fetch_all(pool)
    .await?;

    Ok(switchers)
}

/// An activity event of a client's switcher, with where it happened.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ActivityRow {
    pub id: Uuid,
    pub kind: String,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub switcher_id: Uuid,
    pub switcher_name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
}

/// What to show of the organization's activity.
#[derive(Debug, Clone, Copy, Default)]
pub struct ActivityFilter<'a> {
    /// One client workspace.
    pub workspace_id: Option<Uuid>,
    /// Part of a client's or a switcher's name, or a client's URL.
    pub search: Option<&'a str>,
    /// Only these kinds of event.
    pub kinds: Option<&'a [String]>,
}

/// The newest activity across all the organization's clients first; `before` pages
/// further back.
#[tracing::instrument(skip(pool, filter))]
pub async fn activity(
    pool: &PgPool,
    organization_id: Uuid,
    filter: ActivityFilter<'_>,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<ActivityRow>> {
    let rows = sqlx::query_as::<_, ActivityRow>(
        "
        SELECT switcher_events.id, switcher_events.kind, switcher_events.data,
               switcher_events.created_at, switchers.id AS switcher_id,
               switchers.name AS switcher_name, workspaces.id AS workspace_id,
               workspaces.name AS workspace_name, workspaces.url_slug AS workspace_slug
        FROM switcher_events
        JOIN switchers ON switchers.id = switcher_events.switcher_id
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        WHERE workspaces.organization_id = $1
          AND ($2::uuid IS NULL OR workspaces.id = $2)
          AND ($3::text IS NULL OR workspaces.name ILIKE '%' || $3 || '%'
               OR workspaces.url_slug ILIKE '%' || $3 || '%'
               OR switchers.name ILIKE '%' || $3 || '%')
          AND ($4::text[] IS NULL OR switcher_events.kind = ANY($4))
          AND ($5::timestamptz IS NULL OR switcher_events.created_at < $5)
        ORDER BY switcher_events.created_at DESC
        LIMIT $6
        ",
    )
    .bind(organization_id)
    .bind(filter.workspace_id)
    .bind(filter.search)
    .bind(filter.kinds)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// The organizations where `user_id` is the only admin, which would be left
/// without one if they went (named, for the message).
#[tracing::instrument(skip(pool))]
pub async fn sole_admin_of(pool: &PgPool, user_id: Uuid) -> Result<Vec<String>> {
    let names = sqlx::query_scalar::<_, String>(
        "
        SELECT organizations.name
        FROM organization_members AS members
        JOIN organizations ON organizations.id = members.organization_id
        WHERE members.user_id = $1 AND members.role = 'admin'
          AND NOT EXISTS (
              SELECT 1 FROM organization_members others
              WHERE others.organization_id = members.organization_id
                AND others.role = 'admin' AND others.user_id <> $1)
        ORDER BY organizations.name
        ",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(names)
}
