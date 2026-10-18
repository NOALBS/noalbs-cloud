//! Read models for the staff admin section: the overview, and searchable lists
//! of users and workspaces.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    billing::{ItemKind, LIVE_STATUSES},
    error::Result,
};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Counts {
    pub users: i64,
    pub staff: i64,
    pub new_users_7d: i64,
    pub workspaces: i64,
    pub paying_workspaces: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PlanCount {
    pub code: String,
    pub name: String,
    pub workspaces: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AddonCount {
    pub code: String,
    pub name: String,
    /// Units billed on live subscriptions.
    pub units: i64,
}

/// Monthly recurring revenue in one currency, in its smallest unit (yearly ÷ 12).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Mrr {
    pub currency: String,
    pub amount: i64,
}

#[derive(Debug, Clone)]
pub struct Overview {
    pub counts: Counts,
    pub plans: Vec<PlanCount>,
    pub addons: Vec<AddonCount>,
    pub mrr: Vec<Mrr>,
}

#[tracing::instrument(skip(pool))]
pub async fn overview(pool: &PgPool) -> Result<Overview> {
    let counts = sqlx::query_as::<_, Counts>(
        "
        SELECT
            (SELECT count(*) FROM users) AS users,
            (SELECT count(*) FROM users WHERE staff) AS staff,
            (SELECT count(*) FROM users WHERE created_at > now() - interval '7 days') AS new_users_7d,
            (SELECT count(*) FROM workspaces) AS workspaces,
            (SELECT count(DISTINCT workspace_id) FROM subscriptions WHERE status = ANY($1))
                AS paying_workspaces
        ",
    )
    .bind(&LIVE_STATUSES[..])
    .fetch_one(pool)
    .await?;

    let plans = sqlx::query_as::<_, PlanCount>(
        "
        SELECT plans.code, plans.name, count(workspaces.id) AS workspaces
        FROM plans
        LEFT JOIN workspaces ON workspaces.plan_code = plans.code AND workspaces.organization_id IS NULL
        GROUP BY plans.code, plans.name, plans.position
        ORDER BY plans.position, plans.code
        ",
    )
    .fetch_all(pool)
    .await?;

    let addons = sqlx::query_as::<_, AddonCount>(
        "
        SELECT addons.code, addons.name, COALESCE(sum(live.quantity), 0)::int8 AS units
        FROM addons
        LEFT JOIN (
            SELECT subscription_items.code, subscription_items.quantity
            FROM subscription_items
            JOIN subscriptions ON subscriptions.id = subscription_items.subscription_id
            WHERE subscriptions.status = ANY($1) AND subscription_items.kind = 'addon'
        ) live ON live.code = addons.code
        GROUP BY addons.code, addons.name, addons.position
        ORDER BY addons.position, addons.name
        ",
    )
    .bind(&LIVE_STATUSES[..])
    .fetch_all(pool)
    .await?;

    let mrr = sqlx::query_as::<_, Mrr>(
        "
        SELECT prices.currency,
               round(sum(
                   prices.amount::numeric * subscription_items.quantity
                   / CASE prices.billing_interval WHEN 'year' THEN 12 ELSE 1 END
               ))::int8 AS amount
        FROM subscription_items
        JOIN subscriptions ON subscriptions.id = subscription_items.subscription_id
        JOIN prices ON prices.stripe_price_id = subscription_items.stripe_price_id
        WHERE subscriptions.status = ANY($1)
        GROUP BY prices.currency
        ORDER BY prices.currency
        ",
    )
    .bind(&LIVE_STATUSES[..])
    .fetch_all(pool)
    .await?;

    Ok(Overview {
        counts,
        plans,
        addons,
        mrr,
    })
}

/// A user as staff see them in lists.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub staff: bool,
    /// Disabled by staff (no sign-in), and why.
    pub disabled_at: Option<DateTime<Utc>>,
    pub disabled_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    /// From the most recently used sign-in connection.
    pub username: Option<String>,
    pub provider: Option<String>,
    pub avatar_url: Option<String>,
    pub workspaces: i64,
    /// When one of their sessions was last used, or they last signed in
    /// (sessions are deleted on sign-out and when they expire).
    pub last_active_at: Option<DateTime<Utc>>,
}

/// `users` with their `activity.last_active_at`.
macro_rules! users_with_activity {
    () => {
        "
        FROM users
        LEFT JOIN LATERAL (
            SELECT greatest(
                (SELECT max(user_sessions.updated_at)
                 FROM user_sessions
                 JOIN oauth2_connections ON oauth2_connections.id = user_sessions.oauth2_connection_id
                 WHERE oauth2_connections.user_id = users.id),
                (SELECT max(oauth2_connections.updated_at)
                 FROM oauth2_connections
                 WHERE oauth2_connections.user_id = users.id)
            ) AS last_active_at
        ) activity ON true
        "
    };
}

macro_rules! user_select {
    () => {
        concat!(
            "
            SELECT users.id, users.staff, users.disabled_at, users.disabled_reason, users.created_at,
                   profile.provider_username AS username, profile.provider, profile.provider_avatar_url AS avatar_url,
                   (SELECT count(*) FROM workspace_members WHERE workspace_members.user_id = users.id) AS workspaces,
                   activity.last_active_at
            ",
            users_with_activity!(),
            "
            LEFT JOIN LATERAL (
                SELECT provider::text, provider_username, provider_avatar_url
                FROM oauth2_connections
                WHERE oauth2_connections.user_id = users.id
                ORDER BY updated_at DESC
                LIMIT 1
            ) profile ON true
            "
        )
    };
}

/// Matches a user by any connection's username or by ID (`$1`), active since
/// `$2` and not active since `$3`.
macro_rules! user_matches {
    () => {
        "($1::text IS NULL
          OR users.id::text = $1
          OR EXISTS (SELECT 1 FROM oauth2_connections
                     WHERE oauth2_connections.user_id = users.id
                       AND oauth2_connections.provider_username ILIKE '%' || $1 || '%'))
         AND ($2::timestamptz IS NULL OR activity.last_active_at >= $2)
         AND ($3::timestamptz IS NULL OR activity.last_active_at IS NULL OR activity.last_active_at < $3)"
    };
}

/// Which users [`list_users`] returns, and in what order.
#[derive(Debug, Clone, Copy, Default)]
pub struct UserFilter<'a> {
    /// A username or ID.
    pub query: Option<&'a str>,
    /// Only users active at or after this.
    pub active_since: Option<DateTime<Utc>>,
    /// Only users not active since this (or never).
    pub inactive_since: Option<DateTime<Utc>>,
    /// Most recently active first, instead of newest first.
    pub by_activity: bool,
}

/// Users matching `filter`, and how many match.
#[tracing::instrument(skip(pool))]
pub async fn list_users(
    pool: &PgPool,
    filter: UserFilter<'_>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<UserRow>, i64)> {
    let users = sqlx::query_as::<_, UserRow>(concat!(
        user_select!(),
        " WHERE ",
        user_matches!(),
        "
        ORDER BY CASE WHEN $4 THEN activity.last_active_at END DESC NULLS LAST,
                 users.created_at DESC, users.id
        LIMIT $5 OFFSET $6
        "
    ))
    .bind(filter.query)
    .bind(filter.active_since)
    .bind(filter.inactive_since)
    .bind(filter.by_activity)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let total = sqlx::query_scalar::<_, i64>(concat!(
        "SELECT count(*) ",
        users_with_activity!(),
        " WHERE ",
        user_matches!()
    ))
    .bind(filter.query)
    .bind(filter.active_since)
    .bind(filter.inactive_since)
    .fetch_one(pool)
    .await?;
    Ok((users, total))
}

#[tracing::instrument(skip(pool))]
pub async fn get_user(pool: &PgPool, user_id: Uuid) -> Result<Option<UserRow>> {
    let user = sqlx::query_as::<_, UserRow>(concat!(user_select!(), " WHERE users.id = $1"))
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

/// Grants or removes staff. Returns whether the user exists.
#[tracing::instrument(skip(pool))]
pub async fn set_staff(pool: &PgPool, user_id: Uuid, staff: bool) -> Result<bool> {
    let result = sqlx::query("UPDATE users SET staff = $2 WHERE id = $1")
        .bind(user_id)
        .bind(staff)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// A workspace as staff see them in lists.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WorkspaceRow {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    pub plan_code: String,
    pub stripe_customer_id: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Suspended by staff, and why.
    pub suspended_at: Option<DateTime<Utc>>,
    pub suspended_reason: Option<String>,
    pub members: i64,
    pub switchers: i64,
    /// The live subscription's status, if it has one.
    pub subscription_status: Option<String>,
    pub owner_username: Option<String>,
    /// The organization this is a client workspace of.
    pub organization_id: Option<Uuid>,
    pub organization_name: Option<String>,
}

macro_rules! workspace_select {
    () => {
        "
        SELECT workspaces.id, workspaces.name, workspaces.url_slug, workspaces.plan_code,
               workspaces.stripe_customer_id, workspaces.created_at,
               workspaces.suspended_at, workspaces.suspended_reason,
               workspaces.organization_id, organizations.name AS organization_name,
               (SELECT count(*) FROM workspace_members WHERE workspace_members.workspace_id = workspaces.id)
                   AS members,
               (SELECT count(*) FROM switchers WHERE switchers.workspace_id = workspaces.id) AS switchers,
               (SELECT status FROM subscriptions
                WHERE subscriptions.workspace_id = workspaces.id AND subscriptions.status = ANY($1)
                LIMIT 1) AS subscription_status,
               (SELECT oauth2_connections.provider_username
                FROM workspace_members
                JOIN oauth2_connections ON oauth2_connections.user_id = workspace_members.user_id
                WHERE workspace_members.workspace_id = workspaces.id AND workspace_members.role = 'owner'
                ORDER BY oauth2_connections.updated_at DESC
                LIMIT 1) AS owner_username
        FROM workspaces
        LEFT JOIN organizations ON organizations.id = workspaces.organization_id
        "
    };
}

/// Workspaces matching `query` (name, slug or ID) and, when given, the client
/// workspaces of one organization, newest first, and how many match.
#[tracing::instrument(skip(pool))]
pub async fn list_workspaces(
    pool: &PgPool,
    query: Option<&str>,
    organization_id: Option<Uuid>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<WorkspaceRow>, i64)> {
    let workspaces = sqlx::query_as::<_, WorkspaceRow>(concat!(
        workspace_select!(),
        "
        WHERE ($2::text IS NULL OR workspaces.id::text = $2
               OR workspaces.name ILIKE '%' || $2 || '%' OR workspaces.url_slug ILIKE '%' || $2 || '%')
          AND ($5::uuid IS NULL OR workspaces.organization_id = $5)
        ORDER BY workspaces.created_at DESC, workspaces.id
        LIMIT $3 OFFSET $4
        "
    ))
    .bind(&LIVE_STATUSES[..])
    .bind(query)
    .bind(limit)
    .bind(offset)
    .bind(organization_id)
    .fetch_all(pool)
    .await?;
    let total = sqlx::query_scalar::<_, i64>(
        "
        SELECT count(*) FROM workspaces
        WHERE ($1::text IS NULL OR workspaces.id::text = $1
               OR workspaces.name ILIKE '%' || $1 || '%' OR workspaces.url_slug ILIKE '%' || $1 || '%')
          AND ($2::uuid IS NULL OR workspaces.organization_id = $2)
        ",
    )
    .bind(query)
    .bind(organization_id)
    .fetch_one(pool)
    .await?;
    Ok((workspaces, total))
}

#[tracing::instrument(skip(pool))]
pub async fn get_workspace(pool: &PgPool, workspace_id: Uuid) -> Result<Option<WorkspaceRow>> {
    let workspace = sqlx::query_as::<_, WorkspaceRow>(concat!(
        workspace_select!(),
        " WHERE workspaces.id = $2"
    ))
    .bind(&LIVE_STATUSES[..])
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    Ok(workspace)
}

/// How many workspaces pay for each plan or add-on (live subscriptions), for the
/// products page.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Subscribers {
    pub kind: String,
    pub code: String,
    pub workspaces: i64,
}

#[tracing::instrument(skip(pool))]
pub async fn subscribers(pool: &PgPool) -> Result<Vec<Subscribers>> {
    let rows = sqlx::query_as::<_, Subscribers>(
        "
        SELECT subscription_items.kind, subscription_items.code,
               count(DISTINCT subscriptions.workspace_id) AS workspaces
        FROM subscription_items
        JOIN subscriptions ON subscriptions.id = subscription_items.subscription_id
        WHERE subscriptions.status = ANY($1)
        GROUP BY subscription_items.kind, subscription_items.code
        ",
    )
    .bind(&LIVE_STATUSES[..])
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Why a plan or add-on can't be removed, if something still depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InUse {
    /// Workspaces are on the plan.
    Workspaces(i64),
    /// Live subscriptions pay for it or hold it until their period ends.
    Subscriptions(i64),
}

/// What keeps a plan or add-on from being removed (`None`: nothing).
#[tracing::instrument(skip(pool))]
pub async fn in_use(pool: &PgPool, kind: ItemKind, code: &str) -> Result<Option<InUse>> {
    if kind == ItemKind::Plan {
        let workspaces =
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workspaces WHERE plan_code = $1")
                .bind(code)
                .fetch_one(pool)
                .await?;
        if workspaces > 0 {
            return Ok(Some(InUse::Workspaces(workspaces)));
        }
    }
    let subscriptions = sqlx::query_scalar::<_, i64>(
        "
        SELECT count(DISTINCT subscriptions.id)
        FROM subscriptions
        WHERE subscriptions.status = ANY($1)
          AND (EXISTS (SELECT 1 FROM subscription_items
                       WHERE subscription_items.subscription_id = subscriptions.id
                         AND subscription_items.kind = $2 AND subscription_items.code = $3)
               OR EXISTS (SELECT 1 FROM subscription_holds
                          WHERE subscription_holds.subscription_id = subscriptions.id
                            AND subscription_holds.kind = $2 AND subscription_holds.code = $3
                            AND subscription_holds.until > now()))
        ",
    )
    .bind(&LIVE_STATUSES[..])
    .bind(kind.as_str())
    .bind(code)
    .fetch_one(pool)
    .await?;
    Ok((subscriptions > 0).then_some(InUse::Subscriptions(subscriptions)))
}

/// Removes a plan or add-on and its prices, if nothing depends on it.
/// Returns the Stripe price IDs that were still active, to archive them there,
/// or the reason it can't go.
#[tracing::instrument(skip(pool))]
pub async fn remove_product(
    pool: &PgPool,
    kind: ItemKind,
    code: &str,
) -> Result<std::result::Result<Vec<String>, InUse>> {
    if let Some(reason) = in_use(pool, kind, code).await? {
        return Ok(Err(reason));
    }
    // (A plan with workspaces on it can't be deleted anyway: `workspaces.plan_code`
    // restricts it.)
    let mut tx = pool.begin().await?;
    let active: Vec<String> = sqlx::query_scalar::<_, Option<String>>(
        "DELETE FROM prices WHERE kind = $1 AND code = $2 RETURNING CASE WHEN active THEN stripe_price_id END",
    )
    .bind(kind.as_str())
    .bind(code)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .flatten()
    .collect();
    let delete = match kind {
        ItemKind::Plan => "DELETE FROM plans WHERE code = $1",
        ItemKind::Addon => "DELETE FROM addons WHERE code = $1",
    };
    sqlx::query(delete).bind(code).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Ok(active))
}

/// A switcher with its workspace, for the status page.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SwitcherRef {
    pub id: Uuid,
    pub name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
    pub owner_username: Option<String>,
}

/// The switchers with these IDs (unknown IDs are left out).
#[tracing::instrument(skip(pool, ids))]
pub async fn switchers_by_ids(pool: &PgPool, ids: &[Uuid]) -> Result<Vec<SwitcherRef>> {
    let switchers = sqlx::query_as::<_, SwitcherRef>(
        "SELECT switchers.id, switchers.name, workspaces.id AS workspace_id,
                workspaces.name AS workspace_name, workspaces.url_slug AS workspace_slug,
                (SELECT oauth2_connections.provider_username
                 FROM workspace_members
                 JOIN oauth2_connections ON oauth2_connections.user_id = workspace_members.user_id
                 WHERE workspace_members.workspace_id = workspaces.id AND workspace_members.role = 'owner'
                 ORDER BY oauth2_connections.updated_at DESC
                 LIMIT 1) AS owner_username
         FROM switchers JOIN workspaces ON workspaces.id = switchers.workspace_id
         WHERE switchers.id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;
    Ok(switchers)
}

/// How much the chat bot is used, and what the chat service has recorded.
#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct ChatCounts {
    pub switchers: i64,
    pub active_channels: i64,
    pub links: i64,
    pub twitch_subscriptions: i64,
    pub kick_subscriptions: i64,
}

#[tracing::instrument(skip(pool))]
pub async fn chat_counts(pool: &PgPool) -> Result<ChatCounts> {
    let counts = sqlx::query_as::<_, ChatCounts>(
        "SELECT
            (SELECT count(*) FROM switchers) AS switchers,
            (SELECT count(*) FROM chat_channels WHERE status = 'active'
               AND EXISTS (SELECT 1 FROM switcher_chat_channels l WHERE l.chat_channel_id = chat_channels.id))
               AS active_channels,
            (SELECT count(*) FROM switcher_chat_channels) AS links,
            (SELECT count(*) FROM chat_subscriptions WHERE platform = 'TWITCH') AS twitch_subscriptions,
            (SELECT count(*) FROM chat_subscriptions WHERE platform = 'KICK') AS kick_subscriptions",
    )
    .fetch_one(pool)
    .await?;
    Ok(counts)
}

/// A chat grant the platform refused, that someone has to connect again.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RevokedGrant {
    /// `bot` or `channel`.
    pub kind: String,
    pub platform: crate::chat::ChatPlatform,
    pub login: String,
    /// The bot's workspace, or for a channel the first workspace using it;
    /// `None` for the shared bot or a channel nothing uses any more.
    pub workspace_id: Option<Uuid>,
    pub workspace_name: Option<String>,
    /// Switchers still linked to a revoked channel.
    pub switchers: i64,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[tracing::instrument(skip(pool))]
pub async fn revoked_grants(pool: &PgPool) -> Result<Vec<RevokedGrant>> {
    let grants = sqlx::query_as::<_, RevokedGrant>(
        "SELECT 'bot' AS kind, chat_bots.platform, chat_bots.login,
                workspaces.id AS workspace_id,
                COALESCE(workspaces.name, organizations.name || ' (organization)') AS workspace_name,
                0::bigint AS switchers, chat_bots.updated_at
         FROM chat_bots
         LEFT JOIN workspaces ON workspaces.id = chat_bots.workspace_id
         LEFT JOIN organizations ON organizations.id = chat_bots.organization_id
         WHERE chat_bots.status = 'revoked'
         UNION ALL
         SELECT 'channel', chat_channels.platform, chat_channels.login,
                first.workspace_id, first.workspace_name,
                (SELECT count(*) FROM switcher_chat_channels l WHERE l.chat_channel_id = chat_channels.id),
                chat_channels.updated_at
         FROM chat_channels
         LEFT JOIN LATERAL (
             SELECT workspaces.id AS workspace_id, workspaces.name AS workspace_name
             FROM switcher_chat_channels l
             JOIN switchers ON switchers.id = l.switcher_id
             JOIN workspaces ON workspaces.id = switchers.workspace_id
             WHERE l.chat_channel_id = chat_channels.id
             ORDER BY l.created_at LIMIT 1
         ) first ON true
         WHERE chat_channels.status = 'revoked'
         ORDER BY 7 DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(grants)
}
