use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{Duration, Utc};
use db::connect::{ConnectAccess, ConnectObsAction, Watchdog};
use live::EventDetail;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{CurrentUser, SwitcherAuthorized, SwitcherManage, SwitcherView},
    routes::{ApiError, ApiResult, ErrorBody, switchers::SwitcherDto},
    screenshots,
};

const PAIRING_LIFETIME_SECONDS: i64 = 600;

fn validate_device_name(name: &str) -> ApiResult<()> {
    let length = name.chars().count();
    if !(1..=100).contains(&length) {
        return Err(ApiError::bad_request(
            "invalid_name",
            "Device name must be between 1 and 100 characters",
        ));
    }
    Ok(())
}

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(create_pairing))
        .routes(routes!(poll_pairing))
        .routes(routes!(pairing_preview))
        .routes(routes!(confirm_pairing))
        .routes(routes!(list_devices, rename_device))
        .routes(routes!(revoke_device))
        .routes(routes!(set_permissions))
        .routes(routes!(set_watchdog))
        .routes(routes!(connect_obs_command))
        .routes(routes!(list_connect_logs))
        .routes(routes!(get_connect_log))
        .routes(routes!(upload_connect_log))
        .routes(routes!(delete_connect_log))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConnectLogDto {
    pub id: Uuid,
    pub kind: db::connect_logs::ConnectLogKind,
    pub file_name: String,
    pub file_modified_at: Option<chrono::DateTime<Utc>>,
    pub uploaded_at: chrono::DateTime<Utc>,
    pub size_bytes: i32,
    pub truncated: bool,
    pub device_name: Option<String>,
}

impl From<db::connect_logs::ConnectLogMetadata> for ConnectLogDto {
    fn from(row: db::connect_logs::ConnectLogMetadata) -> Self {
        Self {
            id: row.id,
            kind: row.kind,
            file_name: row.file_name,
            file_modified_at: row.file_modified_at,
            uploaded_at: row.uploaded_at,
            size_bytes: row.size_bytes,
            truncated: row.truncated,
            device_name: row.device_name,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConnectLogDetailDto {
    pub id: Uuid,
    pub kind: db::connect_logs::ConnectLogKind,
    pub file_name: String,
    pub file_modified_at: Option<chrono::DateTime<Utc>>,
    pub uploaded_at: chrono::DateTime<Utc>,
    pub size_bytes: i32,
    pub truncated: bool,
    pub device_name: Option<String>,
    pub content: String,
}

#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/logs", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = OK, body = [ConnectLogDto]), (status = FORBIDDEN, body = ErrorBody)))]
async fn list_connect_logs(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Json<Vec<ConnectLogDto>>> {
    let access = authorized.into_access();
    let viewer = screenshots::viewer_for(&state, &access.workspace).await?;
    require_connect_logs(access.switcher.connect_logs, viewer)?;
    let rows = db::connect_logs::list(&state.db, access.switcher.id).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/logs/{log_id}", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("log_id" = Uuid, Path)),
    responses((status = OK, body = ConnectLogDetailDto), (status = FORBIDDEN, body = ErrorBody), (status = NOT_FOUND, body = ErrorBody)))]
async fn get_connect_log(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Path((_workspace_id, _switcher_id, log_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<ConnectLogDetailDto>> {
    let access = authorized.into_access();
    let viewer = screenshots::viewer_for(&state, &access.workspace).await?;
    require_connect_logs(access.switcher.connect_logs, viewer)?;
    let log = db::connect_logs::get(&state.db, access.switcher.id, log_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Connect log not found"))?;
    let m = log.metadata;
    Ok(Json(ConnectLogDetailDto {
        id: m.id,
        kind: m.kind,
        file_name: m.file_name,
        file_modified_at: m.file_modified_at,
        uploaded_at: m.uploaded_at,
        size_bytes: m.size_bytes,
        truncated: m.truncated,
        device_name: m.device_name,
        content: log.content,
    }))
}

#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/logs", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = CREATED, body = ConnectLogDto), (status = FORBIDDEN, body = ErrorBody), (status = CONFLICT, body = ErrorBody), (status = GATEWAY_TIMEOUT, body = ErrorBody)))]
async fn upload_connect_log(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<(StatusCode, Json<ConnectLogDto>)> {
    let access = authorized.into_access();
    let viewer = screenshots::viewer_for(&state, &access.workspace).await?;
    require_connect_logs(access.switcher.connect_logs, viewer)?;
    let by = access.workspace.display_name(&state).await?;
    let payload = serde_json::to_vec(&live::ConnectObsCommandRequest {
        action: ConnectObsAction::UploadLog,
    })
    .map_err(|_| ApiError::internal())?;
    let response = telemetry::nats::request_with_timeout(
        &state.nats,
        live::connect_request_subject(access.switcher.id),
        payload,
        std::time::Duration::from_secs(35),
    )
    .await
    .map_err(|error| connect_request_error(error.kind()))?;
    let reply: live::ConnectObsCommandReply =
        serde_json::from_slice(&response.payload).map_err(|_| ApiError::internal())?;
    if !reply.ok {
        return Err(connect_command_failure(reply));
    }
    let id = reply.log_id.ok_or_else(ApiError::internal)?;
    let metadata = db::connect_logs::list(&state.db, access.switcher.id)
        .await?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(ApiError::internal)?;
    live::record_event(
        &state.db,
        &state.nats,
        access.switcher.id,
        EventDetail::ConnectLogUploaded {
            kind: metadata.kind,
            log_id: id,
            by: Some(by),
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(metadata.into())))
}

#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/logs/{log_id}", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("log_id" = Uuid, Path)),
    responses((status = NO_CONTENT), (status = FORBIDDEN, body = ErrorBody), (status = NOT_FOUND, body = ErrorBody)))]
async fn delete_connect_log(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Path((_workspace_id, _switcher_id, log_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let viewer = screenshots::viewer_for(&state, &access.workspace).await?;
    require_connect_logs(access.switcher.connect_logs, viewer)?;
    if !db::connect_logs::delete(&state.db, access.switcher.id, log_id).await? {
        return Err(ApiError::not_found("Connect log not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ConnectOs {
    Windows,
    Linux,
    Macos,
}

impl ConnectOs {
    fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "macos",
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreatePairing {
    pub name: String,
    pub os: ConnectOs,
    pub app_version: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PairingCreated {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u32,
    pub interval: u32,
}

/// Start the device authorization flow.
#[utoipa::path(post, path = "/connect/pair", tag = "connect",
    request_body = CreatePairing,
    responses(
        (status = OK, body = PairingCreated),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = TOO_MANY_REQUESTS, body = ErrorBody),
    ))]
async fn create_pairing(
    State(state): State<ApiState>,
    Json(request): Json<CreatePairing>,
) -> ApiResult<Json<PairingCreated>> {
    validate_device_name(&request.name)?;
    if request.app_version.chars().count() > 50 {
        return Err(ApiError::bad_request(
            "invalid_app_version",
            "App version must be at most 50 characters",
        ));
    }

    let device_code = common::tokens::generate();
    let user_code = common::tokens::user_code();
    let stored_user_code =
        db::connect::normalize_user_code(&user_code).expect("generated user code is valid");
    db::connect::create_pairing(
        &state.db,
        &common::tokens::hash_text(&device_code),
        &stored_user_code,
        &request.name,
        request.os.as_str(),
        &request.app_version,
        Utc::now() + Duration::seconds(PAIRING_LIFETIME_SECONDS),
    )
    .await?;

    Ok(Json(PairingCreated {
        device_code,
        verification_url: format!("{}/connect?code={user_code}", state.dashboard_url),
        user_code,
        expires_in: PAIRING_LIFETIME_SECONDS as u32,
        // Polls share the sign-in limit (one per 6 s refill), so stay below it.
        interval: 10,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PollPairing {
    pub device_code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PairingClaimed {
    pub device_id: Uuid,
    pub device_token: String,
    pub switcher_id: Uuid,
    pub link_url: String,
}

/// Poll for confirmation; the device token is returned exactly once.
#[utoipa::path(post, path = "/connect/pair/poll", tag = "connect",
    request_body = PollPairing,
    responses(
        (status = OK, body = PairingClaimed),
        (status = BAD_REQUEST, description = "`authorization_pending` or `expired_token`", body = ErrorBody),
        (status = TOO_MANY_REQUESTS, body = ErrorBody),
    ))]
async fn poll_pairing(
    State(state): State<ApiState>,
    Json(request): Json<PollPairing>,
) -> ApiResult<Json<PairingClaimed>> {
    let device_token = common::tokens::generate();
    let claim = db::connect::claim_pairing(
        &state.db,
        &common::tokens::hash_text(&request.device_code),
        &common::tokens::hash_text(&device_token),
    )
    .await?;
    match claim {
        db::connect::Claim::Expired => Err(ApiError::bad_request(
            "expired_token",
            "The device pairing code has expired",
        )),
        db::connect::Claim::Pending => Err(ApiError::bad_request(
            "authorization_pending",
            "Waiting for pairing confirmation",
        )),
        db::connect::Claim::Claimed {
            device,
            confirmed_by_name,
        } => {
            if let Err(error) = live::record_event(
                &state.db,
                &state.nats,
                device.switcher_id,
                EventDetail::ConnectDevicePaired {
                    name: device.name.clone(),
                    by: confirmed_by_name,
                },
            )
            .await
            {
                tracing::warn!(
                    %error,
                    switcher_id = %device.switcher_id,
                    "Failed to record a Connect device pairing"
                );
            }
            Ok(Json(PairingClaimed {
                device_id: device.id,
                device_token,
                switcher_id: device.switcher_id,
                link_url: state.links.link_url(),
            }))
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PairingPreview {
    pub name: String,
    pub os: String,
    pub app_version: String,
    pub expires_at: chrono::DateTime<Utc>,
}

/// Show the PC waiting for the signed-in user to pair it.
#[utoipa::path(get, path = "/connect/pair/{user_code}", tag = "connect",
    params(("user_code" = String, Path)),
    responses((status = OK, body = PairingPreview), (status = NOT_FOUND, body = ErrorBody)))]
async fn pairing_preview(
    State(state): State<ApiState>,
    _user: CurrentUser,
    Path(user_code): Path<String>,
) -> ApiResult<Json<PairingPreview>> {
    let pairing = db::connect::pairing_by_user_code(&state.db, &user_code)
        .await?
        .ok_or_else(|| ApiError::not_found("Pairing code not found or expired"))?;
    Ok(Json(PairingPreview {
        name: pairing.name,
        os: pairing.os,
        app_version: pairing.app_version,
        expires_at: pairing.expires_at,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConfirmPairing {
    pub user_code: String,
}

/// Pair this computer with the switcher.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/pair", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = ConfirmPairing,
    responses(
        (status = NO_CONTENT, description = "Pairing confirmed"),
        (status = NOT_FOUND, body = ErrorBody),
        (status = FORBIDDEN, body = ErrorBody),
    ))]
async fn confirm_pairing(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<ConfirmPairing>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    access.workspace.members_only()?;
    let by = access.workspace.display_name(&state).await?;
    let confirmed = db::connect::confirm_pairing(
        &state.db,
        &request.user_code,
        access.switcher.id,
        access.workspace.user.id(),
        &by,
    )
    .await?;
    if !confirmed {
        return Err(ApiError::not_found(
            "Pairing code not found or already confirmed",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConnectDeviceDto {
    pub id: Uuid,
    pub name: String,
    pub os: String,
    pub app_version: String,
    pub created_at: chrono::DateTime<Utc>,
    pub last_seen_at: Option<chrono::DateTime<Utc>>,
}

impl From<db::connect::ConnectDevice> for ConnectDeviceDto {
    fn from(device: db::connect::ConnectDevice) -> Self {
        Self {
            id: device.id,
            name: device.name,
            os: device.os,
            app_version: device.app_version,
            created_at: device.created_at,
            last_seen_at: device.last_seen_at,
        }
    }
}

/// List active Connect devices paired with this switcher.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/devices", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = OK, body = Vec<ConnectDeviceDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn list_devices(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Json<Vec<ConnectDeviceDto>>> {
    let switcher_id = authorized.into_access().switcher.id;
    let devices = db::connect::list_for_switcher(&state.db, switcher_id).await?;
    Ok(Json(devices.into_iter().map(Into::into).collect()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RenameDevice {
    pub name: String,
}

/// Rename a paired Connect device.
#[utoipa::path(patch, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/devices/{device_id}", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("device_id" = Uuid, Path)),
    request_body = RenameDevice,
    responses((status = OK, body = ConnectDeviceDto), (status = NOT_FOUND, body = ErrorBody), (status = FORBIDDEN, body = ErrorBody)))]
async fn rename_device(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path((_workspace_id, _switcher_id, device_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<RenameDevice>,
) -> ApiResult<Json<ConnectDeviceDto>> {
    let access = authorized.into_access();
    access.workspace.members_only()?;
    validate_device_name(&request.name)?;
    let device = db::connect::rename(&state.db, access.switcher.id, device_id, &request.name)
        .await?
        .ok_or_else(|| ApiError::not_found("Connect device not found"))?;
    state
        .nats
        .publish(
            live::config_subject(access.switcher.id),
            serde_json::to_vec(&live::ConfigChange {
                renamed_device_id: Some(device.id),
            })
            .expect("serializing a config change cannot fail")
            .into(),
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "Couldn't announce the Connect device rename");
            ApiError::internal()
        })?;
    Ok(Json(device.into()))
}

/// Revoke a device token and close the current obs-link session.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/devices/{device_id}", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("device_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Device revoked"), (status = NOT_FOUND, body = ErrorBody), (status = FORBIDDEN, body = ErrorBody)))]
async fn revoke_device(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path((_workspace_id, _switcher_id, device_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    access.workspace.members_only()?;
    let device = db::connect::list_for_switcher(&state.db, access.switcher.id)
        .await?
        .into_iter()
        .find(|device| device.id == device_id)
        .ok_or_else(|| ApiError::not_found("Connect device not found"))?;
    if !db::connect::revoke(&state.db, access.switcher.id, device_id).await? {
        return Err(ApiError::not_found("Connect device not found"));
    }
    state
        .nats
        .publish(
            live::reconnect_subject(access.switcher.id),
            serde_json::to_vec(&live::Reconnect {
                device_id: Some(device_id),
            })
            .expect("serializing a reconnect request cannot fail")
            .into(),
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "Couldn't request a Connect device reconnect");
            ApiError::internal()
        })?;
    let by = access.workspace.display_name(&state).await?;
    live::record_event(
        &state.db,
        &state.nats,
        access.switcher.id,
        EventDetail::ConnectDeviceRevoked {
            name: device.name,
            by: Some(by),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConnectPermissions {
    pub obs_control: ConnectAccess,
    pub logs: ConnectAccess,
}

/// Choose which switcher roles may control OBS or view Connect logs.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/permissions", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = ConnectPermissions,
    responses((status = OK, body = SwitcherDto), (status = FORBIDDEN, description = "`owner_only`", body = ErrorBody)))]
async fn set_permissions(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Json(request): Json<ConnectPermissions>,
) -> ApiResult<Json<SwitcherDto>> {
    let access = authorized.into_access();
    if !access.workspace.is_owner(&state).await? {
        return Err(ApiError::forbidden(
            "owner_only",
            "Only the workspace's owner can change Connect permissions",
        ));
    }
    if access.switcher.connect_obs_control == request.obs_control
        && access.switcher.connect_logs == request.logs
    {
        return Ok(Json(access.switcher.into()));
    }
    let switcher = db::switchers::set_connect_permissions(
        &state.db,
        access.switcher.id,
        request.obs_control,
        request.logs,
    )
    .await?;
    let by = access.workspace.display_name(&state).await?;
    live::record_event(
        &state.db,
        &state.nats,
        switcher.id,
        EventDetail::ConnectPermissionsChanged {
            obs_control: request.obs_control,
            logs: request.logs,
            by: Some(by),
        },
    )
    .await?;
    Ok(Json(switcher.into()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetWatchdog {
    pub mode: Watchdog,
}

/// Choose the watchdog behavior for Connect.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/watchdog", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetWatchdog,
    responses((status = OK, body = SwitcherDto), (status = FORBIDDEN, description = "`connect_forbidden`", body = ErrorBody)))]
async fn set_watchdog(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Json(request): Json<SetWatchdog>,
) -> ApiResult<Json<SwitcherDto>> {
    let access = authorized.into_access();
    let viewer = screenshots::viewer_for(&state, &access.workspace).await?;
    require_connect_obs_control(access.switcher.connect_obs_control, viewer)?;
    if access.switcher.connect_watchdog == request.mode {
        return Ok(Json(access.switcher.into()));
    }
    let switcher =
        db::switchers::set_connect_watchdog(&state.db, access.switcher.id, request.mode).await?;
    state
        .nats
        .publish(live::config_subject(switcher.id), "".into())
        .await
        .map_err(|error| {
            tracing::error!(%error, "Couldn't announce the Connect watchdog change");
            ApiError::internal()
        })?;
    let by = access.workspace.display_name(&state).await?;
    live::record_event(
        &state.db,
        &state.nats,
        switcher.id,
        EventDetail::ConnectWatchdogChanged {
            mode: request.mode,
            by: Some(by),
        },
    )
    .await?;
    Ok(Json(switcher.into()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConnectObsCommand {
    pub action: ConnectObsAction,
}

#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/obs", tag = "connect",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = ConnectObsCommand,
    responses(
        (status = NO_CONTENT),
        (status = FORBIDDEN, description = "`connect_forbidden`", body = ErrorBody),
        (status = CONFLICT, description = "`connect_offline`, `connect_unsupported`, or `connect_refused`", body = ErrorBody),
        (status = GATEWAY_TIMEOUT, description = "`connect_timeout`", body = ErrorBody),
    ))]
async fn connect_obs_command(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Json(request): Json<ConnectObsCommand>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let viewer = screenshots::viewer_for(&state, &access.workspace).await?;
    if request.action == ConnectObsAction::UploadLog {
        require_connect_logs(access.switcher.connect_logs, viewer)?;
    } else {
        require_connect_obs_control(access.switcher.connect_obs_control, viewer)?;
    }

    let by = access.workspace.display_name(&state).await?;
    let payload = serde_json::to_vec(&live::ConnectObsCommandRequest {
        action: request.action,
    })
    .map_err(|_| ApiError::internal())?;
    let response = telemetry::nats::request_with_timeout(
        &state.nats,
        live::connect_request_subject(access.switcher.id),
        payload,
        std::time::Duration::from_secs(35),
    )
    .await
    .map_err(|error| connect_request_error(error.kind()))?;
    let reply: live::ConnectObsCommandReply =
        serde_json::from_slice(&response.payload).map_err(|_| ApiError::internal())?;
    if !reply.ok {
        return Err(connect_command_failure(reply));
    }

    live::record_event(
        &state.db,
        &state.nats,
        access.switcher.id,
        EventDetail::ConnectObsCommand {
            action: request.action,
            by: Some(by),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn connect_offline() -> ApiError {
    ApiError::conflict(
        "connect_offline",
        "No Connect computer is online for this switcher",
    )
}

fn require_connect_obs_control(level: ConnectAccess, viewer: screenshots::Viewer) -> ApiResult<()> {
    if screenshots::connect_allowed(level, viewer) {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "connect_forbidden",
            "Your role cannot control OBS through Connect",
        ))
    }
}

fn require_connect_logs(level: ConnectAccess, viewer: screenshots::Viewer) -> ApiResult<()> {
    if screenshots::connect_allowed(level, viewer) {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "connect_forbidden_logs",
            "Your role cannot view Connect logs",
        ))
    }
}

fn connect_request_error(kind: async_nats::RequestErrorKind) -> ApiError {
    match kind {
        async_nats::RequestErrorKind::NoResponders => connect_offline(),
        async_nats::RequestErrorKind::TimedOut => connect_timeout(),
        _ => ApiError::internal(),
    }
}

fn connect_timeout() -> ApiError {
    ApiError::gateway_timeout("connect_timeout", "Connect did not respond in time")
}

fn connect_command_failure(reply: live::ConnectObsCommandReply) -> ApiError {
    match reply.code {
        Some(live::ConnectObsCommandCode::Offline) => connect_offline(),
        Some(live::ConnectObsCommandCode::Unsupported) => ApiError::conflict(
            "connect_unsupported",
            reply.error.unwrap_or_else(|| {
                "Update NOALBS Cloud Connect to control OBS from the dashboard".into()
            }),
        ),
        Some(live::ConnectObsCommandCode::Refused) => ApiError::conflict(
            "connect_refused",
            reply
                .error
                .unwrap_or_else(|| "Connect couldn't run the command".into()),
        ),
        Some(live::ConnectObsCommandCode::Timeout) => connect_timeout(),
        None => ApiError::internal(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectOs, connect_command_failure, connect_request_error, require_connect_logs,
        require_connect_obs_control,
    };
    use crate::screenshots::Viewer;
    use db::connect::ConnectAccess;

    #[test]
    fn operating_system_values_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&ConnectOs::Macos).unwrap(),
            "\"macos\""
        );
    }

    #[test]
    fn member_is_forbidden_when_only_owners_may_control_connect() {
        let error =
            require_connect_obs_control(ConnectAccess::Owners, Viewer::Operator).unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(error.code(), "connect_forbidden");
    }

    #[test]
    fn member_is_forbidden_when_only_owners_may_view_connect_logs() {
        let error = require_connect_logs(ConnectAccess::Owners, Viewer::Operator).unwrap_err();
        assert_eq!(error.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(error.code(), "connect_forbidden_logs");
        assert_eq!(error.message(), "Your role cannot view Connect logs");
    }

    #[test]
    fn no_responders_maps_to_offline_conflict() {
        let error = connect_request_error(async_nats::RequestErrorKind::NoResponders);
        assert_eq!(error.status(), axum::http::StatusCode::CONFLICT);
        assert_eq!(error.code(), "connect_offline");
    }

    #[test]
    fn explicit_offline_reply_maps_to_offline_conflict() {
        let error = connect_command_failure(live::ConnectObsCommandReply {
            ok: false,
            code: Some(live::ConnectObsCommandCode::Offline),
            error: Some("offline".into()),
            log_id: None,
        });
        assert_eq!(error.status(), axum::http::StatusCode::CONFLICT);
        assert_eq!(error.code(), "connect_offline");
    }
}
