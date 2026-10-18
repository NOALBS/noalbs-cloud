use chrono::DateTime;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "stream_server_kind", rename_all = "UPPERCASE")]
pub enum StreamServerKind {
    Nginx,
    Nms,
    Nimble,
    Sls,
    Belabox,
    Mediamtx,
    Xiu,
    Openirl,
    Openrtmp,
    Rist,
    Irlhosting,
    #[sqlx(rename = "IRLHOSTING_WEBSOCKET")]
    IrlhostingWebsocket,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct StreamServer {
    pub id: Uuid,
    pub switcher_id: Uuid,

    pub name: String,
    pub priority: i32,
    pub enabled: bool,
    pub stats_url: String,
    pub details: serde_json::Value,
    pub kind: StreamServerKind,

    pub override_scene_live: Option<Uuid>,
    pub override_scene_low: Option<Uuid>,
    pub override_scene_offline: Option<Uuid>,

    pub created_at: DateTime<chrono::Utc>,
    pub updated_at: DateTime<chrono::Utc>,
}

/// Kind-specific settings, kept in the `details` jsonb column. Which fields a kind
/// uses is up to the kind (see the API's validation and obs-link's fetchers):
/// e.g. SLS reads `publisher`, NGINX `application` + `key`, MediaMTX `username` +
/// password. Secrets are stored encrypted with `common::crypto::DataKey`
/// (`encrypt_to_string`) and never returned by the API.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Details {
    /// Stream ID / publisher name on SRT servers (SLS, IRLHosting); the feed on
    /// IRLHosting WebSocket.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// RTMP application (NGINX, NMS, Nimble, Xiu, IRLHosting).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    /// RTMP stream key (NGINX, NMS, Nimble, Xiu, IRLHosting).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Nimble's SRT listener ID, usually `ip:port`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listener_id: Option<String>,
    /// Basic-auth username (MediaMTX, NMS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Basic-auth password, encrypted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_password: Option<String>,
    /// SLS API key (sent as `Authorization`) or IRLHosting WebSocket token (sent
    /// as `?token=`), encrypted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_api_key: Option<String>,
}

impl Details {
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("details serialize to JSON")
    }
}

impl StreamServer {
    /// The typed `details`; unknown or malformed values read as empty.
    pub fn details(&self) -> Details {
        serde_json::from_value(self.details.clone()).unwrap_or_default()
    }
}

#[tracing::instrument(skip(pool))]
pub async fn create(
    pool: &PgPool,
    switcher_id: Uuid,
    name: &str,
    kind: StreamServerKind,
) -> Result<StreamServer> {
    let stream_server = sqlx::query_as::<_, StreamServer>(
        "
        INSERT INTO stream_servers (
            switcher_id,
            name,
            kind
        )
        VALUES ($1, $2, $3)
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(name)
    .bind(kind)
    .fetch_one(pool)
    .await?;

    Ok(stream_server)
}

#[derive(Debug)]
pub struct UpdateStreamServer<'a> {
    pub name: Option<&'a str>,
    pub priority: Option<i32>,
    pub enabled: Option<bool>,
    pub stats_url: Option<&'a str>,
    pub details: Option<serde_json::Value>,
    pub override_scene_live: Option<Uuid>,
    pub override_scene_low: Option<Uuid>,
    pub override_scene_offline: Option<Uuid>,
}

#[tracing::instrument(skip(pool))]
pub async fn update(
    pool: &PgPool,
    stream_server_id: Uuid,
    update: UpdateStreamServer<'_>,
) -> Result<StreamServer> {
    let UpdateStreamServer {
        name,
        priority,
        enabled,
        stats_url,
        details,
        override_scene_live,
        override_scene_low,
        override_scene_offline,
    } = update;

    let stream_server = sqlx::query_as::<_, StreamServer>(
        "
        UPDATE stream_servers
        SET
            name = COALESCE($2, name),
            priority = COALESCE($3, priority),
            enabled = COALESCE($4, enabled),
            stats_url = COALESCE($5, stats_url),
            details = COALESCE($6, details),
            override_scene_live = COALESCE($7, override_scene_live),
            override_scene_low = COALESCE($8, override_scene_low),
            override_scene_offline = COALESCE($9, override_scene_offline)
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(stream_server_id)
    .bind(name)
    .bind(priority)
    .bind(enabled)
    .bind(stats_url)
    .bind(details)
    .bind(override_scene_live)
    .bind(override_scene_low)
    .bind(override_scene_offline)
    .fetch_one(pool)
    .await?;

    Ok(stream_server)
}

#[tracing::instrument(skip(pool))]
pub async fn by_switcher_id(pool: &PgPool, switcher_id: Uuid) -> Result<Vec<StreamServer>> {
    let stream_servers = sqlx::query_as::<_, StreamServer>(
        "
        SELECT *
        FROM stream_servers
        WHERE switcher_id = $1
        ORDER BY priority ASC
        ",
    )
    .bind(switcher_id)
    .fetch_all(pool)
    .await?;

    Ok(stream_servers)
}

/// Adds a stream server at the end of the switcher's priority order if its
/// workspace's plan has room for another one on it, returning `None` when it
/// doesn't. The switcher row is locked so concurrent adds can't exceed the limit.
#[tracing::instrument(skip(pool, stats_url, details))]
pub async fn create_within_plan(
    pool: &PgPool,
    switcher_id: Uuid,
    name: &str,
    kind: StreamServerKind,
    stats_url: &str,
    details: &Details,
) -> Result<Option<StreamServer>> {
    let mut tx = pool.begin().await?;

    let (max, count) = sqlx::query_as::<_, (i32, i64)>(
        "
        SELECT plans.max_stream_servers,
               (SELECT count(*) FROM stream_servers WHERE stream_servers.switcher_id = switchers.id)
        FROM switchers
        JOIN workspaces ON workspaces.id = switchers.workspace_id
        JOIN workspace_plans plans ON plans.workspace_id = workspaces.id
        WHERE switchers.id = $1
        FOR UPDATE OF switchers
        ",
    )
    .bind(switcher_id)
    .fetch_one(&mut *tx)
    .await?;
    if count >= i64::from(max) {
        return Ok(None);
    }

    let stream_server = create_last(&mut *tx, switcher_id, name, kind, stats_url, details).await?;
    tx.commit().await?;

    Ok(Some(stream_server))
}

/// Adds a stream server at the end of the switcher's priority order.
#[tracing::instrument(skip(executor, stats_url, details))]
pub async fn create_last<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    switcher_id: Uuid,
    name: &str,
    kind: StreamServerKind,
    stats_url: &str,
    details: &Details,
) -> Result<StreamServer> {
    let stream_server = sqlx::query_as::<_, StreamServer>(
        "
        INSERT INTO stream_servers (switcher_id, name, kind, stats_url, details, priority)
        VALUES (
            $1, $2, $3, $4, $5,
            (SELECT COALESCE(MAX(priority) + 1, 0) FROM stream_servers WHERE switcher_id = $1)
        )
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(name)
    .bind(kind)
    .bind(stats_url)
    .bind(details.to_value())
    .fetch_one(executor)
    .await?;

    Ok(stream_server)
}

/// Returns the stream server only when it belongs to `switcher_id`.
#[tracing::instrument(skip(pool))]
pub async fn get_in_switcher(
    pool: &PgPool,
    switcher_id: Uuid,
    stream_server_id: Uuid,
) -> Result<Option<StreamServer>> {
    let stream_server = sqlx::query_as::<_, StreamServer>(
        "SELECT * FROM stream_servers WHERE id = $1 AND switcher_id = $2",
    )
    .bind(stream_server_id)
    .bind(switcher_id)
    .fetch_optional(pool)
    .await?;

    Ok(stream_server)
}

/// Replaces the per-server scene overrides; `None` uses the switcher's scene.
#[tracing::instrument(skip(pool))]
pub async fn set_overrides(
    pool: &PgPool,
    stream_server_id: Uuid,
    live: Option<Uuid>,
    low: Option<Uuid>,
    offline: Option<Uuid>,
) -> Result<StreamServer> {
    let stream_server = sqlx::query_as::<_, StreamServer>(
        "
        UPDATE stream_servers
        SET override_scene_live = $2, override_scene_low = $3, override_scene_offline = $4
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(stream_server_id)
    .bind(live)
    .bind(low)
    .bind(offline)
    .fetch_one(pool)
    .await?;

    Ok(stream_server)
}

#[tracing::instrument(skip(pool))]
pub async fn delete(pool: &PgPool, switcher_id: Uuid, stream_server_id: Uuid) -> Result<bool> {
    let result = sqlx::query("DELETE FROM stream_servers WHERE id = $1 AND switcher_id = $2")
        .bind(stream_server_id)
        .bind(switcher_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Sets the priority order: `ids[0]` is tried first. Returns `false`, changing
/// nothing, unless `ids` is exactly the switcher's stream servers.
#[tracing::instrument(skip(pool))]
pub async fn reorder(pool: &PgPool, switcher_id: Uuid, ids: &[Uuid]) -> Result<bool> {
    let mut tx = pool.begin().await?;

    let mut current = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM stream_servers WHERE switcher_id = $1 FOR UPDATE",
    )
    .bind(switcher_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut requested = ids.to_vec();
    current.sort();
    requested.sort();
    requested.dedup();
    if current != requested || requested.len() != ids.len() {
        return Ok(false);
    }

    sqlx::query(
        "
        UPDATE stream_servers
        SET priority = ordered.position - 1
        FROM unnest($2::uuid[]) WITH ORDINALITY AS ordered(id, position)
        WHERE stream_servers.id = ordered.id AND stream_servers.switcher_id = $1
        ",
    )
    .bind(switcher_id)
    .bind(ids)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(true)
}
