//! Live data and remote control for a switcher: the server-sent event feed,
//! the activity log, and OBS actions relayed through obs-link.

use std::{convert::Infallible, time::Duration};

use async_nats::jetstream::{
    consumer::{DeliverPolicy, pull},
    kv::Operation,
};
use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures_util::{Stream, StreamExt, stream};
use live::{BelaboxRefusal, EventDetail, MoblinRefusal, SwitcherEvent};
use obs_actions::{ObsClient, SceneSource};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{SwitcherAuthorized, SwitcherControl, SwitcherView},
    routes::{ApiError, ApiResult, ErrorBody},
    screenshots,
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(live_feed))
        .routes(routes!(list_events))
        .routes(routes!(obs_scenes))
        .routes(routes!(obs_sources))
        .routes(routes!(set_source))
        .routes(routes!(obs_audio))
        .routes(routes!(set_input_muted))
        .routes(routes!(set_input_volume))
        .routes(routes!(audio_feed))
        .routes(routes!(obs_screenshot))
        .routes(routes!(obs_stats))
        .routes(routes!(switch_scene))
        .routes(routes!(set_streaming))
        .routes(routes!(set_recording))
        .routes(routes!(fix_feed))
        .routes(routes!(set_auto_switch))
}

const DEFAULT_EVENTS: i64 = 50;
const MAX_EVENTS: i64 = 200;

fn nats_error(error: impl std::fmt::Display) -> ApiError {
    tracing::error!(%error, "NATS operation failed");
    ApiError::internal()
}

fn status_event(value: Option<&[u8]>) -> Event {
    let data = value.map_or_else(
        || "null".to_owned(),
        |bytes| String::from_utf8_lossy(bytes).into_owned(),
    );
    Event::default().event("status").data(data)
}

/// A device's status event (`belabox`, `moblin`): its KV value, or `null`.
fn device_event(name: &'static str, value: Option<&[u8]>) -> Event {
    let data = value.map_or_else(
        || "null".to_owned(),
        |bytes| String::from_utf8_lossy(bytes).into_owned(),
    );
    Event::default().event(name).data(data)
}

/// Watches a device's status bucket as `name` events.
fn device_updates(
    name: &'static str,
    watch: async_nats::jetstream::kv::Watch,
) -> impl Stream<Item = Event> {
    watch.filter_map(move |entry| async move {
        match entry {
            Ok(entry) if entry.operation == Operation::Put => {
                Some(device_event(name, Some(&entry.value)))
            }
            Ok(_) => Some(device_event(name, None)),
            Err(error) => {
                tracing::warn!(%error, name, "Device status watch failed");
                None
            }
        }
    })
}

/// Live status, stats and activity for a switcher, as server-sent events.
///
/// - `status`: a `SwitcherStatus`, or `null` while no browser source is connected.
///   Sent first, then on every change.
/// - `belabox`: a `BelaboxStatus`, or `null` while no BELABOX link runs (no remote
///   key, or no browser source). Sent second, then on every change (about once a
///   second while the BELABOX is online).
/// - `moblin`: a `MoblinStatus`, or `null` while no Moblin is connected. Sent
///   third, then on every change.
/// - `stats`: a `StatsSample` per second; the last two minutes are replayed first.
/// - `event`: a `SwitcherEvent` as it happens.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/live", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses(
        (status = OK, description = "Server-sent events", content_type = "text/event-stream"),
        (status = FORBIDDEN, body = ErrorBody),
    ))]
async fn live_feed(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let switcher_id = authorized.into_access().switcher.id;
    let key = live::status_key(switcher_id);

    // Subscribe before reading the current status, so no change slips through.
    let status_updates = state.stores.status.watch(&key).await.map_err(nats_error)?;
    let belabox_updates = state.stores.belabox.watch(&key).await.map_err(nats_error)?;
    let moblin_updates = state.stores.moblin.watch(&key).await.map_err(nats_error)?;
    let events = state
        .nats
        .subscribe(live::event_subject(switcher_id))
        .await
        .map_err(nats_error)?;
    let current_status = state.stores.status.get(&key).await.map_err(nats_error)?;
    let current_belabox = state.stores.belabox.get(&key).await.map_err(nats_error)?;
    let current_moblin = state.stores.moblin.get(&key).await.map_err(nats_error)?;
    let stats = state
        .stores
        .stats
        .create_consumer(pull::OrderedConfig {
            filter_subject: live::stats_subject(switcher_id),
            deliver_policy: DeliverPolicy::ByStartTime {
                start_time: time::OffsetDateTime::now_utc() - live::STATS_RETENTION,
            },
            ..Default::default()
        })
        .await
        .map_err(nats_error)?
        .messages()
        .await
        .map_err(nats_error)?;

    let status_updates = status_updates.filter_map(|entry| async move {
        match entry {
            Ok(entry) if entry.operation == Operation::Put => {
                Some(status_event(Some(&entry.value)))
            }
            // Deleted on disconnect, or expired because obs-link stopped.
            Ok(_) => Some(status_event(None)),
            Err(error) => {
                tracing::warn!(%error, "Status watch failed");
                None
            }
        }
    });
    let device_updates = stream::select(
        device_updates("belabox", belabox_updates),
        device_updates("moblin", moblin_updates),
    );
    let stats = stats.filter_map(|message| async move {
        let message = message.ok()?;
        Some(
            Event::default()
                .event("stats")
                .data(String::from_utf8_lossy(&message.payload)),
        )
    });
    let events = events.map(|message| {
        Event::default()
            .event("event")
            .data(String::from_utf8_lossy(&message.payload))
    });

    let initial = [
        status_event(current_status.as_deref()),
        device_event("belabox", current_belabox.as_deref()),
        device_event("moblin", current_moblin.as_deref()),
    ];
    let feed = stream::iter(initial)
        .chain(stream::select(
            stream::select(status_updates, device_updates),
            stream::select(stats, events),
        ))
        .map(Ok);

    Ok(Sse::new(feed).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct EventsQuery {
    /// At most this many events (default 50, up to 200).
    pub limit: Option<i64>,
    /// Only events before this time, for paging back.
    pub before: Option<chrono::DateTime<chrono::Utc>>,
}

/// The activity feed, newest first.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/events", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), EventsQuery),
    responses((status = OK, body = Vec<SwitcherEvent>), (status = FORBIDDEN, body = ErrorBody)))]
async fn list_events(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Query(query): Query<EventsQuery>,
) -> ApiResult<Json<Vec<SwitcherEvent>>> {
    let limit = query.limit.unwrap_or(DEFAULT_EVENTS).clamp(1, MAX_EVENTS);
    let rows = db::switcher_events::list(
        &state.db,
        authorized.into_access().switcher.id,
        query.before,
        limit,
    )
    .await?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|row| {
                Some(SwitcherEvent {
                    id: row.id,
                    created_at: row.created_at,
                    detail: EventDetail::from_parts(&row.kind, row.data)?,
                })
            })
            .collect(),
    ))
}

/// The switcher's OBS, for reading: 503 `obs_offline` unless it's connected.
async fn connected_obs(
    state: &ApiState,
    switcher_id: Uuid,
) -> ApiResult<ObsClient<async_nats::Client>> {
    state
        .control()
        .connected_obs(switcher_id)
        .await
        .map_err(control_error)
}

/// How a refused or failed OBS action is answered.
pub fn control_error(error: control::Error) -> ApiError {
    match error {
        control::Error::Paused => ApiError::forbidden(
            "plan_limit",
            "This switcher is paused: the workspace has more switchers than its plan includes.",
        ),
        control::Error::ObsOffline => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "obs_offline",
            "OBS isn't connected. Check that OBS is open with the browser source active.",
        ),
        control::Error::ObsRefused { code, comment } => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "obs_error",
            comment.unwrap_or_else(|| format!("OBS refused the request (code {code})")),
        ),
        control::Error::ObsUnreachable(error) => {
            tracing::warn!(%error, "OBS request failed");
            ApiError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "obs_timeout",
                "OBS didn't answer. Try again in a moment.",
            )
        }
        control::Error::NoRefreshScene => ApiError::new(
            StatusCode::CONFLICT,
            "no_refresh_scene",
            "Pick a Refresh scene on the Scenes page first.",
        ),
        control::Error::AlreadyRefreshing => ApiError::new(
            StatusCode::CONFLICT,
            "already_refreshing",
            "The Refresh scene is already on air.",
        ),
        control::Error::NoSceneIds => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "obs_error",
            "This OBS version doesn't report scene IDs. Update OBS to do this from here.",
        ),
        control::Error::SourceNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "source_not_found",
            "That source isn't in the scene anymore. Reload the list.",
        ),
        control::Error::InputNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "input_not_found",
            "OBS has no audio source by that name anymore. Reload the list.",
        ),
        control::Error::InvalidVolume => ApiError::bad_request(
            "invalid_volume",
            "The volume must be between -100 and 26 dB.",
        ),
        control::Error::NotStreaming => ApiError::new(
            StatusCode::CONFLICT,
            "not_streaming",
            "The switcher isn't streaming. Markers go on the running stream.",
        ),
        control::Error::InvalidMarker => ApiError::bad_request(
            "invalid_description",
            "A marker's description is at most 200 characters.",
        ),
        control::Error::BelaboxNotSetUp => ApiError::new(
            StatusCode::NOT_FOUND,
            "belabox_not_set_up",
            "BELABOX Cloud isn't set up for this switcher. Add your BELABOX remote secret URL in the switcher's settings.",
        ),
        control::Error::BelaboxUnreachable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "belabox_unreachable",
            "The BELABOX link isn't running. Check that OBS is open with the browser source active.",
        ),
        control::Error::BelaboxRefused(refusal) => belabox_refusal(refusal),
        control::Error::MoblinNotSetUp => ApiError::new(
            StatusCode::NOT_FOUND,
            "moblin_not_set_up",
            "Moblin isn't set up for this switcher. Set it up in the switcher's settings.",
        ),
        control::Error::MoblinUnreachable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "moblin_unreachable",
            "Moblin isn't connected. Check its remote control settings (Streamer).",
        ),
        control::Error::MoblinRefused(refusal) => moblin_refusal(refusal),
        control::Error::Db(error) => error.into(),
    }
}

fn moblin_refusal(refusal: MoblinRefusal) -> ApiError {
    let (status, code, message) = match refusal {
        MoblinRefusal::NotConnected => (
            StatusCode::SERVICE_UNAVAILABLE,
            "moblin_unreachable",
            "Moblin isn't connected. Check its remote control settings (Streamer).",
        ),
        MoblinRefusal::Paused => (
            StatusCode::FORBIDDEN,
            "plan_limit",
            "This switcher is paused: the workspace has more switchers than its plan includes.",
        ),
        MoblinRefusal::AlreadyStreaming => (
            StatusCode::CONFLICT,
            "moblin_already_streaming",
            "Moblin is already live.",
        ),
        MoblinRefusal::NotStreaming => (
            StatusCode::CONFLICT,
            "moblin_not_streaming",
            "Moblin isn't live.",
        ),
        MoblinRefusal::UnknownScene => (
            StatusCode::NOT_FOUND,
            "unknown_scene",
            "Moblin has no such scene (anymore).",
        ),
        MoblinRefusal::UnknownZoomPreset => (
            StatusCode::NOT_FOUND,
            "unknown_zoom_preset",
            "Moblin has no such zoom preset (anymore).",
        ),
        MoblinRefusal::InvalidZoom => (
            StatusCode::BAD_REQUEST,
            "invalid_zoom",
            "The zoom must be between 0.5x and 100x.",
        ),
        MoblinRefusal::UnknownConnection => (
            StatusCode::NOT_FOUND,
            "unknown_connection",
            "Moblin has no such connection (anymore).",
        ),
        MoblinRefusal::LastConnection => (
            StatusCode::CONFLICT,
            "last_connection",
            "The last connection in use can't be turned off.",
        ),
        MoblinRefusal::InvalidPriority => (
            StatusCode::BAD_REQUEST,
            "invalid_priority",
            "The priority must be between 1 and 10.",
        ),
        MoblinRefusal::UnknownPreset => (
            StatusCode::NOT_FOUND,
            "unknown_preset",
            "Moblin has no such bitrate preset (anymore).",
        ),
        MoblinRefusal::Failed => (
            StatusCode::BAD_GATEWAY,
            "moblin_error",
            "Moblin refused the request.",
        ),
        MoblinRefusal::Timeout => (
            StatusCode::GATEWAY_TIMEOUT,
            "moblin_timeout",
            "Moblin didn't answer. Try again in a moment.",
        ),
    };
    ApiError::new(status, code, message)
}

fn belabox_refusal(refusal: BelaboxRefusal) -> ApiError {
    let (status, code, message) = match refusal {
        BelaboxRefusal::Connecting => (
            StatusCode::SERVICE_UNAVAILABLE,
            "belabox_connecting",
            "Still connecting to BELABOX Cloud. Try again in a moment.",
        ),
        BelaboxRefusal::InvalidKey => (
            StatusCode::CONFLICT,
            "belabox_invalid_key",
            "BELABOX Cloud refused the secret URL. Check it in the switcher's settings.",
        ),
        BelaboxRefusal::EncoderOffline => (
            StatusCode::SERVICE_UNAVAILABLE,
            "belabox_offline",
            "The BELABOX is offline. Check that it's on and connected to the internet.",
        ),
        BelaboxRefusal::NoConfig => (
            StatusCode::CONFLICT,
            "belabox_no_config",
            "The BELABOX has no stream settings yet. Start it once from its own page.",
        ),
        BelaboxRefusal::AlreadyStreaming => (
            StatusCode::CONFLICT,
            "belabox_already_streaming",
            "The BELABOX is already streaming.",
        ),
        BelaboxRefusal::NotStreaming => (
            StatusCode::CONFLICT,
            "belabox_not_streaming",
            "The BELABOX isn't streaming.",
        ),
        BelaboxRefusal::InvalidBitrate => (
            StatusCode::BAD_REQUEST,
            "invalid_bitrate",
            "The bitrate must be between 300 and 12000 kbps.",
        ),
        BelaboxRefusal::UnknownInterface => (
            StatusCode::NOT_FOUND,
            "unknown_interface",
            "The BELABOX has no such network interface (anymore).",
        ),
        BelaboxRefusal::LastInterface => (
            StatusCode::CONFLICT,
            "last_interface",
            "The last enabled network interface can't be turned off.",
        ),
        BelaboxRefusal::UnknownWifi => (
            StatusCode::NOT_FOUND,
            "unknown_wifi",
            "The BELABOX has no such WiFi adapter or network (anymore).",
        ),
        BelaboxRefusal::UnknownModem => (
            StatusCode::NOT_FOUND,
            "unknown_modem",
            "The BELABOX has no such modem, or it has no SIM card.",
        ),
        BelaboxRefusal::Streaming => (
            StatusCode::CONFLICT,
            "belabox_streaming",
            "Stream settings can't change while the BELABOX streams. Stop it first.",
        ),
        BelaboxRefusal::Invalid(field) => {
            return ApiError::bad_request(field.code(), format!("Invalid {field}."));
        }
    };
    ApiError::new(status, code, message)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ObsSceneDto {
    /// `null` on OBS versions that don't report scene UUIDs.
    pub uuid: Option<Uuid>,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ObsScenesDto {
    pub current_scene_uuid: Option<Uuid>,
    pub current_scene_name: String,
    /// In the order OBS lists them, top first.
    pub scenes: Vec<ObsSceneDto>,
}

/// The scenes in the switcher's OBS.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scenes", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses(
        (status = OK, body = ObsScenesDto),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
    ))]
async fn obs_scenes(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Json<ObsScenesDto>> {
    let obs = connected_obs(&state, authorized.into_access().switcher.id).await?;
    let mut list = obs
        .scene_list()
        .await
        .map_err(|error| control_error(error.into()))?;
    // obs-websocket lists scenes bottom first.
    list.scenes
        .sort_by(|a, b| b.scene_index.cmp(&a.scene_index));

    Ok(Json(ObsScenesDto {
        current_scene_uuid: list.current_program_scene_uuid,
        current_scene_name: list.current_program_scene_name,
        scenes: list
            .scenes
            .into_iter()
            .map(|scene| ObsSceneDto {
                uuid: scene.scene_uuid,
                name: scene.scene_name,
            })
            .collect(),
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ObsSourceDto {
    /// The scene or group it sits in; send it back to show or hide it.
    pub parent: String,
    pub parent_is_group: bool,
    /// Unique within `parent`.
    pub item_id: i64,
    pub name: String,
    /// `null` on OBS versions that don't report source UUIDs.
    pub uuid: Option<Uuid>,
    /// The input's kind (e.g. `ffmpeg_source`); `null` for scenes and groups.
    pub input_kind: Option<String>,
    /// A nested scene (its items follow it, one level deeper).
    pub is_scene: bool,
    /// A group (its items follow it, one level deeper).
    pub is_group: bool,
    /// Shown (the eye in OBS's Sources dock).
    pub enabled: bool,
    /// 0 on the scene on air, 1 inside a nested scene or group, …
    pub depth: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ObsSourcesDto {
    /// The scene on air.
    pub scene: String,
    /// Top first like OBS's Sources dock, each nested scene's or group's items
    /// right after it. A scene nested twice is only expanded the first time.
    pub sources: Vec<ObsSourceDto>,
}

/// Everything on the scene on air, nested scenes and groups included.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/sources", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses(
        (status = OK, body = ObsSourcesDto),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
    ))]
async fn obs_sources(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Json<ObsSourcesDto>> {
    let (scene, items) = state
        .control()
        .sources(authorized.into_access().switcher.id)
        .await
        .map_err(control_error)?;

    Ok(Json(ObsSourcesDto {
        scene,
        sources: items
            .into_iter()
            .map(|item| ObsSourceDto {
                parent: item.parent,
                parent_is_group: item.parent_is_group,
                item_id: item.item_id,
                name: item.name,
                uuid: item.uuid,
                input_kind: item.input_kind,
                is_scene: item.is_scene,
                is_group: item.is_group,
                enabled: item.enabled,
                depth: item.depth,
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetSource {
    /// The source's `parent`, `parent_is_group` and `item_id` from the list.
    pub parent: String,
    #[serde(default)]
    pub parent_is_group: bool,
    pub item_id: i64,
    /// Show (`true`) or hide it.
    pub enabled: bool,
}

/// Show or hide a source, like `!source`.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/source", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetSource,
    responses(
        (status = NO_CONTENT, description = "Done (or it already was)"),
        (status = NOT_FOUND, description = "`source_not_found`: it's gone from that scene", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`obs_error`", body = ErrorBody),
    ))]
async fn set_source(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetSource>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let by = access.workspace.display_name(&state).await?;
    state
        .control()
        .set_source_enabled(
            access.switcher.id,
            &request.parent,
            request.parent_is_group,
            request.item_id,
            request.enabled,
            &by,
        )
        .await
        .map_err(control_error)?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ObsAudioInputDto {
    /// What mute requests and `AudioUpdate`s name it by.
    pub name: String,
    /// `null` for the global audio devices and on OBS versions before 30.
    pub uuid: Option<Uuid>,
    /// The input's kind (e.g. `ffmpeg_source`); `null` for the global devices.
    pub input_kind: Option<String>,
    /// A global audio device (OBS Settings → Audio), on every scene.
    pub global: bool,
    pub muted: bool,
    /// The fader, in dB (-100 all the way down).
    pub volume_db: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ObsAudioDto {
    /// The scene on air.
    pub scene: String,
    /// Like OBS's Audio Mixer: the global devices, then the inputs with audio
    /// shown on the scene (nested scenes and groups included), each once.
    pub inputs: Vec<ObsAudioInputDto>,
}

/// The inputs on OBS's Audio Mixer for the scene on air, with their mute state.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/audio", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses(
        (status = OK, body = ObsAudioDto),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
    ))]
async fn obs_audio(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Json<ObsAudioDto>> {
    let (scene, inputs) = state
        .control()
        .audio(authorized.into_access().switcher.id)
        .await
        .map_err(control_error)?;

    Ok(Json(ObsAudioDto {
        scene,
        inputs: inputs
            .into_iter()
            .map(|input| ObsAudioInputDto {
                name: input.name,
                uuid: input.uuid,
                input_kind: input.input_kind,
                global: input.global,
                muted: input.muted,
                volume_db: input.volume_db,
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetInputMuted {
    /// The input's `name` from the mixer.
    pub input: String,
    pub muted: bool,
}

/// Mute or unmute an input on OBS's Audio Mixer.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/audio/mute", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetInputMuted,
    responses(
        (status = NO_CONTENT, description = "Done (or it already was)"),
        (status = NOT_FOUND, description = "`input_not_found`: renamed, removed or without audio", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`obs_error`", body = ErrorBody),
    ))]
async fn set_input_muted(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetInputMuted>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let by = access.workspace.display_name(&state).await?;
    state
        .control()
        .set_input_muted(access.switcher.id, &request.input, request.muted, &by)
        .await
        .map_err(control_error)?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetInputVolume {
    /// The input's `name` from the mixer.
    pub input: String,
    /// -100 (all the way down) to 26; OBS's own fader stops at 0.
    pub volume_db: f64,
}

/// Move an input's fader on OBS's Audio Mixer. Not logged in the activity
/// feed: dragging sends many.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/audio/volume", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetInputVolume,
    responses(
        (status = NO_CONTENT, description = "Done"),
        (status = BAD_REQUEST, description = "`invalid_volume`", body = ErrorBody),
        (status = NOT_FOUND, description = "`input_not_found`: renamed, removed or without audio", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`obs_error`", body = ErrorBody),
    ))]
async fn set_input_volume(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetInputVolume>,
) -> ApiResult<StatusCode> {
    state
        .control()
        .set_input_volume(
            authorized.into_access().switcher.id,
            &request.input,
            request.volume_db,
        )
        .await
        .map_err(control_error)?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct AudioFeedQuery {
    /// `false`: only mute and fader changes, without asking OBS for levels.
    pub levels: Option<bool>,
}

/// Audio levels and mute/fader changes for the mixer, as server-sent `audio`
/// events (an `AudioUpdate` each). OBS sends levels only while someone keeps
/// one of these open (they cost the streamer's upload), about 5 a second;
/// inputs missing from a `levels` update are silent.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/audio/live", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), AudioFeedQuery),
    responses(
        (status = OK, description = "Server-sent events", content_type = "text/event-stream"),
        (status = FORBIDDEN, body = ErrorBody),
    ))]
async fn audio_feed(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Query(query): Query<AudioFeedQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let levels = query.levels.unwrap_or(true);
    let switcher_id = authorized.into_access().switcher.id;
    let updates = state
        .nats
        .subscribe(live::audio_subject(switcher_id))
        .await
        .map_err(nats_error)?
        // Someone else may have levels on: not for a viewer who turned them off.
        .filter(move |message| {
            let wanted = levels
                || !matches!(
                    serde_json::from_slice(&message.payload),
                    Ok(live::AudioUpdate::Levels { .. })
                );
            async move { wanted }
        })
        .map(|message| {
            Event::default()
                .event("audio")
                .data(String::from_utf8_lossy(&message.payload))
        });

    // Tells obs-link someone watches, now and then until the viewer leaves
    // (the stream is dropped with the response).
    let nats = state.nats.clone();
    let demand = stream::unfold(
        tokio::time::interval(live::AUDIO_DEMAND_INTERVAL),
        move |mut interval| {
            let nats = nats.clone();
            async move {
                if !levels {
                    return None;
                }
                interval.tick().await;
                let subject = live::audio_demand_subject(switcher_id);
                if let Err(error) = nats.publish(subject, Bytes::new()).await {
                    tracing::warn!(%error, "Couldn't ask for audio levels");
                }
                Some(((), interval))
            }
        },
    )
    .filter_map(|()| async { None::<Event> });

    let feed = stream::select(updates, demand).map(Ok);
    Ok(Sse::new(feed).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ScreenshotQuery {
    /// Width in pixels (160 to 1280, default 640); the height keeps the aspect ratio.
    pub width: Option<u32>,
    /// JPEG quality, 1 (smallest) to 100 (best); default 60.
    pub quality: Option<u8>,
}

/// A JPEG still of the scene on air, for the Live page's preview. Each one is
/// sent by the streamer's OBS computer, so the dashboard asks at the rate the
/// viewer picked; a frame from the last quarter second is shared between viewers.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/screenshot", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ScreenshotQuery),
    responses(
        (status = OK, description = "The image", content_type = "image/jpeg", body = Vec<u8>),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`obs_error`", body = ErrorBody),
    ))]
async fn obs_screenshot(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Query(query): Query<ScreenshotQuery>,
) -> ApiResult<Response> {
    let switcher_id = authorized.into_access().switcher.id;
    let (width, quality) = screenshots::settings(query.width, query.quality);
    let key = (switcher_id, width, quality);
    let frame = match state.screenshots.fresh(key) {
        Some(frame) => frame,
        None => {
            let obs = connected_obs(&state, switcher_id).await?;
            // The scene on air, as obs-link last reported it.
            let scene = state
                .status_of(switcher_id)
                .await
                .and_then(|status| status.current_scene)
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "obs_offline",
                        "OBS hasn't reported its scene yet.",
                    )
                })?;
            let source = match scene.uuid {
                Some(uuid) => SceneSource::Uuid(uuid),
                None => SceneSource::Name(&scene.name),
            };
            let uri = obs
                .scene_screenshot(source, width, i32::from(quality))
                .await
                .map_err(|error| control_error(error.into()))?;
            let frame = Bytes::from(screenshots::decode_data_uri(&uri).ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "obs_error",
                    "OBS sent an image we can't read.",
                )
            })?);
            state.screenshots.store(key, frame.clone());
            frame
        }
    };

    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        frame,
    )
        .into_response())
}

/// Frames dropped (or skipped) for one cause, out of all.
#[derive(Debug, Serialize, ToSchema)]
pub struct ObsDroppedDto {
    pub frames: u64,
    pub total: u64,
}

impl From<control::Dropped> for ObsDroppedDto {
    fn from(dropped: control::Dropped) -> Self {
        Self {
            frames: dropped.frames,
            total: dropped.total,
        }
    }
}

/// OBS's stats, like its Stats dock (and NOALBS's `!serverinfo`).
#[derive(Debug, Serialize, ToSchema)]
pub struct ObsStatsDto {
    pub streaming: bool,
    /// The stream output lost its connection and is retrying.
    pub reconnecting: bool,
    /// Bytes the stream output sent so far, and for how long it has run: two
    /// readings give the bitrate.
    pub stream_bytes: u64,
    pub stream_duration_ms: u64,
    /// 0–1: how far the stream output is behind.
    pub congestion: f64,
    /// Dropped by the stream output (this stream).
    pub network: ObsDroppedDto,
    /// Missed due to rendering lag.
    pub rendering: ObsDroppedDto,
    /// Skipped due to encoding lag.
    pub encoding: ObsDroppedDto,
    /// `rendering` and `encoding` count this stream's frames; otherwise since
    /// OBS started.
    pub this_stream: bool,
    pub fps: f64,
    /// Average time to render a frame, in ms.
    pub frame_render_ms: f64,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    /// Free space on the drive of OBS's recording path, in MB.
    pub disk_free_mb: f64,
}

/// OBS's stats right now: frame rate, dropped frames by cause, CPU, memory and
/// disk, plus the stream output's byte count for a bitrate between two readings.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/stats", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses(
        (status = OK, body = ObsStatsDto),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
    ))]
async fn obs_stats(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Json<ObsStatsDto>> {
    let stats = state
        .control()
        .obs_stats(authorized.into_access().switcher.id)
        .await
        .map_err(control_error)?;
    Ok(Json(ObsStatsDto {
        streaming: stats.streaming,
        reconnecting: stats.reconnecting,
        stream_bytes: stats.stream_bytes,
        stream_duration_ms: stats.stream_duration_ms,
        congestion: stats.congestion,
        network: stats.network.into(),
        rendering: stats.rendering.into(),
        encoding: stats.encoding.into(),
        this_stream: stats.this_stream,
        fps: stats.fps,
        frame_render_ms: stats.frame_render_ms,
        cpu_percent: stats.cpu_percent,
        memory_mb: stats.memory_mb,
        disk_free_mb: stats.disk_free_mb,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SwitchScene {
    pub scene_uuid: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetActive {
    pub active: bool,
}

/// Switch OBS to a scene.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SwitchScene,
    responses(
        (status = NO_CONTENT, description = "Switched"),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`obs_error`", body = ErrorBody),
    ))]
async fn switch_scene(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SwitchScene>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let by = access.workspace.display_name(&state).await?;
    state
        .control()
        .switch_scene(access.switcher.id, request.scene_uuid, &by)
        .await
        .map_err(control_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Start (`active: true`) or stop the stream.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/stream", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetActive,
    responses(
        (status = NO_CONTENT, description = "Done"),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`obs_error`, e.g. already streaming", body = ErrorBody),
    ))]
async fn set_streaming(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetActive>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let by = access.workspace.display_name(&state).await?;
    state
        .control()
        .stream(access.switcher.id, request.active, &by)
        .await
        .map_err(control_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Start (`active: true`) or stop recording.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/record", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetActive,
    responses(
        (status = NO_CONTENT, description = "Done"),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`obs_error`", body = ErrorBody),
    ))]
async fn set_recording(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetActive>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let by = access.workspace.display_name(&state).await?;
    state
        .control()
        .record(access.switcher.id, request.active, &by)
        .await
        .map_err(control_error)?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FixedFeed {
    /// The scene that was on air.
    pub scene: String,
    /// The media sources told to reconnect; empty when none on the scene reads
    /// a network stream (rtmp, srt, udp, rist, rtsp).
    pub sources: Vec<String>,
}

/// Fix the feed like `!fix`: the network media sources playing on the scene on
/// air (nested scenes and groups included) reconnect. Unfreezes a feed stuck
/// after a drop.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/fix", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses(
        (status = OK, description = "Done (OBS reconnects the sources)", body = FixedFeed),
        (status = SERVICE_UNAVAILABLE, description = "`obs_offline`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`obs_error`", body = ErrorBody),
    ))]
async fn fix_feed(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
) -> ApiResult<Json<FixedFeed>> {
    let access = authorized.into_access();
    let by = access.workspace.display_name(&state).await?;
    let (scene, sources) = state
        .control()
        .fix(access.switcher.id, &by)
        .await
        .map_err(control_error)?;

    Ok(Json(FixedFeed { scene, sources }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetAutoSwitch {
    pub enabled: bool,
}

/// Turn automatic switching on or off. Operators may do this mid-stream.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/auto-switch", tag = "live",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetAutoSwitch,
    responses((status = NO_CONTENT, description = "Saved"), (status = FORBIDDEN, body = ErrorBody)))]
async fn set_auto_switch(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetAutoSwitch>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let switcher_id = access.switcher.id;
    if access.switcher.bitrate_switcher_enabled == request.enabled {
        return Ok(StatusCode::NO_CONTENT);
    }

    db::switchers::set_auto_switch(&state.db, switcher_id, request.enabled).await?;
    state.notify_config(switcher_id).await;
    let by = access.workspace.display_name(&state).await?;
    live::record_event(
        &state.db,
        &state.nats,
        switcher_id,
        EventDetail::AutoSwitchChanged {
            enabled: request.enabled,
            by: Some(by),
        },
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}
