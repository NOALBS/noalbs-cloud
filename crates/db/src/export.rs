//! "Download my data" (GDPR access and portability): everything NOALBS Cloud
//! keeps about a user. [`account`] is one document: the account, its sign-ins,
//! workspaces with their switchers and billing, and every record that names
//! the user. [`history`] streams the user's switcher history, including Connect
//! logs, one JSON object per line, as it can run to hundreds of megabytes.
//!
//! Left out: secrets (passwords, API keys, the BELABOX remote key, chat tokens,
//! hashed invite and connect tokens), other people's data and what isn't
//! stored. `tests/export.rs` fails when a table or a reference to `users` is
//! added without deciding how it's exported.

use std::io::Read;

use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use futures_util::{Stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, postgres::PgRow, types::Json};
use uuid::Uuid;

use crate::error::{Error, Result};

/// Switcher columns that are secrets, left out of its settings. Every binary
/// (encrypted or hashed) column has to be here (`tests/export.rs`).
pub const SWITCHER_SECRETS: &[&str] = &[
    "connect_token_hash",
    "password",
    "belabox_remote_key",
    "moblin_password",
];

/// Encrypted values in a stream server's `details`, left out.
pub const STREAM_SERVER_SECRETS: &[&str] = &["encrypted_password", "encrypted_api_key"];

/// Whether a workspace's switchers are the user's own (`members` is their
/// membership): they own it, or an organization manages it for them (its
/// customers join as admins). Other workspaces' switchers, settings and history
/// belong to their owners.
macro_rules! yours {
    () => {
        "(members.role = 'owner' OR (workspaces.organization_id IS NOT NULL AND members.role = 'admin'))"
    };
}

/// The user's own switchers (see [`yours!`]), for `$1` = the user.
macro_rules! your_switchers {
    () => {
        concat!(
            "SELECT switchers.id FROM switchers
             JOIN workspaces ON workspaces.id = switchers.workspace_id
             JOIN workspace_members AS members ON members.workspace_id = workspaces.id
             WHERE members.user_id = $1 AND ",
            yours!()
        )
    };
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountExport {
    pub switcher_preferences: Vec<crate::switcher_preferences::Preferences>,
    pub account: Account,
    /// The Twitch, Kick and YouTube accounts you sign in with.
    pub connections: Vec<Connection>,
    /// Browsers signed in to your account.
    pub sessions: Vec<Session>,
    /// Workspaces you're in or created. Switchers are listed for your own (see
    /// `switchers`), billing for those you own.
    pub workspaces: Vec<Workspace>,
    /// Organizations whose team you're on.
    pub organizations: Vec<Organization>,
    /// Invites you made or accepted.
    pub invites: Vec<Invite>,
    /// Connect devices you paired.
    pub connect_devices: Vec<ConnectDevice>,
    /// Connect pairings you confirmed and that are waiting to be claimed.
    pub connect_pairings: Vec<ConnectPairing>,
    pub chat: Chat,
    /// NOALBS support access you gave (or, as staff, opened).
    pub support_grants: Vec<SupportGrant>,
    /// What you did in organizations, from their audit logs.
    pub organization_audit: Vec<OrganizationAuditEntry>,
    /// What NOALBS staff did about your account (or, as staff, did yourself).
    pub staff_actions: Vec<StaffAction>,
    /// Only for NOALBS staff: their support work and what they set up.
    pub staff_work: StaffWork,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Account {
    pub id: Uuid,
    pub staff: bool,
    pub whitelisted: bool,
    /// Set by staff: the account can't sign in.
    pub disabled_at: Option<DateTime<Utc>>,
    pub disabled_reason: Option<String>,
    /// The workspace the dashboard opens.
    pub last_workspace_id: Option<Uuid>,
    /// The newest "What's new" post seen in the dashboard.
    pub whats_new_seen: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Connection {
    pub provider: String,
    /// The account's ID on the platform.
    pub provider_id: String,
    pub username: String,
    /// No longer asked for; always empty since September 2026.
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    /// What the platform let us do when you signed in.
    pub scopes: Vec<String>,
    pub connected_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Session {
    pub provider: String,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    /// `None` when you created it but are no longer a member.
    pub role: Option<String>,
    pub joined_at: Option<DateTime<Utc>>,
    pub created_by_you: bool,
    /// The organization that manages this client workspace, if it is one.
    pub managed_by: Option<String>,
    /// Not for a client workspace: the organization's limits apply.
    pub plan: Option<String>,
    pub suspended_at: Option<DateTime<Utc>>,
    pub suspended_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip)]
    pub yours: bool,
    /// For workspaces you own.
    #[sqlx(skip)]
    pub billing: Option<Billing>,
    /// For your own workspaces: those you own, or an organization manages for
    /// you. Other workspaces' switchers belong to their owners.
    #[sqlx(skip)]
    pub switchers: Vec<Switcher>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Billing {
    pub stripe_customer_id: Option<String>,
    pub needs_payment_method: bool,
    #[sqlx(skip)]
    pub subscriptions: Vec<Subscription>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Subscription {
    pub stripe_subscription_id: String,
    pub billing_interval: String,
    pub status: String,
    pub current_period_start: Option<DateTime<Utc>>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<SubscriptionItem>))]
    pub items: Json<Vec<SubscriptionItem>>,
    /// What stays paid for until the end of a period after it was lowered.
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<SubscriptionHold>))]
    pub holds: Json<Vec<SubscriptionHold>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubscriptionItem {
    pub kind: String,
    pub code: String,
    pub quantity: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SubscriptionHold {
    pub kind: String,
    pub code: String,
    pub quantity: i32,
    pub until: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Switcher {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    /// Every setting as stored (scenes by ID, triggers, behaviour, chat, BELABOX
    /// and Moblin options), without its secrets.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub settings: serde_json::Value,
    pub has_obs_password: bool,
    pub has_belabox_remote_key: bool,
    pub has_moblin_password: bool,
    /// Each stream server as stored, stats URL and stream key included, without
    /// its encrypted password and API key.
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<Object>))]
    pub stream_servers: serde_json::Value,
    /// Each server combination as stored, with its servers' IDs.
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<Object>))]
    pub scene_rules: serde_json::Value,
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<LinkedChannel>))]
    pub chat_channels: Json<Vec<LinkedChannel>>,
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<CommandOverride>))]
    pub command_overrides: Json<Vec<CommandOverride>>,
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<CommandAlias>))]
    pub command_aliases: Json<Vec<CommandAlias>>,
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<IgnoredChatter>))]
    pub ignored_chatters: Json<Vec<IgnoredChatter>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LinkedChannel {
    pub platform: String,
    pub channel_id: String,
    pub login: String,
    pub display_name: String,
    pub status: String,
    pub scopes: Vec<String>,
    pub auto_switch_notification: bool,
    pub linked_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CommandOverride {
    pub command: String,
    pub enabled: bool,
    pub min_role: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CommandAlias {
    pub alias: String,
    pub command: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct IgnoredChatter {
    pub platform: String,
    pub user_id: String,
    pub username: String,
    pub ignored_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Invite {
    /// `workspace` or `organization`.
    pub kind: String,
    /// The workspace's or organization's name.
    pub name: String,
    pub role: String,
    pub created_by_you: bool,
    pub accepted_by_you: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConnectDevice {
    pub switcher_id: Uuid,
    pub name: String,
    pub os: String,
    pub app_version: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConnectPairing {
    pub switcher_id: Option<Uuid>,
    pub name: String,
    pub os: String,
    pub app_version: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Chat {
    /// Channels you last let the bot into.
    pub channels_authorized: Vec<AuthorizedChannel>,
    /// Channels you linked to switchers.
    pub links: Vec<ChannelLink>,
    /// Bot accounts you connected.
    pub bots: Vec<ConnectedBot>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AuthorizedChannel {
    pub platform: String,
    pub channel_id: String,
    pub login: String,
    pub display_name: String,
    pub status: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChannelLink {
    pub workspace: String,
    pub switcher: String,
    pub platform: String,
    pub login: String,
    pub linked_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConnectedBot {
    pub platform: String,
    pub platform_user_id: String,
    pub login: String,
    pub display_name: String,
    pub status: String,
    pub scopes: Vec<String>,
    /// The workspace or organization it answers for; `None` for NOALBS's shared bot.
    pub answers_for: Option<String>,
    pub connected_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SupportGrant {
    pub workspace: String,
    pub level: String,
    pub break_glass: bool,
    pub reason: Option<String>,
    pub granted_by_you: bool,
    pub requested_by_you: bool,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OrganizationAuditEntry {
    pub organization: String,
    /// Your name as it was then.
    pub actor_name: String,
    /// `team`, or `customer` acting in their client workspace.
    pub actor_kind: String,
    pub action: String,
    pub subject: Option<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub details: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StaffAction {
    pub action: String,
    pub about_you: bool,
    pub by_you: bool,
    /// Your name as it was then, when you did it. Other staff members' names
    /// are left out.
    pub staff_name: Option<String>,
    pub workspace_name: Option<String>,
    pub switcher_name: Option<String>,
    pub reason: Option<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub details: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StaffWork {
    /// Requests you made in workspaces you aren't a member of.
    pub support_access: Vec<SupportAccess>,
    pub notices_created: Vec<NoticeCreated>,
    pub organizations_created: Vec<OrganizationCreated>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SupportAccess {
    pub workspace: String,
    pub method: String,
    pub route: String,
    pub switcher_id: Option<Uuid>,
    /// Without the workspace's permission (break-glass).
    pub without_grant: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct NoticeCreated {
    pub message: String,
    pub severity: String,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OrganizationCreated {
    pub name: String,
    pub url_slug: String,
    pub created_at: DateTime<Utc>,
}

/// Everything about `user_id` but its stream history; `None` if there's no such user.
#[tracing::instrument(skip(pool))]
pub async fn account(pool: &PgPool, user_id: Uuid) -> Result<Option<AccountExport>> {
    let Some(account) = sqlx::query_as::<_, Account>(
        "SELECT id, staff, whitelisted, disabled_at, disabled_reason, last_workspace_id,
                whats_new_seen, created_at, updated_at
         FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };

    let mut workspaces: Vec<Workspace> = all(
        pool,
        user_id,
        concat!(
            "SELECT workspaces.id, workspaces.name, workspaces.url_slug,
                    members.role, members.created_at AS joined_at,
                    COALESCE(workspaces.created_by = $1, false) AS created_by_you,
                    organizations.name AS managed_by,
                    CASE WHEN workspaces.organization_id IS NULL THEN workspaces.plan_code END AS plan,
                    workspaces.suspended_at, workspaces.suspended_reason, workspaces.created_at,
                    COALESCE(",
            yours!(),
            ", false) AS yours
             FROM workspaces
             LEFT JOIN workspace_members AS members
                 ON members.workspace_id = workspaces.id AND members.user_id = $1
             LEFT JOIN organizations ON organizations.id = workspaces.organization_id
             WHERE members.user_id IS NOT NULL OR workspaces.created_by = $1
             ORDER BY workspaces.created_at"
        ),
    )
    .await?;
    for workspace in &mut workspaces {
        if workspace.yours {
            workspace.switchers = switchers(pool, workspace.id).await?;
        }
        if workspace.role.as_deref() == Some("owner") {
            workspace.billing = Some(billing(pool, workspace.id).await?);
        }
    }

    Ok(Some(AccountExport {
        switcher_preferences: crate::switcher_preferences::for_user(pool, user_id).await?,
        account,
        connections: all(
            pool,
            user_id,
            "SELECT provider::text AS provider, provider_id, provider_username AS username,
                    provider_email AS email, provider_avatar_url AS avatar_url, scopes,
                    created_at AS connected_at, updated_at
             FROM oauth2_connections WHERE user_id = $1 ORDER BY created_at",
        )
        .await?,
        // Expired sessions too, until the hourly upkeep deletes them.
        sessions: all(
            pool,
            user_id,
            "SELECT connections.provider::text AS provider, sessions.user_agent,
                    sessions.created_at, sessions.updated_at AS last_used_at, sessions.expires_at
             FROM user_sessions AS sessions
             JOIN oauth2_connections AS connections
                 ON connections.id = sessions.oauth2_connection_id
             WHERE connections.user_id = $1
             ORDER BY sessions.created_at",
        )
        .await?,
        workspaces,
        organizations: all(
            pool,
            user_id,
            "SELECT organizations.id, organizations.name, organizations.url_slug,
                    members.role, members.created_at AS joined_at
             FROM organization_members AS members
             JOIN organizations ON organizations.id = members.organization_id
             WHERE members.user_id = $1
             ORDER BY members.created_at",
        )
        .await?,
        invites: all(
            pool,
            user_id,
            "SELECT 'workspace' AS kind, workspaces.name, invites.role,
                    COALESCE(invites.invited_by = $1, false) AS created_by_you,
                    COALESCE(invites.accepted_by = $1, false) AS accepted_by_you,
                    invites.created_at, invites.expires_at, invites.accepted_at, invites.revoked_at
             FROM workspace_invites AS invites
             JOIN workspaces ON workspaces.id = invites.workspace_id
             WHERE invites.invited_by = $1 OR invites.accepted_by = $1
             UNION ALL
             SELECT 'organization', organizations.name, invites.role,
                    COALESCE(invites.invited_by = $1, false),
                    COALESCE(invites.accepted_by = $1, false),
                    invites.created_at, invites.expires_at, invites.accepted_at, invites.revoked_at
             FROM organization_invites AS invites
             JOIN organizations ON organizations.id = invites.organization_id
             WHERE invites.invited_by = $1 OR invites.accepted_by = $1
             ORDER BY created_at",
        )
        .await?,
        connect_devices: all(
            pool,
            user_id,
            "SELECT switcher_id, name, os, app_version, created_at, last_seen_at, revoked_at
             FROM connect_devices WHERE created_by = $1 ORDER BY created_at",
        )
        .await?,
        connect_pairings: all(
            pool,
            user_id,
            "SELECT switcher_id, name, os, app_version, created_at, expires_at
             FROM connect_pairings WHERE confirmed_by = $1 ORDER BY created_at",
        )
        .await?,
        chat: Chat {
            channels_authorized: all(
                pool,
                user_id,
                "SELECT platform::text AS platform, platform_channel_id AS channel_id, login,
                        display_name, status, scopes, created_at, updated_at
                 FROM chat_channels WHERE authorized_by = $1 ORDER BY created_at",
            )
            .await?,
            links: all(
                pool,
                user_id,
                "SELECT workspaces.name AS workspace, switchers.name AS switcher,
                        channels.platform::text AS platform, channels.login,
                        links.created_at AS linked_at
                 FROM switcher_chat_channels AS links
                 JOIN switchers ON switchers.id = links.switcher_id
                 JOIN workspaces ON workspaces.id = switchers.workspace_id
                 JOIN chat_channels AS channels ON channels.id = links.chat_channel_id
                 WHERE links.linked_by = $1
                 ORDER BY links.created_at",
            )
            .await?,
            bots: all(
                pool,
                user_id,
                "SELECT bots.platform::text AS platform, bots.platform_user_id, bots.login,
                        bots.display_name, bots.status, bots.scopes,
                        COALESCE(workspaces.name, organizations.name) AS answers_for,
                        bots.created_at AS connected_at
                 FROM chat_bots AS bots
                 LEFT JOIN workspaces ON workspaces.id = bots.workspace_id
                 LEFT JOIN organizations ON organizations.id = bots.organization_id
                 WHERE bots.connected_by = $1
                 ORDER BY bots.created_at",
            )
            .await?,
        },
        support_grants: all(
            pool,
            user_id,
            "SELECT workspaces.name AS workspace, grants.level::text AS level, grants.break_glass,
                    grants.reason,
                    COALESCE(grants.granted_by = $1, false) AS granted_by_you,
                    COALESCE(grants.requested_by = $1, false) AS requested_by_you,
                    grants.expires_at, grants.revoked_at, grants.acknowledged_at, grants.created_at
             FROM support_grants AS grants
             JOIN workspaces ON workspaces.id = grants.workspace_id
             WHERE grants.granted_by = $1 OR grants.requested_by = $1
             ORDER BY grants.created_at",
        )
        .await?,
        organization_audit: all(
            pool,
            user_id,
            "SELECT organizations.name AS organization, audit.actor_name, audit.actor_kind,
                    audit.action, audit.subject, audit.details, audit.created_at
             FROM organization_audit AS audit
             JOIN organizations ON organizations.id = audit.organization_id
             WHERE audit.actor_user_id = $1
             ORDER BY audit.created_at",
        )
        .await?,
        staff_actions: all(
            pool,
            user_id,
            "SELECT action,
                    COALESCE(target_user_id = $1, false) AS about_you,
                    COALESCE(staff_user_id = $1, false) AS by_you,
                    CASE WHEN staff_user_id = $1 THEN staff_name END AS staff_name,
                    workspace_name, switcher_name, reason, details, created_at
             FROM staff_actions
             WHERE target_user_id = $1 OR staff_user_id = $1
             ORDER BY created_at",
        )
        .await?,
        staff_work: StaffWork {
            support_access: all(
                pool,
                user_id,
                "SELECT workspaces.name AS workspace, log.method, log.route, log.switcher_id,
                        log.grant_id IS NULL AS without_grant, log.created_at
                 FROM support_access_log AS log
                 JOIN workspaces ON workspaces.id = log.workspace_id
                 WHERE log.staff_user_id = $1
                 ORDER BY log.created_at",
            )
            .await?,
            notices_created: all(
                pool,
                user_id,
                "SELECT message, severity, starts_at, ends_at, created_at
                 FROM notices WHERE created_by = $1 ORDER BY created_at",
            )
            .await?,
            organizations_created: all(
                pool,
                user_id,
                "SELECT name, url_slug, created_at
                 FROM organizations WHERE created_by = $1 ORDER BY created_at",
            )
            .await?,
        },
    }))
}

/// The rows of `sql`, whose only parameter `$1` is the user.
async fn all<T>(pool: &PgPool, user_id: Uuid, sql: &'static str) -> Result<Vec<T>>
where
    T: for<'r> FromRow<'r, PgRow> + Send + Unpin,
{
    Ok(sqlx::query_as::<_, T>(sql)
        .bind(user_id)
        .fetch_all(pool)
        .await?)
}

async fn switchers(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<Switcher>> {
    // Settings leave out the secrets and what's a field of its own.
    let left_out: Vec<&str> = SWITCHER_SECRETS
        .iter()
        .chain(&[
            "id",
            "workspace_id",
            "name",
            "created_at",
            "updated_at",
            "last_seen_at",
        ])
        .copied()
        .collect();
    let switchers = sqlx::query_as::<_, Switcher>(
        "SELECT switchers.id, switchers.name, switchers.created_at, switchers.updated_at,
                switchers.last_seen_at,
                to_jsonb(switchers) - $2::text[] AS settings,
                switchers.password IS NOT NULL AS has_obs_password,
                switchers.belabox_remote_key IS NOT NULL AS has_belabox_remote_key,
                switchers.moblin_password IS NOT NULL AS has_moblin_password,
                COALESCE((
                    SELECT jsonb_agg(
                        to_jsonb(servers) - 'switcher_id'
                            || jsonb_build_object('details', servers.details - $3::text[])
                        ORDER BY servers.priority)
                    FROM stream_servers AS servers WHERE servers.switcher_id = switchers.id
                ), '[]') AS stream_servers,
                COALESCE((
                    SELECT jsonb_agg(
                        to_jsonb(rules) - 'switcher_id'
                            || jsonb_build_object('server_ids', COALESCE((
                                SELECT jsonb_agg(members.stream_server_id)
                                FROM scene_rule_servers AS members
                                WHERE members.scene_rule_id = rules.id
                            ), '[]'))
                        ORDER BY rules.priority)
                    FROM scene_rules AS rules WHERE rules.switcher_id = switchers.id
                ), '[]') AS scene_rules,
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'platform', channels.platform,
                        'channel_id', channels.platform_channel_id,
                        'login', channels.login,
                        'display_name', channels.display_name,
                        'status', channels.status,
                        'scopes', channels.scopes,
                        'auto_switch_notification', links.auto_switch_notification,
                        'linked_at', links.created_at) ORDER BY links.created_at)
                    FROM switcher_chat_channels AS links
                    JOIN chat_channels AS channels ON channels.id = links.chat_channel_id
                    WHERE links.switcher_id = switchers.id
                ), '[]') AS chat_channels,
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'command', command, 'enabled', enabled, 'min_role', min_role)
                        ORDER BY command)
                    FROM noalbs_chat_command_overrides WHERE switcher_id = switchers.id
                ), '[]') AS command_overrides,
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object('alias', alias, 'command', command)
                        ORDER BY alias)
                    FROM noalbs_chat_command_aliases WHERE switcher_id = switchers.id
                ), '[]') AS command_aliases,
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'platform', platform, 'user_id', platform_user_id,
                        'username', platform_username, 'ignored_at', created_at)
                        ORDER BY created_at)
                    FROM chat_ignored_chatters WHERE switcher_id = switchers.id
                ), '[]') AS ignored_chatters
         FROM switchers
         WHERE switchers.workspace_id = $1
         ORDER BY switchers.created_at",
    )
    .bind(workspace_id)
    .bind(left_out)
    .bind(STREAM_SERVER_SECRETS)
    .fetch_all(pool)
    .await?;
    Ok(switchers)
}

async fn billing(pool: &PgPool, workspace_id: Uuid) -> Result<Billing> {
    let mut billing = sqlx::query_as::<_, Billing>(
        "SELECT stripe_customer_id, needs_payment_method FROM workspaces WHERE id = $1",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await?;
    billing.subscriptions = sqlx::query_as::<_, Subscription>(
        "SELECT stripe_subscription_id, billing_interval, status, current_period_start,
                current_period_end, cancel_at_period_end, canceled_at, created_at,
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'kind', kind, 'code', code, 'quantity', quantity) ORDER BY kind, code)
                    FROM subscription_items WHERE subscription_id = subscriptions.id
                ), '[]') AS items,
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'kind', kind, 'code', code, 'quantity', quantity, 'until', until)
                        ORDER BY kind, code)
                    FROM subscription_holds WHERE subscription_id = subscriptions.id
                ), '[]') AS holds
         FROM subscriptions WHERE workspace_id = $1 ORDER BY created_at",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(billing)
}

/// The history of the user's own switchers (see [`yours!`]), one JSON object
/// per item: streams, stats, events, and Connect logs with their stored text.
pub fn history(pool: &PgPool, user_id: Uuid) -> impl Stream<Item = Result<String>> + Send + '_ {
    const STREAMS: &str = concat!(
        "SELECT (jsonb_build_object(
                    'type', 'stream',
                    'workspace', workspaces.name,
                    'switcher', switchers.name,
                    'sources', COALESCE((
                        SELECT jsonb_agg(to_jsonb(sources) - 'stream_id' ORDER BY sources.name)
                        FROM stream_sources AS sources WHERE sources.stream_id = streams.id
                    ), '[]'),
                    'markers', COALESCE((
                        SELECT jsonb_agg(to_jsonb(markers) - 'stream_id' ORDER BY markers.at)
                        FROM stream_markers AS markers WHERE markers.stream_id = streams.id
                    ), '[]'))
                || to_jsonb(streams))::text
         FROM streams
         JOIN switchers ON switchers.id = streams.switcher_id
         JOIN workspaces ON workspaces.id = switchers.workspace_id
         WHERE streams.switcher_id IN (",
        your_switchers!(),
        ")
         ORDER BY streams.started_at"
    );
    const STATS: &str = concat!(
        "SELECT (jsonb_build_object('type', 'stats') || to_jsonb(stats))::text
         FROM stream_stats AS stats
         JOIN streams ON streams.id = stats.stream_id
         WHERE streams.switcher_id IN (",
        your_switchers!(),
        ")
         ORDER BY stats.stream_id, stats.bucket_at, stats.server_id"
    );
    const EVENTS: &str = concat!(
        "SELECT (jsonb_build_object('type', 'event') || CASE
             WHEN events.kind = 'connect_program_command'
                  AND NOT (
                      event_members.role = 'owner'
                      OR (event_members.role = 'admin'
                          AND event_switchers.connect_programs IN ('admins', 'everyone'))
                      OR (event_members.role = 'operator'
                          AND event_switchers.connect_programs = 'everyone')
                  )
             THEN jsonb_set(
                 to_jsonb(events)
                     #- '{data,program,path}'
                     #- '{data,program,start_with_obs}'
                     #- '{data,program,stop_with_obs}'
                     #- '{data,program,close_when_idle}'
                     #- '{data,program,added_from_dashboard_by}',
                 '{data,program,args}', '[]'::jsonb, true
             )
             ELSE to_jsonb(events)
         END)::text
         FROM switcher_events AS events
         JOIN switchers AS event_switchers ON event_switchers.id = events.switcher_id
         JOIN workspace_members AS event_members
             ON event_members.workspace_id = event_switchers.workspace_id
             AND event_members.user_id = $1
         WHERE events.switcher_id IN (",
        your_switchers!(),
        ")
         ORDER BY events.switcher_id, events.created_at"
    );

    const CONNECT_LOGS: &str = concat!(
        "SELECT logs.switcher_id, logs.id, logs.kind, logs.file_name, logs.file_modified_at,
                logs.uploaded_at, logs.size_bytes, logs.truncated, device.name AS device_name,
                logs.content
         FROM connect_logs AS logs
         LEFT JOIN connect_devices AS device ON device.id = logs.device_id
         JOIN switchers AS log_switcher ON log_switcher.id = logs.switcher_id
         JOIN workspace_members AS log_members
             ON log_members.workspace_id = log_switcher.workspace_id
             AND log_members.user_id = $1
         WHERE logs.switcher_id IN (",
        your_switchers!(),
        ")
         AND logs.uploaded_at > now() - interval '30 days'
         AND (
             log_members.role = 'owner'
             OR (log_members.role = 'admin' AND log_switcher.connect_logs IN ('admins', 'everyone'))
             OR (log_members.role = 'operator' AND log_switcher.connect_logs = 'everyone')
         )
         ORDER BY logs.uploaded_at, logs.id"
    );

    let connect_logs = sqlx::query_as::<_, HistoryConnectLog>(CONNECT_LOGS)
        .bind(user_id)
        .fetch(pool)
        .map_err(Error::from)
        .and_then(|log| async move {
            let mut decoder = GzDecoder::new(log.content.as_slice());
            let mut content = Vec::new();
            decoder
                .read_to_end(&mut content)
                .map_err(|error| Error::Sqlx(sqlx::Error::Io(error)))?;
            Ok(serde_json::json!({
                "type": "connect_log",
                "switcher_id": log.switcher_id,
                "id": log.id,
                "kind": log.kind,
                "file_name": log.file_name,
                "file_modified_at": log.file_modified_at,
                "uploaded_at": log.uploaded_at,
                "size_bytes": log.size_bytes,
                "truncated": log.truncated,
                "device_name": log.device_name,
                "content": String::from_utf8_lossy(&content),
            })
            .to_string())
        });

    futures_util::stream::iter([STREAMS, STATS, EVENTS])
        .flat_map(move |sql| {
            sqlx::query_scalar::<_, String>(sql)
                .bind(user_id)
                .fetch(pool)
        })
        .map_err(Error::from)
        .chain(connect_logs)
}

#[derive(sqlx::FromRow)]
struct HistoryConnectLog {
    switcher_id: Uuid,
    id: Uuid,
    kind: String,
    file_name: String,
    file_modified_at: Option<DateTime<Utc>>,
    uploaded_at: DateTime<Utc>,
    size_bytes: i32,
    truncated: bool,
    device_name: Option<String>,
    content: Vec<u8>,
}
