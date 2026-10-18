//! Admin → Status: what's running right now, read live from obs-link's status
//! bucket, the chat service's coordination buckets and the platforms' APIs.
//! Read-only. A part that can't be read (a platform down, the chat service never
//! started) reports its error; the rest still shows.

use std::collections::{BTreeMap, HashMap};

use async_nats::jetstream;
use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, TryStreamExt};
use live::{FeedState, SwitcherStatus};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    oauth::{self, LoginPlatform, PlatformError},
    permissions::Staff,
    routes::{ApiResult, ErrorBody, chat_bots::ChatPlatform},
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(admin_status))
}

/// How many connected switchers are listed at most.
const MAX_CONNECTED: usize = 200;

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminStatusDto {
    pub generated_at: DateTime<Utc>,
    pub switchers: SwitcherCountsDto,
    /// Switchers with their browser source connected: live ones first (longest-running
    /// first), then those with OBS connected.
    pub connected: Vec<ConnectedSwitcherDto>,
    /// Set when obs-link's status bucket couldn't be read.
    pub switchers_error: Option<String>,
    pub chat: ChatStatusDto,
}

#[derive(Debug, Default, Serialize, ToSchema)]
pub struct SwitcherCountsDto {
    /// Every switcher there is.
    pub total: i64,
    /// Browser source connected to obs-link.
    pub connected: i64,
    pub obs_connected: i64,
    pub streaming: i64,
    pub recording: i64,
    /// Over their plan's switcher limit.
    pub paused: i64,
    /// Streaming on the LOW scene's feed, or offline.
    pub low: i64,
    pub offline: i64,
}

/// A switcher whose browser source is connected.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConnectedSwitcherDto {
    pub switcher_id: Uuid,
    pub switcher_name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
    /// The workspace owner's most recently used sign-in name.
    pub owner_username: Option<String>,
    pub obs_connected: bool,
    pub streaming: bool,
    pub recording: bool,
    pub paused: bool,
    pub state: FeedState,
    pub stream_started_at: Option<DateTime<Utc>>,
    pub scene: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChatStatusDto {
    /// This database's chat environment (the chat service's NATS namespace).
    pub environment: String,
    /// Running chat service instances; empty if none is.
    pub instances: Vec<ChatInstanceDto>,
    /// Set when the chat service's buckets couldn't be read (e.g. it never ran).
    pub instances_error: Option<String>,
    /// Channels in use (linked and not revoked), and links to switchers.
    pub active_channels: i64,
    pub links: i64,
    /// `null` when Twitch isn't configured.
    pub twitch: Option<TwitchStatusDto>,
    /// `null` when Kick isn't configured.
    pub kick: Option<KickStatusDto>,
    /// `null` when YouTube isn't configured.
    pub youtube: Option<YouTubeStatusDto>,
    /// Channels and bots whose grant was refused: someone has to connect them again.
    pub revoked: Vec<RevokedGrantDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ChatInstanceDto {
    pub id: String,
    /// Seconds since its last heartbeat (every 10 s; gone after 30 s).
    pub last_seen_seconds: i64,
    /// The Twitch conduit shards it holds.
    pub shards: Vec<u32>,
}

#[derive(Debug, Default, Serialize, ToSchema)]
pub struct TwitchStatusDto {
    pub conduit_id: Option<String>,
    pub shards: Vec<ShardDto>,
    /// All of the app's EventSub subscriptions, by status (`enabled`, or why not).
    pub subscriptions_by_status: Vec<NamedCountDto>,
    /// Those delivered through our conduit, by type.
    pub conduit_subscriptions_by_type: Vec<NamedCountDto>,
    /// Cost against Twitch's limit (subscriptions nobody authorised cost 1).
    pub total_cost: i64,
    pub max_total_cost: i64,
    /// What the chat service recorded having made.
    pub recorded: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ShardDto {
    pub id: String,
    /// Twitch's status for it: `enabled`, or e.g. `websocket_disconnected`.
    pub status: String,
    /// The instance holding it, if any.
    pub instance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct NamedCountDto {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Default, Serialize, ToSchema)]
pub struct KickStatusDto {
    /// Event subscriptions the Kick app has; `null` if Kick couldn't be asked.
    pub subscriptions: Option<i64>,
    pub recorded: i64,
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize, ToSchema)]
pub struct YouTubeStatusDto {
    /// The instance following the live chats, as it last said (every 10 s);
    /// `null` if none has yet.
    pub watcher: Option<live::YouTubeWatcherSummary>,
    /// The last error YouTube answered with (kept a day).
    pub last_error: Option<live::YouTubeLastError>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RevokedGrantDto {
    /// `bot` or `channel`.
    pub kind: String,
    pub platform: ChatPlatform,
    pub login: String,
    pub workspace_id: Option<Uuid>,
    pub workspace_name: Option<String>,
    /// Switchers still linked to a revoked channel.
    pub switchers: i64,
    pub revoked_at: DateTime<Utc>,
}

/// What's running now: switchers, the chat service, Twitch, Kick and YouTube.
#[utoipa::path(get, path = "/admin/status", tag = "admin",
    responses((status = OK, body = AdminStatusDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn admin_status(
    State(state): State<ApiState>,
    _staff: Staff,
) -> ApiResult<Json<AdminStatusDto>> {
    let counts = db::admin::chat_counts(&state.db).await?;

    let (statuses, switchers_error) = match switcher_statuses(&state).await {
        Ok(statuses) => (statuses, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    let mut switchers = count_switchers(&statuses);
    switchers.total = counts.switchers;
    let connected = connected_switchers(&state, &statuses).await?;

    let js = jetstream::new(state.nats.clone());
    let environment = state.chat_environment.clone();
    let (heartbeats, claims, instances_error) = match chat_buckets(&js, &environment).await {
        Ok((heartbeats, claims)) => (heartbeats, claims, None),
        Err(error) => (Vec::new(), Vec::new(), Some(error)),
    };
    let instances = instances(
        &environment,
        &heartbeats,
        &claims,
        time::OffsetDateTime::now_utc(),
    );

    let twitch = if state.oauth.is_configured(LoginPlatform::Twitch) {
        Some(twitch_status(&state, &claims, counts.twitch_subscriptions).await?)
    } else {
        None
    };
    let kick = if state.oauth.is_configured(LoginPlatform::Kick) {
        Some(kick_status(&state, counts.kick_subscriptions).await)
    } else {
        None
    };
    let youtube = if state.oauth.is_configured(LoginPlatform::YouTube) {
        Some(youtube_status(&js, &environment).await)
    } else {
        None
    };
    let revoked = db::admin::revoked_grants(&state.db)
        .await?
        .into_iter()
        .map(|grant| RevokedGrantDto {
            kind: grant.kind,
            platform: grant.platform.into(),
            login: grant.login,
            workspace_id: grant.workspace_id,
            workspace_name: grant.workspace_name,
            switchers: grant.switchers,
            revoked_at: grant.updated_at,
        })
        .collect();

    Ok(Json(AdminStatusDto {
        generated_at: Utc::now(),
        switchers,
        connected,
        switchers_error,
        chat: ChatStatusDto {
            environment,
            instances,
            instances_error,
            active_channels: counts.active_channels,
            links: counts.links,
            twitch,
            kick,
            youtube,
            revoked,
        },
    }))
}

/// Every connected switcher's status, from obs-link's bucket.
async fn switcher_statuses(state: &ApiState) -> Result<Vec<(Uuid, SwitcherStatus)>, String> {
    let keys: Vec<String> = state
        .stores
        .status
        .keys()
        .await
        .map_err(|error| error.to_string())?
        .try_collect()
        .await
        .map_err(|error| error.to_string())?;
    let kv = &state.stores.status;
    let statuses = futures_util::stream::iter(keys)
        .map(|key| async move {
            let id = Uuid::parse_str(&key).ok()?;
            let bytes = kv.get(&key).await.ok()??;
            Some((id, serde_json::from_slice::<SwitcherStatus>(&bytes).ok()?))
        })
        .buffer_unordered(32)
        .filter_map(std::future::ready)
        .collect()
        .await;
    Ok(statuses)
}

fn count_switchers(statuses: &[(Uuid, SwitcherStatus)]) -> SwitcherCountsDto {
    let mut counts = SwitcherCountsDto::default();
    for (_, status) in statuses {
        let add = |yes: bool| i64::from(yes);
        counts.connected += 1;
        counts.obs_connected += add(status.obs_connected);
        counts.streaming += add(status.streaming);
        counts.recording += add(status.recording);
        counts.paused += add(status.paused);
        counts.low += add(status.streaming && status.state == FeedState::Low);
        counts.offline += add(status.streaming && status.state == FeedState::Offline);
    }
    counts
}

async fn connected_switchers(
    state: &ApiState,
    statuses: &[(Uuid, SwitcherStatus)],
) -> ApiResult<Vec<ConnectedSwitcherDto>> {
    let by_id: HashMap<Uuid, &SwitcherStatus> =
        statuses.iter().map(|(id, status)| (*id, status)).collect();
    let ids: Vec<Uuid> = by_id.keys().copied().collect();
    let mut connected: Vec<ConnectedSwitcherDto> = db::admin::switchers_by_ids(&state.db, &ids)
        .await?
        .into_iter()
        .filter_map(|switcher| {
            let status = by_id.get(&switcher.id)?;
            Some(ConnectedSwitcherDto {
                switcher_id: switcher.id,
                switcher_name: switcher.name,
                workspace_id: switcher.workspace_id,
                workspace_name: switcher.workspace_name,
                workspace_slug: switcher.workspace_slug,
                owner_username: switcher.owner_username,
                obs_connected: status.obs_connected,
                streaming: status.streaming,
                recording: status.recording,
                paused: status.paused,
                state: status.state,
                stream_started_at: status.stream_started_at,
                scene: status
                    .current_scene
                    .as_ref()
                    .map(|scene| scene.name.clone()),
            })
        })
        .collect();
    connected.sort_by(|a, b| {
        // Live first (longest-running first), then with OBS, then by name.
        (
            !a.streaming,
            a.stream_started_at,
            !a.obs_connected,
            &a.workspace_name,
        )
            .cmp(&(
                !b.streaming,
                b.stream_started_at,
                !b.obs_connected,
                &b.workspace_name,
            ))
    });
    connected.truncate(MAX_CONNECTED);
    Ok(connected)
}

/// The chat service's heartbeats `(key, written)` and shard claims `(key, holder)`
/// of this environment.
type Heartbeats = Vec<(String, time::OffsetDateTime)>;
type Claims = Vec<(String, String)>;

async fn chat_buckets(
    js: &jetstream::Context,
    environment: &str,
) -> Result<(Heartbeats, Claims), String> {
    let prefix = format!("{environment}.");
    let never_ran = |_| "The chat service hasn't run on this NATS yet".to_owned();
    let instances = js
        .get_key_value(live::CHAT_INSTANCES_BUCKET)
        .await
        .map_err(never_ran)?;
    let shards = js
        .get_key_value(live::CHAT_SHARDS_BUCKET)
        .await
        .map_err(never_ran)?;

    let mut heartbeats = Vec::new();
    for key in keys_with_prefix(&instances, &prefix).await? {
        if let Ok(Some(entry)) = instances.entry(&key).await {
            heartbeats.push((key, entry.created));
        }
    }
    let mut claims = Vec::new();
    for key in keys_with_prefix(&shards, &prefix).await? {
        if let Ok(Some(value)) = shards.get(&key).await {
            claims.push((key, String::from_utf8_lossy(&value).into_owned()));
        }
    }
    Ok((heartbeats, claims))
}

async fn keys_with_prefix(
    store: &jetstream::kv::Store,
    prefix: &str,
) -> Result<Vec<String>, String> {
    let keys: Vec<String> = store
        .keys()
        .await
        .map_err(|error| error.to_string())?
        .try_collect()
        .await
        .map_err(|error| error.to_string())?;
    Ok(keys
        .into_iter()
        .filter(|key| key.starts_with(prefix))
        .collect())
}

/// What the chat service keeps about YouTube (see `live::CHAT_YOUTUBE_STATUS_BUCKET`).
async fn youtube_status(js: &jetstream::Context, environment: &str) -> YouTubeStatusDto {
    let store = match js.get_key_value(live::CHAT_YOUTUBE_STATUS_BUCKET).await {
        Ok(store) => store,
        Err(_) => {
            return YouTubeStatusDto {
                error: Some(
                    "The chat service hasn't followed YouTube chats on this NATS yet".into(),
                ),
                ..Default::default()
            };
        }
    };
    let mut status = YouTubeStatusDto::default();
    match store.get(live::chat_key(&[environment, "watcher"])).await {
        Ok(value) => status.watcher = value.and_then(|value| serde_json::from_slice(&value).ok()),
        Err(error) => status.error = Some(error.to_string()),
    }
    match store.get(live::chat_key(&[environment, "error"])).await {
        Ok(value) => {
            status.last_error = value.and_then(|value| serde_json::from_slice(&value).ok());
        }
        Err(error) => status.error = Some(error.to_string()),
    }
    status
}

/// The shard number of a claim key `<environment>.shard.<n>`.
fn shard_number(environment: &str, key: &str) -> Option<u32> {
    key.strip_prefix(environment)?
        .strip_prefix(".shard.")?
        .parse()
        .ok()
}

/// Instances from their heartbeats, with the shards each claims (claims store
/// the plain instance ID; heartbeat keys hold it as a KV key).
fn instances(
    environment: &str,
    heartbeats: &[(String, time::OffsetDateTime)],
    claims: &[(String, String)],
    now: time::OffsetDateTime,
) -> Vec<ChatInstanceDto> {
    let mut instances: Vec<ChatInstanceDto> = heartbeats
        .iter()
        .filter_map(|(key, written)| {
            let id = key.strip_prefix(environment)?.strip_prefix('.')?;
            let mut shards: Vec<u32> = claims
                .iter()
                .filter(|(_, holder)| live::chat_key(&[environment, holder]) == *key)
                .filter_map(|(claim, _)| shard_number(environment, claim))
                .collect();
            shards.sort_unstable();
            Some(ChatInstanceDto {
                id: id.to_owned(),
                last_seen_seconds: (now - *written).whole_seconds().max(0),
                shards,
            })
        })
        .collect();
    instances.sort_by(|a, b| a.id.cmp(&b.id));
    instances
}

fn counts(names: impl IntoIterator<Item = String>) -> Vec<NamedCountDto> {
    let mut counted: BTreeMap<String, i64> = BTreeMap::new();
    for name in names {
        *counted.entry(name).or_default() += 1;
    }
    counted
        .into_iter()
        .map(|(name, count)| NamedCountDto { name, count })
        .collect()
}

async fn twitch_status(
    state: &ApiState,
    claims: &[(String, String)],
    recorded: i64,
) -> ApiResult<TwitchStatusDto> {
    let conduit_id = db::chat::platform_state(&state.db, "twitch_conduit_id").await?;
    let mut status = TwitchStatusDto {
        conduit_id: conduit_id.clone(),
        recorded,
        ..TwitchStatusDto::default()
    };
    let result: Result<(), PlatformError> = async {
        let provider = state
            .oauth
            .provider(LoginPlatform::Twitch)
            .map_err(|error| PlatformError {
                unauthorized: false,
                message: error.message().to_owned(),
            })?;
        let token = state
            .oauth
            .app_token(&state.http, LoginPlatform::Twitch)
            .await
            .map_err(|error| PlatformError {
                unauthorized: false,
                message: error.message().to_owned(),
            })?;
        if let Some(conduit_id) = &conduit_id {
            let holders: HashMap<u32, &str> = claims
                .iter()
                .filter_map(|(key, holder)| {
                    Some((shard_number(&state.chat_environment, key)?, holder.as_str()))
                })
                .collect();
            status.shards =
                oauth::twitch::conduit_shards(&state.http, provider, &token, conduit_id)
                    .await?
                    .into_iter()
                    .map(|shard| ShardDto {
                        instance: shard
                            .id
                            .parse::<u32>()
                            .ok()
                            .and_then(|n| holders.get(&n))
                            .map(|holder| (*holder).to_owned()),
                        id: shard.id,
                        status: shard.status,
                    })
                    .collect();
        }
        let (subscriptions, total_cost, max_total_cost) =
            oauth::twitch::subscriptions(&state.http, provider, &token).await?;
        status.total_cost = total_cost;
        status.max_total_cost = max_total_cost;
        status.subscriptions_by_status = counts(subscriptions.iter().map(|sub| sub.status.clone()));
        status.conduit_subscriptions_by_type = counts(
            subscriptions
                .iter()
                .filter(|sub| {
                    conduit_id.is_some()
                        && sub.transport.get("conduit_id").and_then(|id| id.as_str())
                            == conduit_id.as_deref()
                })
                .map(|sub| sub.kind.clone()),
        );
        Ok(())
    }
    .await;
    if let Err(error) = result {
        if error.unauthorized {
            state.oauth.forget_app_token(LoginPlatform::Twitch);
        }
        status.error = Some(error.message);
    }
    Ok(status)
}

async fn kick_status(state: &ApiState, recorded: i64) -> KickStatusDto {
    let mut status = KickStatusDto {
        recorded,
        ..KickStatusDto::default()
    };
    let result = async {
        let provider = state.oauth.provider(LoginPlatform::Kick).ok()?;
        let token = match state
            .oauth
            .app_token(&state.http, LoginPlatform::Kick)
            .await
        {
            Ok(token) => token,
            Err(error) => return Some(Err(error.message().to_owned())),
        };
        Some(
            oauth::kick::subscription_count(&state.http, provider, &token)
                .await
                .map_err(|error| {
                    if error.unauthorized {
                        state.oauth.forget_app_token(LoginPlatform::Kick);
                    }
                    error.message
                }),
        )
    }
    .await;
    match result {
        Some(Ok(count)) => status.subscriptions = Some(count as i64),
        Some(Err(error)) => status.error = Some(error),
        None => {}
    }
    status
}

#[cfg(test)]
mod tests {
    use time::{Duration, OffsetDateTime};

    use super::{ChatInstanceDto, instances, shard_number};

    #[test]
    fn shard_claims_are_matched_to_their_instances() {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::hours(1);
        let heartbeats = [
            ("env1.chat-a".to_owned(), now - Duration::seconds(4)),
            // An instance ID with characters a key can't hold.
            ("env1.vps_2".to_owned(), now - Duration::seconds(12)),
        ];
        let claims = [
            ("env1.shard.2".to_owned(), "chat-a".to_owned()),
            ("env1.shard.0".to_owned(), "chat-a".to_owned()),
            ("env1.shard.1".to_owned(), "vps:2".to_owned()),
            // A claim whose instance stopped heartbeating isn't anyone's.
            ("env1.shard.3".to_owned(), "gone".to_owned()),
        ];
        assert_eq!(
            instances("env1", &heartbeats, &claims, now),
            [
                ChatInstanceDto {
                    id: "chat-a".into(),
                    last_seen_seconds: 4,
                    shards: vec![0, 2],
                },
                ChatInstanceDto {
                    id: "vps_2".into(),
                    last_seen_seconds: 12,
                    shards: vec![1],
                },
            ]
        );
        assert_eq!(shard_number("env1", "env1.shard.12"), Some(12));
        assert_eq!(shard_number("env1", "env2.shard.1"), None);
    }
}
