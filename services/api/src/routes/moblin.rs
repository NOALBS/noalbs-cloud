//! Remote control of a switcher's Moblin (connected to obs-link as a remote
//! control streamer; see `control::Control::moblin`). Its live status comes with
//! the switcher's live feed (`moblin` events); the password is saved with `PUT
//! …/moblin` in `switchers.rs`.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use live::MoblinAction;
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{SwitcherAuthorized, SwitcherControl},
    routes::{
        ApiResult, ErrorBody,
        feed::{SetActive, control_error},
    },
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(set_moblin_streaming))
        .routes(routes!(set_moblin_recording))
        .routes(routes!(set_moblin_muted))
        .routes(routes!(set_moblin_bitrate_preset))
        .routes(routes!(set_moblin_scene))
        .routes(routes!(set_moblin_zoom))
        .routes(routes!(set_moblin_zoom_preset))
        .routes(routes!(set_moblin_torch))
        .routes(routes!(set_moblin_stealth_mode))
        .routes(routes!(set_moblin_connection_priorities))
        .routes(routes!(set_moblin_connection))
}

async fn send(
    state: &ApiState,
    authorized: SwitcherAuthorized<SwitcherControl>,
    action: MoblinAction,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let by = access.workspace.display_name(state).await?;
    state
        .control()
        .moblin(access.switcher.id, action, &by)
        .await
        .map_err(control_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Go live (`active: true`) or stop, on Moblin.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/stream", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetActive,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = NOT_FOUND, description = "`moblin_not_set_up`", body = ErrorBody),
        (status = CONFLICT, description = "`moblin_already_streaming`, `moblin_not_streaming`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
        (status = GATEWAY_TIMEOUT, description = "`moblin_timeout`", body = ErrorBody),
    ))]
async fn set_moblin_streaming(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetActive>,
) -> ApiResult<StatusCode> {
    let action = MoblinAction::Stream {
        active: request.active,
    };
    send(&state, authorized, action).await
}

/// Start (`active: true`) or stop recording on the phone.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/record", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetActive,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_recording(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetActive>,
) -> ApiResult<StatusCode> {
    let action = MoblinAction::Record {
        active: request.active,
    };
    send(&state, authorized, action).await
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetMoblinMuted {
    pub muted: bool,
}

/// Mute or unmute Moblin's microphone.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/mute", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetMoblinMuted,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_muted(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetMoblinMuted>,
) -> ApiResult<StatusCode> {
    let action = MoblinAction::Mute {
        muted: request.muted,
    };
    send(&state, authorized, action).await
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetMoblinChoice {
    /// The ID from the Moblin status (`bitrate_presets[].id` or `scenes[].id`).
    pub id: String,
}

/// Use one of Moblin's bitrate presets.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/bitrate-preset", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetMoblinChoice,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = NOT_FOUND, description = "`unknown_preset`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_bitrate_preset(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetMoblinChoice>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized,
        MoblinAction::BitratePreset { id: request.id },
    )
    .await
}

/// Switch Moblin's scene.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/scene", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetMoblinChoice,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = NOT_FOUND, description = "`unknown_scene`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_scene(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetMoblinChoice>,
) -> ApiResult<StatusCode> {
    send(&state, authorized, MoblinAction::Scene { id: request.id }).await
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetMoblinZoom {
    /// The zoom factor, 0.5–100 (Moblin clamps it to what the camera can do).
    pub x: f64,
}

/// Zoom Moblin's camera.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/zoom", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetMoblinZoom,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = BAD_REQUEST, description = "`invalid_zoom`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_zoom(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetMoblinZoom>,
) -> ApiResult<StatusCode> {
    send(&state, authorized, MoblinAction::Zoom { x: request.x }).await
}

/// Use one of the current camera's zoom presets.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/zoom-preset", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetMoblinChoice,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = NOT_FOUND, description = "`unknown_zoom_preset`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_zoom_preset(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetMoblinChoice>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized,
        MoblinAction::ZoomPreset { id: request.id },
    )
    .await
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetMoblinOn {
    pub on: bool,
}

/// Turn the phone's light on or off.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/torch", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetMoblinOn,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_torch(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetMoblinOn>,
) -> ApiResult<StatusCode> {
    send(&state, authorized, MoblinAction::Torch { on: request.on }).await
}

/// Stealth mode: Moblin's screen shows a black (or custom) image instead of the
/// camera. The stream isn't affected.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/stealth-mode", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetMoblinOn,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_stealth_mode(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetMoblinOn>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized,
        MoblinAction::StealthMode { on: request.on },
    )
    .await
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetMoblinConnectionPriorities {
    pub enabled: bool,
}

/// Turn Moblin's connection priorities on or off. Off, Moblin uses every
/// connection alike (the connections' on/off and priorities don't apply).
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/connection-priorities", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetMoblinConnectionPriorities,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_connection_priorities(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetMoblinConnectionPriorities>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized,
        MoblinAction::ConnectionPriorities {
            enabled: request.enabled,
        },
    )
    .await
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetMoblinConnection {
    pub enabled: bool,
    /// 1–10; leave out to keep it.
    pub priority: Option<u8>,
}

/// Turn one of Moblin's bonding connections (Cellular, WiFi, a Moblink relay, …)
/// on or off, or set its priority. Applies while connection priorities are on.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/connections/{connection_id}", tag = "moblin",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("connection_id" = String, Path)),
    request_body = SetMoblinConnection,
    responses(
        (status = NO_CONTENT, description = "Done by Moblin"),
        (status = BAD_REQUEST, description = "`invalid_priority`", body = ErrorBody),
        (status = NOT_FOUND, description = "`unknown_connection`", body = ErrorBody),
        (status = CONFLICT, description = "`last_connection`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`moblin_unreachable`", body = ErrorBody),
    ))]
async fn set_moblin_connection(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Path(path): Path<(Uuid, Uuid, String)>,
    Json(request): Json<SetMoblinConnection>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized,
        MoblinAction::Connection {
            id: path.2,
            enabled: request.enabled,
            priority: request.priority,
        },
    )
    .await
}
