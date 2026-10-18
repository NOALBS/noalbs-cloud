use std::collections::BTreeMap;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Switcher {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,

    pub scene_live: Option<Uuid>,
    pub scene_low: Option<Uuid>,
    pub scene_offline: Option<Uuid>,

    pub scene_starting: Option<Uuid>,
    pub scene_ending: Option<Uuid>,
    pub scene_privacy: Option<Uuid>,
    pub scene_refresh: Option<Uuid>,

    pub trigger_low: Option<i32>,
    pub trigger_offline: Option<i32>,
    pub trigger_rtt: Option<i32>,
    pub trigger_rtt_offline: Option<i32>,

    pub bitrate_switcher_enabled: bool,
    pub only_switch_when_streaming: bool,
    pub instantly_switch_on_recover: bool,
    pub auto_switch_notification: bool,

    pub retry_attempts: i32,

    pub auto_stop_on_raid: bool,
    pub announce_raid: bool,
    pub record_while_streaming: bool,
    /// OBS scenes on which NOALBS leaves recording running. Empty disables
    /// scene-based recording control.
    pub recording_scene_ids: Vec<Uuid>,
    pub switch_to_starting: bool,
    pub switch_to_live: bool,
    /// Minutes on the BRB scene with no feed before the stream is stopped
    /// (NOALBS's offline timeout); `None` = never.
    pub offline_timeout: Option<i32>,
    /// Minutes on the Ending scene before the stream is stopped; `None` = never.
    pub ending_timeout: Option<i32>,

    pub connect_token_hash: Vec<u8>,
    pub password: Option<Vec<u8>>,
    pub port: i32,
    pub last_seen_at: Option<DateTime<chrono::Utc>>,

    pub command_prefix: String,
    pub language: String,
    pub public_commands_enabled: bool,
    pub mod_commands_enabled: bool,
    pub threaded_replies: bool,

    /// The BELABOX Cloud remote key, encrypted; `None` = BELABOX isn't used.
    pub belabox_remote_key: Option<Vec<u8>>,
    /// Start BELABOX when the OBS stream starts.
    pub belabox_start_with_stream: bool,
    /// Stop BELABOX when the OBS stream stops.
    pub belabox_stop_with_stream: bool,
    /// Names for the BELABOX's interfaces, by interface key (a JSON object of strings).
    pub belabox_interface_labels: serde_json::Value,

    /// The Moblin remote control password, encrypted; `None` = Moblin isn't used.
    pub moblin_password: Option<Vec<u8>>,
    /// Start Moblin's stream when the OBS stream starts.
    pub moblin_start_with_stream: bool,
    /// Stop Moblin's stream when the OBS stream stops.
    pub moblin_stop_with_stream: bool,

    pub created_at: DateTime<chrono::Utc>,
    pub updated_at: DateTime<chrono::Utc>,
}

/// NOALBS's default chat aliases (`alias`, `command`), which every new switcher
/// gets: `!b` bitrate, `!f` fix, `!ss` switch, and belabot's `!bbm` for `!bbs`.
/// Removing one keeps it removed. The migrations `…_default_chat_aliases` and
/// `…_belabox` gave them to earlier switchers.
macro_rules! default_aliases {
    () => {
        "('b', 'bitrate'), ('f', 'fix'), ('ss', 'switch'), ('bbm', 'bbs')"
    };
}

/// Creates a switcher, with NOALBS's default chat aliases. `connect_token_hash`
/// is the SHA-256 of the token the browser source identifies with (see
/// `common::tokens`).
pub async fn create<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    workspace_id: Uuid,
    name: &str,
    connect_token_hash: &[u8],
) -> Result<Switcher> {
    let switcher = sqlx::query_as::<_, Switcher>(concat!(
        "
        WITH switcher AS (
            INSERT INTO switchers (workspace_id, name, connect_token_hash)
            VALUES ($1, $2, $3)
            RETURNING *
        ),
        aliases AS (
            INSERT INTO noalbs_chat_command_aliases (switcher_id, alias, command)
            SELECT switcher.id, defaults.alias, defaults.command
            FROM switcher, (VALUES ",
        default_aliases!(),
        ") AS defaults (alias, command)
        )
        SELECT * FROM switcher
        "
    ))
    .bind(workspace_id)
    .bind(name)
    .bind(connect_token_hash)
    .fetch_one(executor)
    .await?;

    Ok(switcher)
}

/// Creates a switcher if the workspace's plan has room for another one,
/// returning `None` when it doesn't. The workspace row is locked so
/// concurrent creates can't exceed the limit.
#[tracing::instrument(skip(pool, connect_token_hash))]
pub async fn create_within_plan(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    connect_token_hash: &[u8],
) -> Result<Option<Switcher>> {
    let mut tx = pool.begin().await?;

    // The limit of a client workspace is its allocation (at most the pool); the pool
    // itself is checked below. Lock order is workspace, then organization.
    let (max_switchers, organization_id) = sqlx::query_as::<_, (i32, Option<Uuid>)>(
        "
        SELECT plans.max_switchers, workspaces.organization_id
        FROM workspaces
        JOIN workspace_plans plans ON plans.workspace_id = workspaces.id
        WHERE workspaces.id = $1
        FOR UPDATE OF workspaces
        ",
    )
    .bind(workspace_id)
    .fetch_one(&mut *tx)
    .await?;

    if count_in_workspace(&mut *tx, workspace_id).await? >= i64::from(max_switchers) {
        return Ok(None);
    }

    if let Some(organization_id) = organization_id {
        let (pool, used) = sqlx::query_as::<_, (i32, i64)>(
            "
            SELECT organizations.max_switchers,
                   (SELECT count(*) FROM switchers
                    JOIN workspaces ON workspaces.id = switchers.workspace_id
                    WHERE workspaces.organization_id = organizations.id)
            FROM organizations
            WHERE organizations.id = $1
            FOR UPDATE
            ",
        )
        .bind(organization_id)
        .fetch_one(&mut *tx)
        .await?;
        if used >= i64::from(pool) {
            return Ok(None);
        }
    }

    let switcher = create(&mut *tx, workspace_id, name, connect_token_hash).await?;
    tx.commit().await?;

    Ok(Some(switcher))
}

/// Returns the switcher only when it belongs to `workspace_id`.
#[tracing::instrument(skip(pool))]
pub async fn get_in_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
    switcher_id: Uuid,
) -> Result<Option<Switcher>> {
    let switcher = sqlx::query_as::<_, Switcher>(
        "SELECT * FROM switchers WHERE id = $1 AND workspace_id = $2",
    )
    .bind(switcher_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;

    Ok(switcher)
}

#[tracing::instrument(skip(pool))]
pub async fn by_id(pool: &PgPool, switcher_id: Uuid) -> Result<Switcher> {
    let switcher = sqlx::query_as::<_, Switcher>(
        "
        SELECT *
        FROM switchers
        WHERE id = $1
        ",
    )
    .bind(switcher_id)
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}

pub async fn count_in_workspace<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    workspace_id: Uuid,
) -> Result<i64> {
    let count =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM switchers WHERE workspace_id = $1")
            .bind(workspace_id)
            .fetch_one(executor)
            .await?;

    Ok(count)
}

/// A switcher as listed in its workspace.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SwitcherSummary {
    pub id: Uuid,
    pub name: String,
    pub last_seen_at: Option<DateTime<chrono::Utc>>,
    pub stream_server_count: i64,
    pub created_at: DateTime<chrono::Utc>,
    /// Over the plan's switcher limit, or the workspace (or its organization) is
    /// suspended (see [`is_paused`]).
    pub paused: bool,
}

#[tracing::instrument(skip(pool))]
pub async fn list_summaries(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<SwitcherSummary>> {
    // A client workspace's limits only stop new switchers (see `create_within_plan`):
    // what's there keeps running, so only a plan's limit ranks them, and only
    // suspension pauses a client's.
    let switchers = sqlx::query_as::<_, SwitcherSummary>(
        "
        SELECT
            switchers.id,
            switchers.name,
            switchers.last_seen_at,
            (SELECT count(*) FROM stream_servers WHERE stream_servers.switcher_id = switchers.id)
                AS stream_server_count,
            switchers.created_at,
            (workspaces.organization_id IS NULL
                AND row_number() OVER (ORDER BY switchers.created_at, switchers.id)
                    > plans.max_switchers)
                OR plans.suspended_at IS NOT NULL AS paused
        FROM switchers
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        JOIN workspace_plans plans ON plans.workspace_id = workspaces.id
        WHERE switchers.workspace_id = $1
        ORDER BY switchers.created_at, switchers.id
        ",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;

    Ok(switchers)
}

#[tracing::instrument(skip(pool))]
pub async fn rename(pool: &PgPool, switcher_id: Uuid, name: &str) -> Result<Switcher> {
    let switcher =
        sqlx::query_as::<_, Switcher>("UPDATE switchers SET name = $2 WHERE id = $1 RETURNING *")
            .bind(switcher_id)
            .bind(name)
            .fetch_one(pool)
            .await?;

    Ok(switcher)
}

/// Bitrate (kbps) and RTT (ms) thresholds. `None` turns a trigger off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triggers {
    pub low: Option<i32>,
    pub offline: Option<i32>,
    pub rtt: Option<i32>,
    pub rtt_offline: Option<i32>,
}

#[tracing::instrument(skip(pool))]
pub async fn set_triggers(
    pool: &PgPool,
    switcher_id: Uuid,
    triggers: Triggers,
) -> Result<Switcher> {
    let switcher = sqlx::query_as::<_, Switcher>(
        "
        UPDATE switchers
        SET trigger_low = $2, trigger_offline = $3, trigger_rtt = $4, trigger_rtt_offline = $5
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(triggers.low)
    .bind(triggers.offline)
    .bind(triggers.rtt)
    .bind(triggers.rtt_offline)
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}

/// How the switcher behaves, apart from its triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Behaviour {
    pub bitrate_switcher_enabled: bool,
    pub only_switch_when_streaming: bool,
    pub instantly_switch_on_recover: bool,
    pub auto_switch_notification: bool,
    pub retry_attempts: i32,
    pub auto_stop_on_raid: bool,
    pub announce_raid: bool,
    pub record_while_streaming: bool,
    pub recording_scene_ids: Vec<Uuid>,
    pub switch_to_starting: bool,
    pub switch_to_live: bool,
    pub offline_timeout: Option<i32>,
    pub ending_timeout: Option<i32>,
}

#[tracing::instrument(skip(pool))]
pub async fn set_behaviour(
    pool: &PgPool,
    switcher_id: Uuid,
    behaviour: Behaviour,
) -> Result<Switcher> {
    let switcher = sqlx::query_as::<_, Switcher>(
        "
        UPDATE switchers
        SET bitrate_switcher_enabled = $2,
            only_switch_when_streaming = $3,
            instantly_switch_on_recover = $4,
            auto_switch_notification = $5,
            retry_attempts = $6,
            auto_stop_on_raid = $7,
            announce_raid = $8,
            record_while_streaming = $9,
            recording_scene_ids = $10,
            switch_to_starting = $11,
            switch_to_live = $12,
            offline_timeout = $13,
            ending_timeout = $14
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(behaviour.bitrate_switcher_enabled)
    .bind(behaviour.only_switch_when_streaming)
    .bind(behaviour.instantly_switch_on_recover)
    .bind(behaviour.auto_switch_notification)
    .bind(behaviour.retry_attempts)
    .bind(behaviour.auto_stop_on_raid)
    .bind(behaviour.announce_raid)
    .bind(behaviour.record_while_streaming)
    .bind(behaviour.recording_scene_ids)
    .bind(behaviour.switch_to_starting)
    .bind(behaviour.switch_to_live)
    .bind(behaviour.offline_timeout)
    .bind(behaviour.ending_timeout)
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}

/// OBS scene UUIDs for each role. `None` means not set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scenes {
    pub live: Option<Uuid>,
    pub low: Option<Uuid>,
    pub offline: Option<Uuid>,
    pub starting: Option<Uuid>,
    pub ending: Option<Uuid>,
    pub privacy: Option<Uuid>,
    pub refresh: Option<Uuid>,
}

#[tracing::instrument(skip(pool))]
pub async fn set_scenes(pool: &PgPool, switcher_id: Uuid, scenes: Scenes) -> Result<Switcher> {
    let switcher = sqlx::query_as::<_, Switcher>(
        "
        UPDATE switchers
        SET scene_live = $2, scene_low = $3, scene_offline = $4, scene_starting = $5,
            scene_ending = $6, scene_privacy = $7, scene_refresh = $8
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(scenes.live)
    .bind(scenes.low)
    .bind(scenes.offline)
    .bind(scenes.starting)
    .bind(scenes.ending)
    .bind(scenes.privacy)
    .bind(scenes.refresh)
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}

/// What to do with a stored secret (the OBS websocket password, the BELABOX
/// remote key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretChange {
    Keep,
    Clear,
    /// The new value, already encrypted (see `common::crypto`).
    Set(Vec<u8>),
}

#[tracing::instrument(skip(pool, password))]
pub async fn set_obs(
    pool: &PgPool,
    switcher_id: Uuid,
    port: i32,
    password: SecretChange,
) -> Result<Switcher> {
    let (replace, value) = match password {
        SecretChange::Keep => (false, None),
        SecretChange::Clear => (true, None),
        SecretChange::Set(encrypted) => (true, Some(encrypted)),
    };

    let switcher = sqlx::query_as::<_, Switcher>(
        "
        UPDATE switchers
        SET port = $2, password = CASE WHEN $3 THEN $4 ELSE password END
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(port)
    .bind(replace)
    .bind(value)
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}

/// Whether a device (BELABOX, Moblin) starts and stops with the OBS stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamLink {
    pub start: bool,
    pub stop: bool,
}

/// Sets the BELABOX Cloud remote key and whether BELABOX follows the OBS stream.
#[tracing::instrument(skip(pool, remote_key))]
pub async fn set_belabox(
    pool: &PgPool,
    switcher_id: Uuid,
    remote_key: SecretChange,
    link: StreamLink,
) -> Result<Switcher> {
    let (replace, value) = match remote_key {
        SecretChange::Keep => (false, None),
        SecretChange::Clear => (true, None),
        SecretChange::Set(encrypted) => (true, Some(encrypted)),
    };

    let switcher = sqlx::query_as::<_, Switcher>(
        "
        UPDATE switchers
        SET belabox_start_with_stream = $2,
            belabox_stop_with_stream = $3,
            belabox_remote_key = CASE WHEN $4 THEN $5 ELSE belabox_remote_key END
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(link.start)
    .bind(link.stop)
    .bind(replace)
    .bind(value)
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}

impl Switcher {
    /// The BELABOX interface names, by interface key; malformed values read as none.
    pub fn interface_labels(&self) -> BTreeMap<String, String> {
        serde_json::from_value(self.belabox_interface_labels.clone()).unwrap_or_default()
    }
}

/// Replaces the names of the BELABOX's interfaces (key → name).
#[tracing::instrument(skip(pool, labels))]
pub async fn set_belabox_interface_labels(
    pool: &PgPool,
    switcher_id: Uuid,
    labels: &BTreeMap<String, String>,
) -> Result<Switcher> {
    let switcher = sqlx::query_as::<_, Switcher>(
        "
        UPDATE switchers
        SET belabox_interface_labels = $2
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(serde_json::to_value(labels).expect("labels serialize to JSON"))
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}

/// Chat settings stored on the switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSettings {
    pub command_prefix: String,
    pub language: String,
    pub public_commands_enabled: bool,
    pub mod_commands_enabled: bool,
    /// Answer commands as replies to them rather than as plain messages.
    pub threaded_replies: bool,
}

#[tracing::instrument(skip(pool))]
pub async fn set_chat_settings(
    pool: &PgPool,
    switcher_id: Uuid,
    settings: &ChatSettings,
) -> Result<Switcher> {
    let switcher = sqlx::query_as::<_, Switcher>(
        "
        UPDATE switchers
        SET command_prefix = $2, language = $3, public_commands_enabled = $4,
            mod_commands_enabled = $5, threaded_replies = $6
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(&settings.command_prefix)
    .bind(&settings.language)
    .bind(settings.public_commands_enabled)
    .bind(settings.mod_commands_enabled)
    .bind(settings.threaded_replies)
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}

/// Replaces the connect token, disconnecting browser sources using the old one.
#[tracing::instrument(skip(pool, connect_token_hash))]
pub async fn set_connect_token_hash(
    pool: &PgPool,
    switcher_id: Uuid,
    connect_token_hash: &[u8],
) -> Result<()> {
    sqlx::query("UPDATE switchers SET connect_token_hash = $2 WHERE id = $1")
        .bind(switcher_id)
        .bind(connect_token_hash)
        .execute(pool)
        .await?;

    Ok(())
}

#[tracing::instrument(skip(pool))]
pub async fn delete(pool: &PgPool, switcher_id: Uuid) -> Result<bool> {
    let result = sqlx::query("DELETE FROM switchers WHERE id = $1")
        .bind(switcher_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Finds the switcher a browser source belongs to, by its connect token's hash.
#[tracing::instrument(skip(pool, connect_token_hash))]
pub async fn by_connect_token_hash(
    pool: &PgPool,
    connect_token_hash: &[u8],
) -> Result<Option<Switcher>> {
    let switcher =
        sqlx::query_as::<_, Switcher>("SELECT * FROM switchers WHERE connect_token_hash = $1")
            .bind(connect_token_hash)
            .fetch_optional(pool)
            .await?;

    Ok(switcher)
}

/// Returns the switcher, or `None` if it was deleted.
#[tracing::instrument(skip(pool))]
pub async fn get(pool: &PgPool, switcher_id: Uuid) -> Result<Option<Switcher>> {
    let switcher = sqlx::query_as::<_, Switcher>("SELECT * FROM switchers WHERE id = $1")
        .bind(switcher_id)
        .fetch_optional(pool)
        .await?;

    Ok(switcher)
}

/// Records that the switcher's browser source is (or just was) connected.
#[tracing::instrument(skip(pool))]
pub async fn touch_last_seen(pool: &PgPool, switcher_id: Uuid) -> Result<()> {
    sqlx::query("UPDATE switchers SET last_seen_at = now() WHERE id = $1")
        .bind(switcher_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Turns automatic switching on or off.
#[tracing::instrument(skip(pool))]
pub async fn set_auto_switch(pool: &PgPool, switcher_id: Uuid, enabled: bool) -> Result<Switcher> {
    let switcher = sqlx::query_as::<_, Switcher>(
        "UPDATE switchers SET bitrate_switcher_enabled = $2 WHERE id = $1 RETURNING *",
    )
    .bind(switcher_id)
    .bind(enabled)
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}

/// Whether the switcher is paused: over its workspace plan's switcher limit
/// (after a downgrade the newest ones beyond the limit, until the workspace
/// upgrades or deletes others), or its workspace is suspended by staff. A client
/// workspace is only paused by suspension (its own or its organization's): the
/// organization's pool and its allocations stop new switchers, never running
/// ones, since pausing a customer's stream would hurt the organization most.
/// obs-link doesn't run it and remote control refuses it. `false` for a switcher
/// that doesn't exist.
#[tracing::instrument(skip(pool))]
pub async fn is_paused(pool: &PgPool, switcher_id: Uuid) -> Result<bool> {
    let paused = sqlx::query_scalar::<_, bool>(
        "
        SELECT (workspaces.organization_id IS NULL AND ranked.rank > plans.max_switchers)
               OR plans.suspended_at IS NOT NULL
        FROM (
            SELECT id, workspace_id,
                   row_number() OVER (PARTITION BY workspace_id ORDER BY created_at, id) AS rank
            FROM switchers
            WHERE workspace_id = (SELECT workspace_id FROM switchers WHERE id = $1)
        ) ranked
        JOIN workspaces ON workspaces.id = ranked.workspace_id
        JOIN workspace_plans plans ON plans.workspace_id = workspaces.id
        WHERE ranked.id = $1
        ",
    )
    .bind(switcher_id)
    .fetch_optional(pool)
    .await?;

    Ok(paused.unwrap_or(false))
}

/// An on/off setting chat commands can flip (`!notify`, `!autostop`, `!public`, `!mod`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatFlag {
    AutoSwitchNotification,
    AutoStopOnRaid,
    PublicCommands,
    ModCommands,
}

#[tracing::instrument(skip(pool))]
pub async fn set_chat_flag(
    pool: &PgPool,
    switcher_id: Uuid,
    flag: ChatFlag,
    value: bool,
) -> Result<Switcher> {
    let query = match flag {
        ChatFlag::AutoSwitchNotification => {
            "UPDATE switchers SET auto_switch_notification = $2 WHERE id = $1 RETURNING *"
        }
        ChatFlag::AutoStopOnRaid => {
            "UPDATE switchers SET auto_stop_on_raid = $2 WHERE id = $1 RETURNING *"
        }
        ChatFlag::PublicCommands => {
            "UPDATE switchers SET public_commands_enabled = $2 WHERE id = $1 RETURNING *"
        }
        ChatFlag::ModCommands => {
            "UPDATE switchers SET mod_commands_enabled = $2 WHERE id = $1 RETURNING *"
        }
    };
    let switcher = sqlx::query_as::<_, Switcher>(query)
        .bind(switcher_id)
        .bind(value)
        .fetch_one(pool)
        .await?;
    Ok(switcher)
}

/// Sets the Moblin remote control password and whether Moblin follows the OBS
/// stream.
#[tracing::instrument(skip(pool, password))]
pub async fn set_moblin(
    pool: &PgPool,
    switcher_id: Uuid,
    password: SecretChange,
    link: StreamLink,
) -> Result<Switcher> {
    let (replace, value) = match password {
        SecretChange::Keep => (false, None),
        SecretChange::Clear => (true, None),
        SecretChange::Set(encrypted) => (true, Some(encrypted)),
    };

    let switcher = sqlx::query_as::<_, Switcher>(
        "
        UPDATE switchers
        SET moblin_start_with_stream = $2,
            moblin_stop_with_stream = $3,
            moblin_password = CASE WHEN $4 THEN $5 ELSE moblin_password END
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(link.start)
    .bind(link.stop)
    .bind(replace)
    .bind(value)
    .fetch_one(pool)
    .await?;

    Ok(switcher)
}
