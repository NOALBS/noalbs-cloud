//! The chat bot: channels linked to switchers, the bot accounts that speak in
//! them, and the event subscriptions the chat service keeps on the platforms.
//!
//! Which bot speaks for a link: the workspace's own bot for that platform when
//! its plan (a client workspace: its organization's setting) has `custom_chat_bot`
//! and the bot is active, else its organization's bot (client workspaces), else the
//! shared NOALBS bot. A Twitch channel's `channel:bot` grant is for our app, not for one
//! bot account, so moving between bots needs nothing from the streamer.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

/// A platform with chat. Stored as the `oauth2_provider` enum.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    sqlx::Type,
)]
#[sqlx(type_name = "oauth2_provider", rename_all = "UPPERCASE")]
#[serde(rename_all = "lowercase")]
pub enum ChatPlatform {
    Twitch,
    Kick,
    YouTube,
}

impl ChatPlatform {
    pub const ALL: [Self; 3] = [Self::Twitch, Self::Kick, Self::YouTube];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Twitch => "twitch",
            Self::Kick => "kick",
            Self::YouTube => "youtube",
        }
    }

    /// Its name as people write it ("by X (YouTube chat)").
    pub const fn label(self) -> &'static str {
        match self {
            Self::Twitch => "Twitch",
            Self::Kick => "Kick",
            Self::YouTube => "YouTube",
        }
    }
}

/// Whether a channel's or bot's grant still works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum GrantStatus {
    Active,
    /// The platform refused the grant: it has to be connected again.
    Revoked,
}

/// Who a platform account is, as its profile says.
#[derive(Debug, Clone, Copy)]
pub struct PlatformAccount<'a> {
    pub platform: ChatPlatform,
    pub id: &'a str,
    pub login: &'a str,
    pub display_name: &'a str,
}

/// A grant's tokens, already encrypted with the data key (`None` where the
/// platform keeps the grant itself).
#[derive(Debug, Clone, Copy, Default)]
pub struct Grant<'a> {
    pub access_token: Option<&'a [u8]>,
    pub refresh_token: Option<&'a [u8]>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: &'a [String],
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChatChannel {
    pub id: Uuid,
    pub platform: ChatPlatform,
    pub platform_channel_id: String,
    pub login: String,
    pub display_name: String,
    pub status: GrantStatus,
    pub authorized_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChatBot {
    pub id: Uuid,
    pub platform: ChatPlatform,
    /// `None` for the shared NOALBS bot and an organization's.
    pub workspace_id: Option<Uuid>,
    /// An organization's bot, speaking for its client workspaces.
    pub organization_id: Option<Uuid>,
    pub platform_user_id: String,
    pub login: String,
    pub display_name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub status: GrantStatus,
    pub connected_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

macro_rules! channel_columns {
    () => {
        "chat_channels.id, chat_channels.platform, chat_channels.platform_channel_id, \
         chat_channels.login, chat_channels.display_name, chat_channels.status, \
         chat_channels.authorized_by, chat_channels.created_at, chat_channels.updated_at"
    };
}

macro_rules! bot_columns {
    () => {
        "chat_bots.id, chat_bots.platform, chat_bots.workspace_id, chat_bots.organization_id, \
         chat_bots.platform_user_id, \
         chat_bots.login, chat_bots.display_name, chat_bots.scopes, chat_bots.expires_at, \
         chat_bots.status, chat_bots.connected_by, chat_bots.created_at, chat_bots.updated_at"
    };
}

/// For each `switcher_chat_channels` row (`links`, joined to `chat_channels`), the bot that speaks for it as `bot`: see the module docs. Needs
/// `switchers`, `workspaces` and `plans` (the `workspace_plans` view) joined as those names.
macro_rules! speaking_bot {
    () => {
        " LEFT JOIN LATERAL (
            SELECT chat_bots.*
            FROM chat_bots
            WHERE chat_bots.platform = chat_channels.platform
              AND chat_bots.status = 'active'
              AND ((chat_bots.workspace_id IS NULL
                    AND (chat_bots.organization_id IS NULL
                         OR chat_bots.organization_id = workspaces.organization_id))
                   OR (chat_bots.workspace_id = switchers.workspace_id AND plans.custom_chat_bot))
            ORDER BY (CASE WHEN chat_bots.workspace_id IS NOT NULL THEN 0
                           WHEN chat_bots.organization_id IS NOT NULL THEN 1
                           ELSE 2 END)
            LIMIT 1
        ) bot ON true "
    };
}

/// Stores the channel's grant (a new channel, or a fresh grant for a known one)
/// and links it to the switcher. Linking a linked channel again only refreshes.
#[tracing::instrument(skip(pool, grant))]
pub async fn link_channel(
    pool: &PgPool,
    switcher_id: Uuid,
    account: PlatformAccount<'_>,
    grant: Grant<'_>,
    user_id: Uuid,
) -> Result<ChatChannel> {
    let mut tx = pool.begin().await?;
    let channel = sqlx::query_as::<_, ChatChannel>(concat!(
        "
        INSERT INTO chat_channels (platform, platform_channel_id, login, display_name,
                                   access_token, refresh_token, expires_at, scopes, authorized_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (platform, platform_channel_id) DO UPDATE SET
            login = EXCLUDED.login,
            display_name = EXCLUDED.display_name,
            access_token = EXCLUDED.access_token,
            refresh_token = EXCLUDED.refresh_token,
            expires_at = EXCLUDED.expires_at,
            scopes = EXCLUDED.scopes,
            authorized_by = EXCLUDED.authorized_by,
            status = 'active'
        RETURNING ",
        channel_columns!()
    ))
    .bind(account.platform)
    .bind(account.id)
    .bind(account.login)
    .bind(account.display_name)
    .bind(grant.access_token)
    .bind(grant.refresh_token)
    .bind(grant.expires_at)
    .bind(grant.scopes)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "
        INSERT INTO switcher_chat_channels (switcher_id, chat_channel_id, linked_by)
        VALUES ($1, $2, $3)
        ON CONFLICT (switcher_id, chat_channel_id) DO NOTHING
        ",
    )
    .bind(switcher_id)
    .bind(channel.id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(channel)
}

/// Removes one switcher's link. The channel stays for any other switcher using
/// it; once nothing does, the chat service drops its subscriptions and then the
/// channel with its grant ([`delete_unlinked_channels`]).
#[tracing::instrument(skip(pool))]
pub async fn unlink_channel(pool: &PgPool, switcher_id: Uuid, channel_id: Uuid) -> Result<bool> {
    let removed = sqlx::query(
        "DELETE FROM switcher_chat_channels WHERE switcher_id = $1 AND chat_channel_id = $2",
    )
    .bind(switcher_id)
    .bind(channel_id)
    .execute(pool)
    .await?;
    Ok(removed.rows_affected() > 0)
}

/// Turns a link's automatic switch notifications on or off; `false` if the
/// channel isn't linked to the switcher.
#[tracing::instrument(skip(pool))]
pub async fn set_link_notification(
    pool: &PgPool,
    switcher_id: Uuid,
    channel_id: Uuid,
    enabled: bool,
) -> Result<bool> {
    let updated = sqlx::query(
        "UPDATE switcher_chat_channels SET auto_switch_notification = $3
         WHERE switcher_id = $1 AND chat_channel_id = $2",
    )
    .bind(switcher_id)
    .bind(channel_id)
    .bind(enabled)
    .execute(pool)
    .await?;
    Ok(updated.rows_affected() > 0)
}

/// A channel linked to a switcher, and who speaks there for it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LinkedChannel {
    #[sqlx(flatten)]
    pub channel: ChatChannel,
    pub linked_at: DateTime<Utc>,
    /// `None` when no bot is connected for the platform (not even the shared one).
    pub bot_login: Option<String>,
    pub bot_display_name: Option<String>,
    /// Whether it is the workspace's own bot rather than the shared one.
    pub bot_is_custom: bool,
    /// The bot is the client workspace's organization's.
    pub bot_is_organization: bool,
    /// Other switchers the channel is linked to, in any workspace. Commands run
    /// on all of them.
    pub other_links: i64,
    /// Whether this switcher's automatic switches are posted in the channel.
    pub auto_switch_notification: bool,
}

/// The channels linked to a switcher, oldest link first.
#[tracing::instrument(skip(pool))]
pub async fn channels_for_switcher(pool: &PgPool, switcher_id: Uuid) -> Result<Vec<LinkedChannel>> {
    let channels = sqlx::query_as::<_, LinkedChannel>(concat!(
        "SELECT ",
        channel_columns!(),
        ", links.created_at AS linked_at, links.auto_switch_notification,
           bot.login AS bot_login, bot.display_name AS bot_display_name,
           bot.workspace_id IS NOT NULL AS bot_is_custom,
           bot.organization_id IS NOT NULL AS bot_is_organization,
           (SELECT count(*) FROM switcher_chat_channels others
            WHERE others.chat_channel_id = links.chat_channel_id
              AND others.switcher_id <> links.switcher_id) AS other_links
        FROM switcher_chat_channels links
        JOIN chat_channels ON chat_channels.id = links.chat_channel_id
        JOIN switchers ON switchers.id = links.switcher_id
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        JOIN workspace_plans plans ON plans.workspace_id = workspaces.id",
        speaking_bot!(),
        "WHERE links.switcher_id = $1
        ORDER BY links.created_at, chat_channels.login"
    ))
    .bind(switcher_id)
    .fetch_all(pool)
    .await?;
    Ok(channels)
}

/// Stores a bot account's grant: the shared bot (`workspace_id` `None`) or a
/// workspace's own. Connecting another account replaces the one before.
pub async fn upsert_bot(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    account: PlatformAccount<'_>,
    grant: Grant<'_>,
    user_id: Uuid,
) -> Result<ChatBot> {
    upsert_owned_bot(pool, workspace_id, None, account, grant, user_id).await
}

/// Stores an organization's bot, which speaks for all its client workspaces.
pub async fn upsert_organization_bot(
    pool: &PgPool,
    organization_id: Uuid,
    account: PlatformAccount<'_>,
    grant: Grant<'_>,
    user_id: Uuid,
) -> Result<ChatBot> {
    upsert_owned_bot(pool, None, Some(organization_id), account, grant, user_id).await
}

#[tracing::instrument(skip(pool, grant))]
async fn upsert_owned_bot(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    organization_id: Option<Uuid>,
    account: PlatformAccount<'_>,
    grant: Grant<'_>,
    user_id: Uuid,
) -> Result<ChatBot> {
    let bot = sqlx::query_as::<_, ChatBot>(concat!(
        "
        INSERT INTO chat_bots (platform, workspace_id, organization_id, platform_user_id, login,
                               display_name, access_token, refresh_token, expires_at, scopes,
                               connected_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (workspace_id, organization_id, platform) DO UPDATE SET
            platform_user_id = EXCLUDED.platform_user_id,
            login = EXCLUDED.login,
            display_name = EXCLUDED.display_name,
            access_token = EXCLUDED.access_token,
            refresh_token = EXCLUDED.refresh_token,
            expires_at = EXCLUDED.expires_at,
            scopes = EXCLUDED.scopes,
            connected_by = EXCLUDED.connected_by,
            status = 'active'
        RETURNING ",
        bot_columns!()
    ))
    .bind(account.platform)
    .bind(workspace_id)
    .bind(organization_id)
    .bind(account.id)
    .bind(account.login)
    .bind(account.display_name)
    .bind(grant.access_token)
    .bind(grant.refresh_token)
    .bind(grant.expires_at)
    .bind(grant.scopes)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(bot)
}

/// A workspace's own bots (`Some`), or the shared bots (`None`).
pub async fn bots(pool: &PgPool, workspace_id: Option<Uuid>) -> Result<Vec<ChatBot>> {
    owned_bots(pool, workspace_id, None).await
}

/// An organization's bots.
pub async fn organization_bots(pool: &PgPool, organization_id: Uuid) -> Result<Vec<ChatBot>> {
    owned_bots(pool, None, Some(organization_id)).await
}

#[tracing::instrument(skip(pool))]
async fn owned_bots(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    organization_id: Option<Uuid>,
) -> Result<Vec<ChatBot>> {
    let bots = sqlx::query_as::<_, ChatBot>(concat!(
        "SELECT ",
        bot_columns!(),
        " FROM chat_bots
          WHERE workspace_id IS NOT DISTINCT FROM $1 AND organization_id IS NOT DISTINCT FROM $2
          ORDER BY platform"
    ))
    .bind(workspace_id)
    .bind(organization_id)
    .fetch_all(pool)
    .await?;
    Ok(bots)
}

/// Forgets a bot account: a workspace goes back to the shared bot.
pub async fn remove_bot(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    platform: ChatPlatform,
) -> Result<bool> {
    remove_owned_bot(pool, workspace_id, None, platform).await
}

/// Forgets an organization's bot: its clients go back to the shared bot.
pub async fn remove_organization_bot(
    pool: &PgPool,
    organization_id: Uuid,
    platform: ChatPlatform,
) -> Result<bool> {
    remove_owned_bot(pool, None, Some(organization_id), platform).await
}

#[tracing::instrument(skip(pool))]
async fn remove_owned_bot(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    organization_id: Option<Uuid>,
    platform: ChatPlatform,
) -> Result<bool> {
    let removed = sqlx::query(
        "DELETE FROM chat_bots
         WHERE workspace_id IS NOT DISTINCT FROM $1 AND organization_id IS NOT DISTINCT FROM $2
           AND platform = $3",
    )
    .bind(workspace_id)
    .bind(organization_id)
    .bind(platform)
    .execute(pool)
    .await?;
    Ok(removed.rows_affected() > 0)
}

/// A subscription the links need: a channel's events, read on Twitch by one bot
/// (`platform_bot_id`; empty on Kick, where subscriptions are per channel).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, sqlx::FromRow)]
pub struct WantedSubscription {
    pub platform: ChatPlatform,
    pub platform_channel_id: String,
    pub platform_bot_id: String,
}

/// What every link needs, for the chat service's reconciler: each active
/// channel with a link, per bot speaking there. Twitch channels without any
/// bot are left out (there's nobody to read with). YouTube has no subscriptions
/// (see [`youtube_channels`]).
#[tracing::instrument(skip(pool))]
pub async fn wanted_subscriptions(pool: &PgPool) -> Result<Vec<WantedSubscription>> {
    let wanted = sqlx::query_as::<_, WantedSubscription>(concat!(
        "SELECT DISTINCT chat_channels.platform, chat_channels.platform_channel_id,
                CASE WHEN chat_channels.platform = 'KICK' THEN '' ELSE bot.platform_user_id END
                    AS platform_bot_id
        FROM switcher_chat_channels links
        JOIN chat_channels ON chat_channels.id = links.chat_channel_id
        JOIN switchers ON switchers.id = links.switcher_id
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        JOIN workspace_plans plans ON plans.workspace_id = workspaces.id",
        speaking_bot!(),
        "WHERE chat_channels.status = 'active' AND plans.suspended_at IS NULL
          AND chat_channels.platform <> 'YOUTUBE'
          AND (chat_channels.platform = 'KICK' OR bot.id IS NOT NULL)
        ORDER BY 1, 2, 3"
    ))
    .fetch_all(pool)
    .await?;
    Ok(wanted)
}

/// A switcher that a channel's chat controls, as the chat service needs it
/// when a message arrives.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChatLink {
    pub switcher_id: Uuid,
    pub switcher_name: String,
    pub workspace_id: Uuid,
    /// The switcher's command prefix, so a message that starts with none of its
    /// links' prefixes is dropped without loading anything else.
    pub command_prefix: String,
    /// The bot speaking for this link; `None` when none is connected.
    pub bot_id: Option<Uuid>,
    pub bot_platform_user_id: Option<String>,
    pub bot_login: Option<String>,
}

/// The switchers an active channel is linked to, oldest link first.
#[tracing::instrument(skip(pool))]
pub async fn links_for_channel(
    pool: &PgPool,
    platform: ChatPlatform,
    platform_channel_id: &str,
) -> Result<Vec<ChatLink>> {
    let links = sqlx::query_as::<_, ChatLink>(concat!(
        "SELECT switchers.id AS switcher_id, switchers.name AS switcher_name,
                switchers.workspace_id, switchers.command_prefix, bot.id AS bot_id,
                bot.platform_user_id AS bot_platform_user_id, bot.login AS bot_login
        FROM switcher_chat_channels links
        JOIN chat_channels ON chat_channels.id = links.chat_channel_id
        JOIN switchers ON switchers.id = links.switcher_id
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        JOIN workspace_plans plans ON plans.workspace_id = workspaces.id",
        speaking_bot!(),
        "WHERE chat_channels.platform = $1 AND chat_channels.platform_channel_id = $2
          AND chat_channels.status = 'active' AND plans.suspended_at IS NULL
        ORDER BY links.created_at"
    ))
    .bind(platform)
    .bind(platform_channel_id)
    .fetch_all(pool)
    .await?;
    Ok(links)
}

/// Every bot account's platform user ID, so the bots never answer each other.
#[tracing::instrument(skip(pool))]
pub async fn bot_user_ids(pool: &PgPool, platform: ChatPlatform) -> Result<Vec<String>> {
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT platform_user_id FROM chat_bots WHERE platform = $1",
    )
    .bind(platform)
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

/// Whether a chat account belongs to a member of the workspace (they linked it
/// to their NOALBS login), which makes them an admin in chat.
#[tracing::instrument(skip(pool))]
pub async fn is_member_account(
    pool: &PgPool,
    workspace_id: Uuid,
    platform: ChatPlatform,
    platform_user_id: &str,
) -> Result<bool> {
    let member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM oauth2_connections
            JOIN workspace_members ON workspace_members.user_id = oauth2_connections.user_id
            WHERE workspace_members.workspace_id = $1
              AND oauth2_connections.provider = $2 AND oauth2_connections.provider_id = $3
        )",
    )
    .bind(workspace_id)
    .bind(platform)
    .bind(platform_user_id)
    .fetch_one(pool)
    .await?;
    Ok(member)
}

/// Whether the switcher ignores this chatter (e.g. another bot).
#[tracing::instrument(skip(pool))]
pub async fn is_ignored(
    pool: &PgPool,
    switcher_id: Uuid,
    platform: ChatPlatform,
    platform_user_id: &str,
) -> Result<bool> {
    let ignored = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM chat_ignored_chatters
            WHERE switcher_id = $1 AND platform = $2 AND platform_user_id = $3
        )",
    )
    .bind(switcher_id)
    .bind(platform)
    .bind(platform_user_id)
    .fetch_one(pool)
    .await?;
    Ok(ignored)
}

/// A subscription the chat service made on a platform.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChatSubscription {
    pub id: Uuid,
    pub platform: ChatPlatform,
    pub event_type: String,
    pub platform_channel_id: String,
    pub platform_bot_id: String,
    pub platform_subscription_id: String,
}

#[tracing::instrument(skip(pool))]
pub async fn subscriptions(pool: &PgPool) -> Result<Vec<ChatSubscription>> {
    let subscriptions = sqlx::query_as::<_, ChatSubscription>(
        "SELECT id, platform, event_type, platform_channel_id, platform_bot_id,
                platform_subscription_id
         FROM chat_subscriptions ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(subscriptions)
}

/// Records a subscription the platform made (or already had) for us.
#[tracing::instrument(skip(pool))]
pub async fn record_subscription(
    pool: &PgPool,
    platform: ChatPlatform,
    event_type: &str,
    platform_channel_id: &str,
    platform_bot_id: &str,
    platform_subscription_id: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO chat_subscriptions (platform, event_type, platform_channel_id,
                                         platform_bot_id, platform_subscription_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (platform, event_type, platform_channel_id, platform_bot_id)
         DO UPDATE SET platform_subscription_id = EXCLUDED.platform_subscription_id",
    )
    .bind(platform)
    .bind(event_type)
    .bind(platform_channel_id)
    .bind(platform_bot_id)
    .bind(platform_subscription_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[tracing::instrument(skip(pool))]
pub async fn forget_subscription(
    pool: &PgPool,
    platform: ChatPlatform,
    platform_subscription_id: &str,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM chat_subscriptions WHERE platform = $1 AND platform_subscription_id = $2",
    )
    .bind(platform)
    .bind(platform_subscription_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Forgets every subscription on a platform, e.g. when the Twitch conduit they
/// were delivered through is replaced.
#[tracing::instrument(skip(pool))]
pub async fn forget_subscriptions(pool: &PgPool, platform: ChatPlatform) -> Result<()> {
    sqlx::query("DELETE FROM chat_subscriptions WHERE platform = $1")
        .bind(platform)
        .execute(pool)
        .await?;
    Ok(())
}

/// A shared setting of the chat service instances.
#[tracing::instrument(skip(pool))]
pub async fn platform_state(pool: &PgPool, key: &str) -> Result<Option<String>> {
    let value =
        sqlx::query_scalar::<_, String>("SELECT value FROM chat_platform_state WHERE key = $1")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(value)
}

/// Stores `value` in a shared setting if the key is unset (`expected` `None`) or still holds
/// `expected`, and returns what the key holds afterwards. Two instances racing
/// to set it both end up with the same winner.
#[tracing::instrument(skip(pool))]
pub async fn replace_platform_state(
    pool: &PgPool,
    key: &str,
    expected: Option<&str>,
    value: &str,
) -> Result<String> {
    sqlx::query(
        "INSERT INTO chat_platform_state (key, value) VALUES ($1, $3)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value
         WHERE chat_platform_state.value = $2",
    )
    .bind(key)
    .bind(expected)
    .bind(value)
    .execute(pool)
    .await?;
    let current =
        sqlx::query_scalar::<_, String>("SELECT value FROM chat_platform_state WHERE key = $1")
            .bind(key)
            .fetch_one(pool)
            .await?;
    Ok(current)
}

/// Whose stored tokens (Kick and YouTube; Twitch keeps its grants itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenOwner {
    Bot(Uuid),
    Channel(Uuid),
}

/// A grant's tokens, still encrypted.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StoredTokens {
    /// Whose endpoint refreshes them (not written by [`store_tokens`]).
    pub platform: ChatPlatform,
    pub access_token: Option<Vec<u8>>,
    pub refresh_token: Option<Vec<u8>>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Reads the tokens, locking the row until `tx` ends, so only one instance
/// refreshes them at a time (Kick replaces the refresh token on every use).
pub async fn lock_tokens(
    tx: &mut sqlx::PgConnection,
    owner: TokenOwner,
) -> Result<Option<StoredTokens>> {
    let query = match owner {
        TokenOwner::Bot(_) => {
            "SELECT platform, access_token, refresh_token, expires_at FROM chat_bots
             WHERE id = $1 FOR UPDATE"
        }
        TokenOwner::Channel(_) => {
            "SELECT platform, access_token, refresh_token, expires_at FROM chat_channels
             WHERE id = $1 FOR UPDATE"
        }
    };
    let (TokenOwner::Bot(id) | TokenOwner::Channel(id)) = owner;
    let tokens = sqlx::query_as::<_, StoredTokens>(query)
        .bind(id)
        .fetch_optional(tx)
        .await?;
    Ok(tokens)
}

/// Stores refreshed tokens (already encrypted).
pub async fn store_tokens(
    tx: &mut sqlx::PgConnection,
    owner: TokenOwner,
    tokens: &StoredTokens,
) -> Result<()> {
    let query = match owner {
        TokenOwner::Bot(_) => {
            "UPDATE chat_bots SET access_token = $2, refresh_token = $3, expires_at = $4
             WHERE id = $1"
        }
        TokenOwner::Channel(_) => {
            "UPDATE chat_channels SET access_token = $2, refresh_token = $3, expires_at = $4
             WHERE id = $1"
        }
    };
    let (TokenOwner::Bot(id) | TokenOwner::Channel(id)) = owner;
    sqlx::query(query)
        .bind(id)
        .bind(&tokens.access_token)
        .bind(&tokens.refresh_token)
        .bind(tokens.expires_at)
        .execute(tx)
        .await?;
    Ok(())
}

/// What a revoked grant becomes: its tokens are useless now, so they go.
macro_rules! revoked {
    () => {
        "status = 'revoked', access_token = NULL, refresh_token = NULL, expires_at = NULL"
    };
}

/// Marks a grant revoked: the platform refused it and someone has to connect it again.
#[tracing::instrument(skip(pool))]
pub async fn revoke(pool: &PgPool, owner: TokenOwner) -> Result<()> {
    let query = match owner {
        TokenOwner::Bot(_) => concat!("UPDATE chat_bots SET ", revoked!(), " WHERE id = $1"),
        TokenOwner::Channel(_) => {
            concat!("UPDATE chat_channels SET ", revoked!(), " WHERE id = $1")
        }
    };
    let (TokenOwner::Bot(id) | TokenOwner::Channel(id)) = owner;
    sqlx::query(query).bind(id).execute(pool).await?;
    Ok(())
}

/// A channel by its platform ID (e.g. to find its token for subscribing).
#[tracing::instrument(skip(pool))]
pub async fn channel_by_platform_id(
    pool: &PgPool,
    platform: ChatPlatform,
    platform_channel_id: &str,
) -> Result<Option<ChatChannel>> {
    let channel = sqlx::query_as::<_, ChatChannel>(concat!(
        "SELECT ",
        channel_columns!(),
        " FROM chat_channels WHERE platform = $1 AND platform_channel_id = $2"
    ))
    .bind(platform)
    .bind(platform_channel_id)
    .fetch_optional(pool)
    .await?;
    Ok(channel)
}

/// Active channels of `platform` where a linked switcher stops the stream after
/// raiding out (`auto_stop_on_raid`), so the chat service hears about their raids.
#[tracing::instrument(skip(pool))]
pub async fn raid_channels(pool: &PgPool, platform: ChatPlatform) -> Result<Vec<String>> {
    let channels = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT chat_channels.platform_channel_id
         FROM switcher_chat_channels links
         JOIN chat_channels ON chat_channels.id = links.chat_channel_id
         JOIN switchers ON switchers.id = links.switcher_id
         JOIN workspaces ON workspaces.id = switchers.workspace_id
         JOIN workspace_plans plans ON plans.workspace_id = workspaces.id
         WHERE chat_channels.platform = $1 AND chat_channels.status = 'active'
           AND switchers.auto_stop_on_raid AND plans.suspended_at IS NULL
         ORDER BY 1",
    )
    .bind(platform)
    .fetch_all(pool)
    .await?;
    Ok(channels)
}

/// A YouTube channel whose live chat the chat service follows while one of its
/// switchers streams.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct YouTubeChannel {
    pub id: Uuid,
    pub platform_channel_id: String,
    pub switcher_ids: Vec<Uuid>,
}

/// Active YouTube channels with a bot to answer in, and the switchers each one
/// controls (outside suspended workspaces).
#[tracing::instrument(skip(pool))]
pub async fn youtube_channels(pool: &PgPool) -> Result<Vec<YouTubeChannel>> {
    let channels = sqlx::query_as::<_, YouTubeChannel>(concat!(
        "SELECT chat_channels.id, chat_channels.platform_channel_id,
                array_agg(DISTINCT switchers.id ORDER BY switchers.id) AS switcher_ids
        FROM switcher_chat_channels links
        JOIN chat_channels ON chat_channels.id = links.chat_channel_id
        JOIN switchers ON switchers.id = links.switcher_id
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        JOIN workspace_plans plans ON plans.workspace_id = workspaces.id",
        speaking_bot!(),
        "WHERE chat_channels.platform = 'YOUTUBE' AND chat_channels.status = 'active'
          AND plans.suspended_at IS NULL AND bot.id IS NOT NULL
        GROUP BY chat_channels.id
        ORDER BY chat_channels.platform_channel_id"
    ))
    .fetch_all(pool)
    .await?;
    Ok(channels)
}

/// A channel a switcher posts to, with the bot speaking there for it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SwitcherChannel {
    pub platform: ChatPlatform,
    pub platform_channel_id: String,
    pub bot_id: Option<Uuid>,
    pub bot_platform_user_id: Option<String>,
    /// The channel controls other switchers too (so posts name the switcher).
    pub shared: bool,
    /// This switcher's automatic switches are posted here (when the switcher
    /// has `auto_switch_notification` on).
    pub auto_switch_notification: bool,
}

/// The active channels linked to a switcher, e.g. to post its notifications.
#[tracing::instrument(skip(pool))]
pub async fn channels_of_switcher(
    pool: &PgPool,
    switcher_id: Uuid,
) -> Result<Vec<SwitcherChannel>> {
    let channels = sqlx::query_as::<_, SwitcherChannel>(concat!(
        "SELECT chat_channels.platform, chat_channels.platform_channel_id,
                bot.id AS bot_id, bot.platform_user_id AS bot_platform_user_id,
                links.auto_switch_notification,
                EXISTS (SELECT 1 FROM switcher_chat_channels others
                        WHERE others.chat_channel_id = links.chat_channel_id
                          AND others.switcher_id <> links.switcher_id) AS shared
        FROM switcher_chat_channels links
        JOIN chat_channels ON chat_channels.id = links.chat_channel_id
        JOIN switchers ON switchers.id = links.switcher_id
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        JOIN workspace_plans plans ON plans.workspace_id = workspaces.id",
        speaking_bot!(),
        "WHERE links.switcher_id = $1 AND chat_channels.status = 'active'
          AND plans.suspended_at IS NULL
        ORDER BY links.created_at"
    ))
    .bind(switcher_id)
    .fetch_all(pool)
    .await?;
    Ok(channels)
}

/// Stored grants (Kick, YouTube) whose access token expires before `before`, to refresh
/// them ahead of time (an unused refresh token can expire too).
#[tracing::instrument(skip(pool))]
pub async fn tokens_expiring(pool: &PgPool, before: DateTime<Utc>) -> Result<Vec<TokenOwner>> {
    let rows = sqlx::query_as::<_, (bool, Uuid)>(
        "SELECT true, id FROM chat_bots
         WHERE status = 'active' AND refresh_token IS NOT NULL AND expires_at < $1
         UNION ALL
         SELECT false, id FROM chat_channels
         WHERE status = 'active' AND refresh_token IS NOT NULL AND expires_at < $1",
    )
    .bind(before)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(bot, id)| {
            if bot {
                TokenOwner::Bot(id)
            } else {
                TokenOwner::Channel(id)
            }
        })
        .collect())
}

/// A platform account took our app's access away: every bot and channel grant
/// of that account is revoked. Returns how many there were.
#[tracing::instrument(skip(pool))]
pub async fn revoke_account(
    pool: &PgPool,
    platform: ChatPlatform,
    platform_user_id: &str,
) -> Result<u64> {
    let bots = sqlx::query(concat!(
        "UPDATE chat_bots SET ",
        revoked!(),
        " WHERE platform = $1 AND platform_user_id = $2 AND status = 'active'"
    ))
    .bind(platform)
    .bind(platform_user_id)
    .execute(pool)
    .await?;
    let channels = sqlx::query(concat!(
        "UPDATE chat_channels SET ",
        revoked!(),
        " WHERE platform = $1 AND platform_channel_id = $2 AND status = 'active'"
    ))
    .bind(platform)
    .bind(platform_user_id)
    .execute(pool)
    .await?;
    Ok(bots.rows_affected() + channels.rows_affected())
}

/// A chatter a switcher ignores (e.g. another bot).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IgnoredChatter {
    pub platform: ChatPlatform,
    pub platform_user_id: String,
    pub platform_username: String,
    pub created_at: DateTime<Utc>,
}

#[tracing::instrument(skip(pool))]
pub async fn ignored_chatters(pool: &PgPool, switcher_id: Uuid) -> Result<Vec<IgnoredChatter>> {
    let ignored = sqlx::query_as::<_, IgnoredChatter>(
        "SELECT platform, platform_user_id, platform_username, created_at
         FROM chat_ignored_chatters WHERE switcher_id = $1
         ORDER BY platform, lower(platform_username)",
    )
    .bind(switcher_id)
    .fetch_all(pool)
    .await?;
    Ok(ignored)
}

/// Ignores a chatter (again: the name is refreshed).
#[tracing::instrument(skip(pool, platform_username))]
pub async fn ignore_chatter(
    pool: &PgPool,
    switcher_id: Uuid,
    platform: ChatPlatform,
    platform_user_id: &str,
    platform_username: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO chat_ignored_chatters (switcher_id, platform, platform_user_id, platform_username)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (switcher_id, platform, platform_user_id)
         DO UPDATE SET platform_username = EXCLUDED.platform_username",
    )
    .bind(switcher_id)
    .bind(platform)
    .bind(platform_user_id)
    .bind(platform_username)
    .execute(pool)
    .await?;
    Ok(())
}

#[tracing::instrument(skip(pool))]
pub async fn unignore_chatter(
    pool: &PgPool,
    switcher_id: Uuid,
    platform: ChatPlatform,
    platform_user_id: &str,
) -> Result<bool> {
    let removed = sqlx::query(
        "DELETE FROM chat_ignored_chatters
         WHERE switcher_id = $1 AND platform = $2 AND platform_user_id = $3",
    )
    .bind(switcher_id)
    .bind(platform)
    .bind(platform_user_id)
    .execute(pool)
    .await?;
    Ok(removed.rows_affected() > 0)
}

/// Stops ignoring a chatter by name (case-insensitive), as chat commands only know the name.
#[tracing::instrument(skip(pool, platform_username))]
pub async fn unignore_chatter_named(
    pool: &PgPool,
    switcher_id: Uuid,
    platform: ChatPlatform,
    platform_username: &str,
) -> Result<bool> {
    let removed = sqlx::query(
        "DELETE FROM chat_ignored_chatters
         WHERE switcher_id = $1 AND platform = $2 AND lower(platform_username) = lower($3)",
    )
    .bind(switcher_id)
    .bind(platform)
    .bind(platform_username)
    .execute(pool)
    .await?;
    Ok(removed.rows_affected() > 0)
}

/// This database's chat environment: a random ID made once, which the chat
/// service and the API put in their NATS subjects and keys, so environments
/// sharing a NATS server (staging and production, tests and dev) stay apart.
#[tracing::instrument(skip(pool))]
pub async fn environment_id(pool: &PgPool) -> Result<String> {
    const KEY: &str = "environment_id";
    if let Some(id) = platform_state(pool, KEY).await? {
        return Ok(id);
    }
    let fresh = Uuid::new_v4().simple().to_string()[..12].to_owned();
    replace_platform_state(pool, KEY, None, &fresh).await
}

/// A switcher a channel is linked to, for staff.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChannelLink {
    pub chat_channel_id: Uuid,
    pub switcher_id: Uuid,
    pub switcher_name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
}

/// The channels linked to any of the workspace's switchers, for Admin.
#[tracing::instrument(skip(pool))]
pub async fn channels_of_workspace(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<ChatChannel>> {
    let channels = sqlx::query_as::<_, ChatChannel>(concat!(
        "SELECT DISTINCT ",
        channel_columns!(),
        " FROM chat_channels
         JOIN switcher_chat_channels links ON links.chat_channel_id = chat_channels.id
         JOIN switchers ON switchers.id = links.switcher_id
         WHERE switchers.workspace_id = $1
         ORDER BY chat_channels.created_at"
    ))
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(channels)
}

/// Every link of these channels, in any workspace, oldest first.
#[tracing::instrument(skip(pool))]
pub async fn links_of_channels(pool: &PgPool, channel_ids: &[Uuid]) -> Result<Vec<ChannelLink>> {
    let links = sqlx::query_as::<_, ChannelLink>(
        "SELECT links.chat_channel_id, switchers.id AS switcher_id, switchers.name AS switcher_name,
                workspaces.id AS workspace_id, workspaces.name AS workspace_name
         FROM switcher_chat_channels links
         JOIN switchers ON switchers.id = links.switcher_id
         JOIN workspaces ON workspaces.id = switchers.workspace_id
         WHERE links.chat_channel_id = ANY($1)
         ORDER BY links.created_at",
    )
    .bind(channel_ids)
    .fetch_all(pool)
    .await?;
    Ok(links)
}

/// Unlinks a channel from every switcher (staff: the bot leaves it). Returns
/// the links it had; the channel and its grant go with the chat service's next
/// reconcile ([`delete_unlinked_channels`]), and its owner can link it again.
#[tracing::instrument(skip(pool))]
pub async fn unlink_everywhere(pool: &PgPool, channel_id: Uuid) -> Result<Vec<ChannelLink>> {
    let mut tx = pool.begin().await?;
    let links = sqlx::query_as::<_, ChannelLink>(
        "SELECT links.chat_channel_id, switchers.id AS switcher_id, switchers.name AS switcher_name,
                workspaces.id AS workspace_id, workspaces.name AS workspace_name
         FROM switcher_chat_channels links
         JOIN switchers ON switchers.id = links.switcher_id
         JOIN workspaces ON workspaces.id = switchers.workspace_id
         WHERE links.chat_channel_id = $1
         ORDER BY links.created_at
         FOR UPDATE OF links",
    )
    .bind(channel_id)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM switcher_chat_channels WHERE chat_channel_id = $1")
        .bind(channel_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(links)
}

/// Deletes the channels no switcher links any more (unlinked, or their switchers
/// or workspaces deleted), with their grants, once the chat service has dropped
/// their platform subscriptions. Returns how many went.
#[tracing::instrument(skip(pool))]
pub async fn delete_unlinked_channels(pool: &PgPool) -> Result<u64> {
    macro_rules! unlinked {
        () => {
            "NOT EXISTS (SELECT 1 FROM switcher_chat_channels links
                         WHERE links.chat_channel_id = chat_channels.id)
             AND NOT EXISTS (SELECT 1 FROM chat_subscriptions subs
                             WHERE subs.platform = chat_channels.platform
                               AND subs.platform_channel_id = chat_channels.platform_channel_id)"
        };
    }
    let mut tx = pool.begin().await?;
    // Lock first, then check again in a new statement: a link being made right
    // now (`link_channel` updates the channel row, then links it) has committed
    // by then, and the second statement's snapshot sees it.
    let candidates = sqlx::query_scalar::<_, Uuid>(concat!(
        "SELECT id FROM chat_channels WHERE ",
        unlinked!(),
        " FOR UPDATE"
    ))
    .fetch_all(&mut *tx)
    .await?;
    if candidates.is_empty() {
        return Ok(0);
    }
    let deleted = sqlx::query(concat!(
        "DELETE FROM chat_channels WHERE id = ANY($1) AND ",
        unlinked!()
    ))
    .bind(&candidates)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(deleted.rows_affected())
}

/// One channel by ID.
#[tracing::instrument(skip(pool))]
pub async fn channel(pool: &PgPool, channel_id: Uuid) -> Result<Option<ChatChannel>> {
    let channel = sqlx::query_as::<_, ChatChannel>(concat!(
        "SELECT ",
        channel_columns!(),
        " FROM chat_channels WHERE id = $1"
    ))
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;
    Ok(channel)
}
