//! Remote control of a switcher's BELABOX through BELABOX Cloud (obs-link keeps
//! the connection; see `control::Control::belabox`). Its live status comes with
//! the switcher's live feed (`belabox` events); the remote key is saved with
//! `PUT …/belabox` in `switchers.rs`.
//!
//! The settings dialog (WiFi, modems, audio and stream settings) is fetched from obs-link
//! on demand (`GET …/belabox/details`) instead of riding the live feed: it holds network
//! names, which say where the streamer is, so support access never gets it. Joining or
//! forgetting a network, the hotspot's and the modems' settings need `switcher.manage` (they
//! can cut the BELABOX off); the rest is `switcher.control`, like start and stop.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use live::{BelaboxAction, BelaboxDetails, BelaboxSettingsUpdate};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{SwitcherAccess, SwitcherAuthorized, SwitcherControl, SwitcherManage},
    routes::{
        ApiResult, ErrorBody,
        feed::{SetActive, control_error},
    },
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(set_belabox_streaming))
        .routes(routes!(set_belabox_bitrate))
        .routes(routes!(set_belabox_interface))
        .routes(routes!(get_belabox_details))
        .routes(routes!(set_belabox_settings))
        .routes(routes!(scan_belabox_wifi))
        .routes(routes!(connect_belabox_wifi))
        .routes(routes!(disconnect_belabox_wifi))
        .routes(routes!(add_belabox_wifi))
        .routes(routes!(forget_belabox_wifi))
        .routes(routes!(set_belabox_hotspot))
        .routes(routes!(set_belabox_hotspot_config))
        .routes(routes!(set_belabox_modem))
        .routes(routes!(scan_belabox_modem))
}

async fn send(
    state: &ApiState,
    access: SwitcherAccess,
    action: BelaboxAction,
) -> ApiResult<StatusCode> {
    let by = access.workspace.display_name(state).await?;
    state
        .control()
        .belabox(access.switcher.id, action, &by)
        .await
        .map_err(control_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Start (`active: true`) or stop the BELABOX stream. Starting uses the settings
/// last saved on the BELABOX.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/stream", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetActive,
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = NOT_FOUND, description = "`belabox_not_set_up`", body = ErrorBody),
        (status = CONFLICT, description = "`belabox_already_streaming`, `belabox_not_streaming`, `belabox_no_config`, `belabox_invalid_key`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn set_belabox_streaming(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetActive>,
) -> ApiResult<StatusCode> {
    let action = if request.active {
        BelaboxAction::Start
    } else {
        BelaboxAction::Stop
    };
    send(&state, authorized.into_access(), action).await
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetBelaboxBitrate {
    /// kbps, 300–12000.
    pub max_br: u32,
}

/// Set the BELABOX's max bitrate: at once while it streams, otherwise from its next
/// start from here, chat or the OBS stream.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/bitrate", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetBelaboxBitrate,
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = BAD_REQUEST, description = "`invalid_bitrate`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn set_belabox_bitrate(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<SetBelaboxBitrate>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::Bitrate {
            max_br: request.max_br,
        },
    )
    .await
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetBelaboxInterface {
    pub enabled: bool,
}

/// Turn one of the BELABOX's network interfaces (`usb0`, `wlan0`, …) on or off.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/interfaces/{name}", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("name" = String, Path)),
    request_body = SetBelaboxInterface,
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = NOT_FOUND, description = "`unknown_interface`", body = ErrorBody),
        (status = CONFLICT, description = "`last_interface`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn set_belabox_interface(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Path(path): Path<(Uuid, Uuid, String)>,
    Json(request): Json<SetBelaboxInterface>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::Interface {
            name: path.2,
            enabled: request.enabled,
        },
    )
    .await
}

/// What the BELABOX settings dialog shows: WiFi adapters with their networks and hotspot,
/// modems with their APN settings, audio inputs and codecs, pipelines, relays and the
/// stream settings. Passwords and the stream ID are never returned (`has_…`). The dialog
/// polls this while it's open: obs-link answers from what the BELABOX reported.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/details", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses(
        (status = OK, body = BelaboxDetails),
        (status = FORBIDDEN, description = "`support_secret`", body = ErrorBody),
        (status = NOT_FOUND, description = "`belabox_not_set_up`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn get_belabox_details(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
) -> ApiResult<Json<BelaboxDetails>> {
    let access = authorized.into_access();
    access.workspace.secrets_allowed()?;
    let details = state
        .control()
        .belabox_details(access.switcher.id)
        .await
        .map_err(control_error)?;

    Ok(Json(details))
}

/// Stream settings, used from the BELABOX's next start from here (the dashboard, chat or the
/// OBS stream): BELABOX only saves them when it starts, so nothing reaches it before that.
/// Refused while it streams.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/settings", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = BelaboxSettingsUpdate,
    responses(
        (status = NO_CONTENT, description = "Kept for the next start"),
        (status = BAD_REQUEST, description = "`invalid_pipeline`, `invalid_audio_source`, `invalid_audio_codec`, `invalid_audio_delay`, `invalid_srt_latency`, `invalid_relay`, `invalid_relay_account`, `invalid_address`, `invalid_port`, `invalid_stream_id`", body = ErrorBody),
        (status = CONFLICT, description = "`belabox_streaming`, `belabox_no_config`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn set_belabox_settings(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(settings): Json<BelaboxSettingsUpdate>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::Settings { settings },
    )
    .await
}

/// Search for WiFi networks again; the results show up in the details a few seconds later.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/wifi/scan", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn scan_belabox_wifi(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
) -> ApiResult<StatusCode> {
    send(&state, authorized.into_access(), BelaboxAction::WifiScan).await
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BelaboxWifiConnection {
    /// The saved connection's UUID, from the details.
    pub uuid: String,
}

/// Join a saved WiFi network. How it went shows in the details' `result` a few seconds later.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/wifi/connect", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = BelaboxWifiConnection,
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = NOT_FOUND, description = "`unknown_wifi`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn connect_belabox_wifi(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<BelaboxWifiConnection>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::WifiConnect { uuid: request.uuid },
    )
    .await
}

/// Leave the WiFi network the BELABOX is connected to (it stays saved).
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/wifi/disconnect", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = BelaboxWifiConnection,
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = NOT_FOUND, description = "`unknown_wifi`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn disconnect_belabox_wifi(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<BelaboxWifiConnection>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::WifiDisconnect { uuid: request.uuid },
    )
    .await
}

#[derive(Deserialize, ToSchema)]
pub struct AddBelaboxWifi {
    /// The adapter's ID, from the details.
    pub device: String,
    pub ssid: String,
    /// 8–63 characters; leave out for an open network.
    pub password: Option<String>,
}

/// Join and save a new WiFi network (`switcher.manage`). The password is write-only.
/// How it went (`auth` = wrong password) shows in the details' `result` within 15 seconds.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/wifi/networks", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = AddBelaboxWifi,
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = BAD_REQUEST, description = "`invalid_wifi_name`, `invalid_wifi_password`", body = ErrorBody),
        (status = NOT_FOUND, description = "`unknown_wifi`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn add_belabox_wifi(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<AddBelaboxWifi>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::WifiNew {
            device: request.device,
            ssid: request.ssid,
            password: request.password.filter(|password| !password.is_empty()),
        },
    )
    .await
}

/// Forget a saved WiFi network (`switcher.manage`).
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/wifi/networks/{uuid}", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("uuid" = String, Path)),
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = NOT_FOUND, description = "`unknown_wifi`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn forget_belabox_wifi(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, String)>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::WifiForget { uuid: path.2 },
    )
    .await
}

/// Turn a WiFi adapter into a hotspot (`active: true`) or back into a WiFi client.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/wifi/{device}/hotspot", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("device" = String, Path)),
    request_body = SetActive,
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = NOT_FOUND, description = "`unknown_wifi` (no such adapter, or it can't be a hotspot)", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn set_belabox_hotspot(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Path(path): Path<(Uuid, Uuid, String)>,
    Json(request): Json<SetActive>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::Hotspot {
            device: path.2,
            active: request.active,
        },
    )
    .await
}

#[derive(Deserialize, ToSchema)]
pub struct SetBelaboxHotspotConfig {
    /// 1–32 characters.
    pub name: String,
    /// 8–64 characters. Left out keeps the current one.
    pub password: Option<String>,
    /// One of the hotspot's `channels` (`auto`, `auto_24`, `auto_50`).
    pub channel: String,
}

/// Rename the hotspot, change its password or band (`switcher.manage`; it must be running).
/// The BELABOX restarts it; how that went shows in the details' `result`.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/wifi/{device}/hotspot/config", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("device" = String, Path)),
    request_body = SetBelaboxHotspotConfig,
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = BAD_REQUEST, description = "`invalid_hotspot_name`, `invalid_hotspot_password`, `invalid_hotspot_channel`", body = ErrorBody),
        (status = NOT_FOUND, description = "`unknown_wifi` (no such adapter, or it isn't a hotspot now)", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn set_belabox_hotspot_config(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, String)>,
    Json(request): Json<SetBelaboxHotspotConfig>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::HotspotConfig {
            device: path.2,
            name: request.name,
            password: request.password.filter(|password| !password.is_empty()),
            channel: request.channel,
        },
    )
    .await
}

#[derive(Deserialize, ToSchema)]
pub struct SetBelaboxModem {
    /// Let the BELABOX pick the APN (only on modems that can).
    pub autoconfig: bool,
    pub apn: String,
    pub username: String,
    /// Left out keeps the current one.
    pub password: Option<String>,
    pub roaming: bool,
    /// An operator ID from the last scan or the saved one; empty is automatic.
    pub network: String,
    /// One of the modem's `network_types`.
    pub network_type: String,
}

/// Change a modem's APN, roaming, network type and operator (`switcher.manage`). The BELABOX
/// takes the modem's connection down to apply them, so a stream over it stutters.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/modems/{modem_id}", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("modem_id" = String, Path)),
    request_body = SetBelaboxModem,
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = BAD_REQUEST, description = "`invalid_apn`, `invalid_network_type`, `invalid_network`", body = ErrorBody),
        (status = NOT_FOUND, description = "`unknown_modem`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn set_belabox_modem(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, String)>,
    Json(request): Json<SetBelaboxModem>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::ModemConfig {
            modem: path.2,
            autoconfig: request.autoconfig,
            apn: request.apn,
            username: request.username,
            password: request.password.filter(|password| !password.is_empty()),
            roaming: request.roaming,
            network: request.network,
            network_type: request.network_type,
        },
    )
    .await
}

/// Search for mobile operators. The modem goes offline while it scans (about a minute), so
/// do it before a stream. The results show up in the details.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/modems/{modem_id}/scan", tag = "belabox",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("modem_id" = String, Path)),
    responses(
        (status = NO_CONTENT, description = "Sent to the BELABOX"),
        (status = NOT_FOUND, description = "`unknown_modem`", body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, description = "`belabox_unreachable`, `belabox_connecting`, `belabox_offline`", body = ErrorBody),
    ))]
async fn scan_belabox_modem(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Path(path): Path<(Uuid, Uuid, String)>,
) -> ApiResult<StatusCode> {
    send(
        &state,
        authorized.into_access(),
        BelaboxAction::ModemScan { modem: path.2 },
    )
    .await
}
