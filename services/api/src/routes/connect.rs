use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{Duration, Utc};
use db::connect::{ConnectAccess, Watchdog};
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
        db::connect::Claim::Claimed(device) => Ok(Json(PairingClaimed {
            device_id: device.id,
            device_token,
            switcher_id: device.switcher_id,
            link_url: state.links.link_url(),
        })),
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
    let pairing = db::connect::pairing_by_user_code(&state.db, &request.user_code)
        .await?
        .ok_or_else(|| ApiError::not_found("Pairing code not found or already confirmed"))?;
    let confirmed = db::connect::confirm_pairing(
        &state.db,
        &request.user_code,
        access.switcher.id,
        access.workspace.user.id(),
    )
    .await?;
    if !confirmed {
        return Err(ApiError::not_found(
            "Pairing code not found or already confirmed",
        ));
    }
    let by = access.workspace.display_name(&state).await?;
    live::record_event(
        &state.db,
        &state.nats,
        access.switcher.id,
        EventDetail::ConnectDevicePaired {
            name: pairing.name,
            by: Some(by),
        },
    )
    .await?;
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
    if !screenshots::connect_allowed(access.switcher.connect_obs_control, viewer) {
        return Err(ApiError::forbidden(
            "connect_forbidden",
            "Your role cannot control OBS through Connect",
        ));
    }
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

#[cfg(test)]
mod tests {
    use super::ConnectOs;

    #[test]
    fn operating_system_values_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&ConnectOs::Macos).unwrap(),
            "\"macos\""
        );
    }
}
