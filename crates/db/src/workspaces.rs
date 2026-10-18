use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{error::Result, organizations::OrganizationRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRoleEnum {
    Operator,
    Admin,
    Owner,
}

/// A capability granted to a workspace member through their role.
///
/// The values deliberately match the entries in the `permissions` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspacePermission(&'static str);

impl WorkspacePermission {
    pub const ACCOUNT_VIEW: Self = Self("account.view");
    pub const ACCOUNT_MANAGE: Self = Self("account.manage");
    pub const SWITCHER_VIEW: Self = Self("switcher.view");
    pub const SWITCHER_CONTROL: Self = Self("switcher.control");
    pub const SWITCHER_MANAGE: Self = Self("switcher.manage");
    pub const BILLING_MANAGE: Self = Self("billing.manage");
    pub const WORKSPACE_DELETE: Self = Self("workspace.delete");
    pub const WORKSPACE_TRANSFER: Self = Self("workspace.transfer");
    pub const WORKSPACE_SUPPORT_ACCESS: Self = Self("workspace.support_access");

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WorkspaceRole {
    pub role: WorkspaceRoleEnum,
    pub rank: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Workspace {
    pub id: Uuid,
    /// `None` once the account that created it is deleted.
    pub created_by: Option<Uuid>,
    pub name: String,
    pub url_slug: String,
    pub stripe_customer_id: Option<String>,
    pub plan_code: String,
    /// A transfer removed the previous owner's cards; set until Stripe has one again.
    pub needs_payment_method: bool,
    /// The organization this is a client workspace of (it owns it and sets its limits).
    pub organization_id: Option<Uuid>,
    /// Most switchers this client may have, within the organization's pool.
    pub switcher_allocation: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[tracing::instrument(skip(pool))]
pub async fn create(pool: &PgPool, user_id: Uuid, name: &str, url_slug: &str) -> Result<Workspace> {
    let mut tx = pool.begin().await?;
    let workspace = insert(&mut tx, user_id, name, url_slug).await?;
    tx.commit().await?;

    Ok(workspace)
}

/// Creates a workspace unless the user already owns `max_owned`; `None` then.
/// Locks the user's row, so two requests at once can't both squeeze in.
#[tracing::instrument(skip(pool))]
pub async fn create_within_limit(
    pool: &PgPool,
    user_id: Uuid,
    name: &str,
    url_slug: &str,
    max_owned: i64,
) -> Result<Option<Workspace>> {
    let mut tx = pool.begin().await?;

    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR UPDATE")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let owned = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM workspace_members WHERE user_id = $1 AND role = 'owner'",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    if owned >= max_owned {
        return Ok(None);
    }

    let workspace = insert(&mut tx, user_id, name, url_slug).await?;
    tx.commit().await?;

    Ok(Some(workspace))
}

/// Inserts a workspace with its creator as the owner.
async fn insert(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    name: &str,
    url_slug: &str,
) -> Result<Workspace> {
    let workspace = sqlx::query_as::<_, Workspace>(
        "
        INSERT INTO workspaces (
            created_by,
            name,
            url_slug
        )
        VALUES ($1, $2, $3)
        RETURNING *
        ",
    )
    .bind(user_id)
    .bind(name)
    .bind(url_slug)
    .fetch_one(&mut **tx)
    .await?;

    add_member(&mut **tx, workspace.id, user_id, WorkspaceRoleEnum::Owner).await?;

    Ok(workspace)
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WorkspaceMember {
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub role: WorkspaceRoleEnum,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Returns whether `user_id` has `permission` in `workspace_id`, as a member or
/// (in a client workspace) through the organization's team.
///
/// Permissions are intentionally resolved from the database rather than inferred
/// from role rank, so changes to `workspace_role_permissions` take effect without
/// changing application code.
#[tracing::instrument(skip(pool))]
pub async fn has_permission(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    permission: WorkspacePermission,
) -> Result<bool> {
    let access = member_access(pool, workspace_id, user_id, permission).await?;
    Ok(access.allowed || access.organization_allowed)
}

/// What a request from a member needs to know, in one query.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct MemberAccess {
    /// The user is a member whose role has the permission.
    pub allowed: bool,
    /// Why staff suspended the workspace (or its organization), if they did.
    pub suspended_reason: Option<String>,
    /// The suspension is the organization's, not the workspace's own.
    pub suspended_by_organization: bool,
    /// The user is on the team of the organization that owns this client workspace,
    /// and their organization role has the permission there.
    pub organization_allowed: bool,
    /// The organization that owns the workspace, if it's a client workspace.
    pub organization_id: Option<Uuid>,
    pub organization_name: Option<String>,
}

/// [`has_permission`] together with the workspace's suspension and the user's
/// access through its organization.
#[tracing::instrument(skip(pool))]
pub async fn member_access(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    permission: WorkspacePermission,
) -> Result<MemberAccess> {
    let access = sqlx::query_as::<_, MemberAccess>(
        "
        SELECT
            EXISTS (
                SELECT 1
                FROM workspace_members AS members
                JOIN workspace_role_permissions AS role_permissions
                  ON role_permissions.role = members.role
                WHERE members.workspace_id = $1
                  AND members.user_id = $2
                  AND role_permissions.permission = $3
            ) AS allowed,
            (
                SELECT COALESCE(workspaces.suspended_reason, organizations.suspended_reason)
                FROM workspaces
                LEFT JOIN organizations ON organizations.id = workspaces.organization_id
                WHERE workspaces.id = $1
                  AND COALESCE(workspaces.suspended_at, organizations.suspended_at) IS NOT NULL
            ) AS suspended_reason,
            COALESCE((
                SELECT workspaces.suspended_at IS NULL AND organizations.suspended_at IS NOT NULL
                FROM workspaces
                LEFT JOIN organizations ON organizations.id = workspaces.organization_id
                WHERE workspaces.id = $1
            ), false) AS suspended_by_organization,
            EXISTS (
                SELECT 1
                FROM workspaces
                JOIN organization_members AS members
                  ON members.organization_id = workspaces.organization_id
                JOIN organization_role_permissions AS role_permissions
                  ON role_permissions.role = members.role
                WHERE workspaces.id = $1
                  AND members.user_id = $2
                  AND role_permissions.permission = $3
            ) AS organization_allowed,
            organizations.id AS organization_id,
            organizations.name AS organization_name
        FROM (SELECT 1) AS one
        LEFT JOIN workspaces ON workspaces.id = $1
        LEFT JOIN organizations ON organizations.id = workspaces.organization_id
        ",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(permission.as_str())
    .fetch_one(pool)
    .await?;

    Ok(access)
}

#[tracing::instrument(skip(pool))]
pub async fn get_by_id(pool: &PgPool, workspace_id: Uuid) -> Result<Option<Workspace>> {
    let workspace = sqlx::query_as::<_, Workspace>("SELECT * FROM workspaces WHERE id = $1")
        .bind(workspace_id)
        .fetch_optional(pool)
        .await?;

    Ok(workspace)
}

#[tracing::instrument(skip(pool))]
pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Workspace>> {
    let workspaces = sqlx::query_as::<_, Workspace>(
        "
        SELECT workspaces.*
        FROM workspaces
        JOIN workspace_members ON workspace_members.workspace_id = workspaces.id
        WHERE workspace_members.user_id = $1
        ORDER BY workspaces.created_at
        ",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(workspaces)
}

#[tracing::instrument(skip(pool))]
pub async fn list(pool: &PgPool) -> Result<Vec<Workspace>> {
    let workspaces = sqlx::query_as::<_, Workspace>("SELECT * FROM workspaces ORDER BY created_at")
        .fetch_all(pool)
        .await?;

    Ok(workspaces)
}

#[tracing::instrument(skip(pool))]
pub async fn list_members(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<WorkspaceMember>> {
    let members = sqlx::query_as::<_, WorkspaceMember>(
        "SELECT * FROM workspace_members WHERE workspace_id = $1 ORDER BY created_at",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;

    Ok(members)
}

pub async fn add_member<'a>(
    pool: impl sqlx::PgExecutor<'a>,
    workspace_id: Uuid,
    user_id: Uuid,
    role: WorkspaceRoleEnum,
) -> Result<WorkspaceMember> {
    let member = sqlx::query_as::<_, WorkspaceMember>(
        "
        INSERT INTO workspace_members (
            workspace_id,
            user_id,
            role
        )
        VALUES ($1, $2, $3) 
        RETURNING *
        ",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .fetch_one(pool)
    .await?;

    Ok(member)
}

/// A workspace as listed for a user, with their role in it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WorkspaceSummary {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    pub plan_code: String,
    /// `None` when a staff user sees a workspace they aren't a member of.
    pub role: Option<WorkspaceRoleEnum>,
    pub switcher_count: i64,
    pub created_at: DateTime<Utc>,
    /// Suspended by staff: switchers paused, chat quiet.
    pub suspended_at: Option<DateTime<Utc>>,
    pub suspended_reason: Option<String>,
    /// The suspension is the organization's, not the workspace's own.
    pub suspended_by_organization: bool,
    /// Set for a client workspace: the organization that owns and manages it.
    pub organization_id: Option<Uuid>,
    pub organization_name: Option<String>,
    pub organization_slug: Option<String>,
    /// `user_id`'s role in that organization, when they are in it.
    pub organization_role: Option<OrganizationRole>,
    /// A client workspace's switcher allocation.
    pub switcher_allocation: Option<i32>,
}

/// The select behind [`WorkspaceSummary`], kept as a macro so queries stay
/// `&'static str`. `$1` is the user whose role is looked up.
macro_rules! summary_select {
    () => {
        "
    SELECT
        workspaces.id,
        workspaces.name,
        workspaces.url_slug,
        CASE WHEN workspaces.organization_id IS NULL THEN workspaces.plan_code ELSE 'hosted' END AS plan_code,
        members.role,
        (SELECT count(*) FROM switchers WHERE switchers.workspace_id = workspaces.id) AS switcher_count,
        workspaces.created_at,
        COALESCE(workspaces.suspended_at, organizations.suspended_at) AS suspended_at,
        COALESCE(workspaces.suspended_reason, organizations.suspended_reason) AS suspended_reason,
        (workspaces.suspended_at IS NULL AND organizations.suspended_at IS NOT NULL)
            AS suspended_by_organization,
        workspaces.organization_id,
        organizations.name AS organization_name,
        organizations.url_slug AS organization_slug,
        organization_members.role AS organization_role,
        workspaces.switcher_allocation
    FROM workspaces
    LEFT JOIN workspace_members AS members
      ON members.workspace_id = workspaces.id AND members.user_id = $1
    LEFT JOIN organizations ON organizations.id = workspaces.organization_id
    LEFT JOIN organization_members
      ON organization_members.organization_id = workspaces.organization_id
     AND organization_members.user_id = $1
"
    };
}

/// Lists the workspaces `user_id` is a member of (staff too: they find the
/// others under Admin).
#[tracing::instrument(skip(pool))]
pub async fn list_summaries(pool: &PgPool, user_id: Uuid) -> Result<Vec<WorkspaceSummary>> {
    let workspaces = sqlx::query_as::<_, WorkspaceSummary>(concat!(
        summary_select!(),
        " WHERE members.user_id IS NOT NULL ORDER BY workspaces.created_at"
    ))
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(workspaces)
}

/// Which of a user's workspaces to list, and in what order.
#[derive(Debug, Clone, Copy, Default)]
pub struct MemberFilter<'a> {
    /// Part of the name or URL.
    pub search: Option<&'a str>,
    /// Only where the user has this role.
    pub role: Option<WorkspaceRoleEnum>,
    /// By name instead of the oldest first.
    pub by_name: bool,
}

/// A page of the workspaces `user_id` is a member of, and how many match in all. For a
/// person who is in a lot of them: the menu lists them all, this one is searched and paged.
#[tracing::instrument(skip(pool, filter))]
pub async fn list_summaries_page(
    pool: &PgPool,
    user_id: Uuid,
    filter: MemberFilter<'_>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<WorkspaceSummary>, i64)> {
    let workspaces = sqlx::query_as::<_, WorkspaceSummary>(concat!(
        summary_select!(),
        " WHERE members.user_id IS NOT NULL
            AND ($2::text IS NULL OR workspaces.name ILIKE '%' || $2 || '%'
                 OR workspaces.url_slug ILIKE '%' || $2 || '%')
            AND ($3::text IS NULL OR members.role = $3)
          ORDER BY (CASE WHEN $4 THEN lower(workspaces.name) END), workspaces.created_at,
                   workspaces.id
          LIMIT $5 OFFSET $6"
    ))
    .bind(user_id)
    .bind(filter.search)
    .bind(filter.role)
    .bind(filter.by_name)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)
         FROM workspaces
         JOIN workspace_members AS members
           ON members.workspace_id = workspaces.id AND members.user_id = $1
         WHERE ($2::text IS NULL OR workspaces.name ILIKE '%' || $2 || '%'
                OR workspaces.url_slug ILIKE '%' || $2 || '%')
           AND ($3::text IS NULL OR members.role = $3)",
    )
    .bind(user_id)
    .bind(filter.search)
    .bind(filter.role)
    .fetch_one(pool)
    .await?;

    Ok((workspaces, total))
}

/// A switcher in a workspace the user is a member of, named for lists across them.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemberSwitcher {
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
    pub role: WorkspaceRoleEnum,
    pub switcher_id: Uuid,
    pub switcher_name: String,
}

/// The most switchers read across one person's workspaces at once.
pub const MAX_MEMBER_SWITCHERS: i64 = 5_000;

/// The switchers of the workspaces `user_id` is a member of (at most
/// [`MAX_MEMBER_SWITCHERS`]), or only those of `workspace_ids` when given.
#[tracing::instrument(skip(pool, workspace_ids))]
pub async fn member_switchers(
    pool: &PgPool,
    user_id: Uuid,
    workspace_ids: Option<&[Uuid]>,
) -> Result<Vec<MemberSwitcher>> {
    let switchers = sqlx::query_as::<_, MemberSwitcher>(
        "SELECT workspaces.id AS workspace_id, workspaces.name AS workspace_name,
                workspaces.url_slug AS workspace_slug, members.role,
                switchers.id AS switcher_id, switchers.name AS switcher_name
         FROM switchers
         JOIN workspaces ON workspaces.id = switchers.workspace_id
         JOIN workspace_members AS members
           ON members.workspace_id = workspaces.id AND members.user_id = $1
         WHERE ($2::uuid[] IS NULL OR workspaces.id = ANY($2))
         ORDER BY workspaces.id, switchers.id
         LIMIT $3",
    )
    .bind(user_id)
    .bind(workspace_ids)
    .bind(MAX_MEMBER_SWITCHERS)
    .fetch_all(pool)
    .await?;

    Ok(switchers)
}

/// One workspace by its URL slug, with `user_id`'s role in it (`None` if they
/// aren't a member).
#[tracing::instrument(skip(pool))]
pub async fn get_summary_by_slug(
    pool: &PgPool,
    url_slug: &str,
    user_id: Uuid,
) -> Result<Option<WorkspaceSummary>> {
    let workspace = sqlx::query_as::<_, WorkspaceSummary>(concat!(
        summary_select!(),
        " WHERE workspaces.url_slug = $2"
    ))
    .bind(user_id)
    .bind(url_slug)
    .fetch_optional(pool)
    .await?;

    Ok(workspace)
}

/// One workspace with `user_id`'s role in it.
#[tracing::instrument(skip(pool))]
pub async fn get_summary(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<WorkspaceSummary>> {
    let workspace = sqlx::query_as::<_, WorkspaceSummary>(concat!(
        summary_select!(),
        " WHERE workspaces.id = $2"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;

    Ok(workspace)
}

#[tracing::instrument(skip(pool))]
pub async fn slug_exists(pool: &PgPool, url_slug: &str) -> Result<bool> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM workspaces WHERE url_slug = $1)",
    )
    .bind(url_slug)
    .fetch_one(pool)
    .await?;

    Ok(exists)
}

#[tracing::instrument(skip(pool))]
pub async fn rename(pool: &PgPool, workspace_id: Uuid, name: &str) -> Result<Option<Workspace>> {
    let workspace =
        sqlx::query_as::<_, Workspace>("UPDATE workspaces SET name = $2 WHERE id = $1 RETURNING *")
            .bind(workspace_id)
            .bind(name)
            .fetch_optional(pool)
            .await?;

    Ok(workspace)
}

/// Sets the plan a workspace is entitled to. `plan_code` must exist in `plans`.
/// A client workspace has none (its organization sets the limits): `None`.
#[tracing::instrument(skip(pool))]
pub async fn set_plan(
    pool: &PgPool,
    workspace_id: Uuid,
    plan_code: &str,
) -> Result<Option<Workspace>> {
    let workspace = sqlx::query_as::<_, Workspace>(
        "UPDATE workspaces SET plan_code = $2 WHERE id = $1 AND organization_id IS NULL RETURNING *",
    )
    .bind(workspace_id)
    .bind(plan_code)
    .fetch_optional(pool)
    .await?;

    Ok(workspace)
}

/// A member together with the profile of their most recently used login.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemberProfile {
    pub user_id: Uuid,
    pub role: WorkspaceRoleEnum,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    pub provider: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Lists members, owner first, then by role and join date.
#[tracing::instrument(skip(pool))]
pub async fn list_member_profiles(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<MemberProfile>> {
    let members = sqlx::query_as::<_, MemberProfile>(
        "
        SELECT
            members.user_id,
            members.role,
            profile.provider_username AS username,
            profile.provider_avatar_url AS avatar_url,
            profile.provider,
            members.created_at
        FROM workspace_members AS members
        JOIN workspace_roles AS roles ON roles.role = members.role
        LEFT JOIN LATERAL (
            SELECT provider::text, provider_username, provider_avatar_url
            FROM oauth2_connections
            WHERE oauth2_connections.user_id = members.user_id
            ORDER BY updated_at DESC
            LIMIT 1
        ) AS profile ON true
        WHERE members.workspace_id = $1
        ORDER BY roles.rank DESC, members.created_at
        ",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;

    Ok(members)
}

#[tracing::instrument(skip(pool))]
pub async fn member_role(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<WorkspaceRoleEnum>> {
    let role = sqlx::query_scalar::<_, WorkspaceRoleEnum>(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(role)
}

pub async fn member_count<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    workspace_id: Uuid,
) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM workspace_members WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_one(executor)
    .await?;

    Ok(count)
}

#[tracing::instrument(skip(pool))]
pub async fn update_member_role(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: WorkspaceRoleEnum,
) -> Result<Option<WorkspaceMember>> {
    let member = sqlx::query_as::<_, WorkspaceMember>(
        "
        UPDATE workspace_members SET role = $3
        WHERE workspace_id = $1 AND user_id = $2
        RETURNING *
        ",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .fetch_optional(pool)
    .await?;

    Ok(member)
}

/// Removes a member. Returns whether they were a member.
#[tracing::instrument(skip(pool))]
pub async fn remove_member(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> Result<bool> {
    let result =
        sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
            .bind(workspace_id)
            .bind(user_id)
            .execute(pool)
            .await?;

    Ok(result.rows_affected() > 0)
}

/// Deletes a workspace and everything in it (members, invites, switchers and
/// their history, subscriptions). `false` if it didn't exist.
#[tracing::instrument(skip(pool))]
pub async fn delete(pool: &PgPool, workspace_id: Uuid) -> Result<bool> {
    let deleted = sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(workspace_id)
        .execute(pool)
        .await?;
    Ok(deleted.rows_affected() > 0)
}

/// A workspace a user owns, and how many other people are in it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OwnedWorkspace {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    pub stripe_customer_id: Option<String>,
    pub other_members: i64,
}

/// The workspaces `user_id` owns, oldest first.
#[tracing::instrument(skip(pool))]
pub async fn owned_by(pool: &PgPool, user_id: Uuid) -> Result<Vec<OwnedWorkspace>> {
    let workspaces = sqlx::query_as::<_, OwnedWorkspace>(
        "SELECT workspaces.id, workspaces.name, workspaces.url_slug, workspaces.stripe_customer_id,
                (SELECT count(*) FROM workspace_members others
                 WHERE others.workspace_id = workspaces.id AND others.user_id <> $1) AS other_members
         FROM workspaces
         JOIN workspace_members owner
           ON owner.workspace_id = workspaces.id AND owner.user_id = $1 AND owner.role = 'owner'
         ORDER BY workspaces.created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(workspaces)
}

/// Makes `new_owner` (a member) the owner; the current owner becomes an admin.
/// `needs_payment_method` is stored with it (the previous owner's cards were
/// removed). Returns `false`, changing nothing, when `new_owner` isn't a member.
#[tracing::instrument(skip(pool))]
pub async fn transfer_ownership(
    pool: &PgPool,
    workspace_id: Uuid,
    new_owner: Uuid,
    needs_payment_method: bool,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT 1 FROM workspaces WHERE id = $1 FOR UPDATE")
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;
    // One owner per workspace: step the old one down first.
    sqlx::query(
        "UPDATE workspace_members SET role = 'admin'
         WHERE workspace_id = $1 AND role = 'owner' AND user_id <> $2",
    )
    .bind(workspace_id)
    .bind(new_owner)
    .execute(&mut *tx)
    .await?;
    let promoted = sqlx::query(
        "UPDATE workspace_members SET role = 'owner' WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(new_owner)
    .execute(&mut *tx)
    .await?;
    if promoted.rows_affected() == 0 {
        return Ok(false);
    }
    sqlx::query("UPDATE workspaces SET needs_payment_method = $2 WHERE id = $1")
        .bind(workspace_id)
        .bind(needs_payment_method)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Whether the transfer flag is set (see [`transfer_ownership`]).
#[tracing::instrument(skip(pool))]
pub async fn set_needs_payment_method(
    pool: &PgPool,
    workspace_id: Uuid,
    needed: bool,
) -> Result<()> {
    sqlx::query("UPDATE workspaces SET needs_payment_method = $2 WHERE id = $1")
        .bind(workspace_id)
        .bind(needed)
        .execute(pool)
        .await?;
    Ok(())
}

/// Suspends the workspace (`Some(reason)`) or lifts the suspension (`None`).
/// `false` if there's no such workspace.
#[tracing::instrument(skip(pool, reason))]
pub async fn set_suspension(
    pool: &PgPool,
    workspace_id: Uuid,
    reason: Option<&str>,
) -> Result<bool> {
    let updated = sqlx::query(
        "UPDATE workspaces
         SET suspended_at = CASE WHEN $2::text IS NULL THEN NULL ELSE COALESCE(suspended_at, now()) END,
             suspended_reason = $2
         WHERE id = $1",
    )
    .bind(workspace_id)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(updated.rows_affected() > 0)
}

/// Whether the workspace is suspended, and why.
#[tracing::instrument(skip(pool))]
pub async fn suspension(pool: &PgPool, workspace_id: Uuid) -> Result<Option<String>> {
    let reason = sqlx::query_scalar::<_, Option<String>>(
        "SELECT suspended_reason FROM workspaces WHERE id = $1 AND suspended_at IS NOT NULL",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;

    Ok(reason.flatten())
}
