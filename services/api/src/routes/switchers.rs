use std::collections::BTreeMap;

use axum::{Json, extract::State, http::StatusCode};
use db::switchers::{Behaviour, Scenes, SecretChange, StreamLink, Switcher, Triggers};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{SwitcherAuthorized, SwitcherManage, SwitcherView, WorkspaceAuthorized},
    routes::{ApiError, ApiResult, ErrorBody, chat::ChatSettingsDto},
    validate,
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_switchers, create_switcher))
        .routes(routes!(get_switcher, rename_switcher, delete_switcher))
        .routes(routes!(rotate_connect_token))
        .routes(routes!(set_triggers))
        .routes(routes!(set_behaviour))
        .routes(routes!(set_scenes))
        .routes(routes!(set_obs))
        .routes(routes!(set_belabox))
        .routes(routes!(set_belabox_interface_labels))
        .routes(routes!(set_moblin))
        .routes(routes!(moblin_connection))
}

const MAX_RETRY_ATTEMPTS: i32 = 100;
/// A day, in minutes.
const MAX_TIMEOUT_MINUTES: i32 = 1440;
const MAX_PASSWORD_LEN: usize = 256;
const MAX_REMOTE_KEY_LEN: usize = 128;
/// A BELABOX has a handful of interfaces; names of ones that are gone are kept too.
const MAX_INTERFACE_LABELS: usize = 24;
const MAX_INTERFACE_LABEL_LEN: usize = 30;
const MAX_INTERFACE_KEY_LEN: usize = 64;
/// Moblin generates about 20 characters; shorter ones are too easy to guess.
const MOBLIN_PASSWORD_LEN: std::ops::RangeInclusive<usize> = 8..=128;

/// A switcher as listed in its workspace.
#[derive(Debug, Serialize, ToSchema)]
pub struct SwitcherSummaryDto {
    pub id: Uuid,
    pub name: String,
    /// When a browser source last connected; `null` if it never has.
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub stream_server_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Over the plan's switcher limit after a downgrade: it doesn't switch and
    /// can't be controlled until the workspace upgrades or deletes another switcher.
    pub paused: bool,
    /// Live status; `null` while no browser source is connected.
    pub status: Option<live::SwitcherStatus>,
}

impl SwitcherSummaryDto {
    fn new(switcher: db::switchers::SwitcherSummary, status: Option<live::SwitcherStatus>) -> Self {
        Self {
            id: switcher.id,
            name: switcher.name,
            last_seen_at: switcher.last_seen_at,
            stream_server_count: switcher.stream_server_count,
            created_at: switcher.created_at,
            paused: switcher.paused,
            status,
        }
    }
}

/// Bitrate (kbps) and RTT (ms) thresholds. `null` turns a trigger off.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct TriggersDto {
    /// Switch to the LOW scene at or below this bitrate.
    pub low: Option<i32>,
    /// Switch to the offline (BRB) scene at or below this bitrate.
    pub offline: Option<i32>,
    /// Switch to the LOW scene at or above this round-trip time.
    pub rtt: Option<i32>,
    /// Switch to the offline scene at or above this round-trip time.
    pub rtt_offline: Option<i32>,
}

impl From<TriggersDto> for Triggers {
    fn from(triggers: TriggersDto) -> Self {
        Self {
            low: triggers.low,
            offline: triggers.offline,
            rtt: triggers.rtt,
            rtt_offline: triggers.rtt_offline,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BehaviourDto {
    /// Switch scenes automatically using the triggers.
    pub bitrate_switcher_enabled: bool,
    /// Leave OBS alone while it isn't streaming.
    pub only_switch_when_streaming: bool,
    /// Go back to LIVE as soon as the feed recovers.
    pub instantly_switch_on_recover: bool,
    /// Post in chat when a scene switches.
    pub auto_switch_notification: bool,
    /// How many bad checks in a row before switching away from LIVE.
    pub retry_attempts: i32,
    pub auto_stop_on_raid: bool,
    pub announce_raid: bool,
    pub record_while_streaming: bool,
    /// Keep recording on these OBS scenes; pause it on every other scene.
    /// An empty list disables scene-based recording control.
    pub recording_scene_ids: Vec<Uuid>,
    pub switch_to_starting: bool,
    pub switch_to_live: bool,
    /// Stop the stream after this many minutes on the BRB scene with no feed
    /// (1–1440); `null` = never.
    pub offline_timeout: Option<i32>,
    /// Stop the stream after this many minutes on the Ending scene (1–1440);
    /// `null` = never.
    pub ending_timeout: Option<i32>,
}

impl From<BehaviourDto> for Behaviour {
    fn from(b: BehaviourDto) -> Self {
        Self {
            bitrate_switcher_enabled: b.bitrate_switcher_enabled,
            only_switch_when_streaming: b.only_switch_when_streaming,
            instantly_switch_on_recover: b.instantly_switch_on_recover,
            auto_switch_notification: b.auto_switch_notification,
            retry_attempts: b.retry_attempts,
            auto_stop_on_raid: b.auto_stop_on_raid,
            announce_raid: b.announce_raid,
            record_while_streaming: b.record_while_streaming,
            recording_scene_ids: b.recording_scene_ids,
            switch_to_starting: b.switch_to_starting,
            switch_to_live: b.switch_to_live,
            offline_timeout: b.offline_timeout,
            ending_timeout: b.ending_timeout,
        }
    }
}

/// OBS scene UUIDs for each role. `null` means not set.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct ScenesDto {
    pub live: Option<Uuid>,
    pub low: Option<Uuid>,
    pub offline: Option<Uuid>,
    pub starting: Option<Uuid>,
    pub ending: Option<Uuid>,
    pub privacy: Option<Uuid>,
    pub refresh: Option<Uuid>,
}

impl From<ScenesDto> for Scenes {
    fn from(s: ScenesDto) -> Self {
        Self {
            live: s.live,
            low: s.low,
            offline: s.offline,
            starting: s.starting,
            ending: s.ending,
            privacy: s.privacy,
            refresh: s.refresh,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ObsDto {
    /// The obs-websocket port on the streamer's machine.
    pub port: i32,
    /// Whether an obs-websocket password is stored (it is never returned).
    pub has_password: bool,
}

/// A switcher and all its settings. Secrets are never included.
#[derive(Debug, Serialize, ToSchema)]
pub struct SwitcherDto {
    pub id: Uuid,
    pub name: String,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub triggers: TriggersDto,
    pub behaviour: BehaviourDto,
    pub scenes: ScenesDto,
    pub obs: ObsDto,
    pub chat: ChatSettingsDto,
    pub belabox: BelaboxDto,
    pub moblin: MoblinDto,
}

/// BELABOX Cloud. Optional: without a remote key nothing BELABOX-related runs.
#[derive(Debug, Serialize, ToSchema)]
pub struct BelaboxDto {
    /// Whether a remote key is stored (it is never returned).
    pub has_remote_key: bool,
    /// BELABOX starts when the OBS stream starts.
    pub start_with_stream: bool,
    /// BELABOX stops when the OBS stream stops.
    pub stop_with_stream: bool,
    /// Names for the BELABOX's interfaces, by the `key` the live status gives
    /// each one (so a name follows its device when `usb0` becomes `usb1`).
    pub interface_labels: BTreeMap<String, String>,
}

/// Moblin's remote control. Optional: without a password no Moblin can connect.
#[derive(Debug, Serialize, ToSchema)]
pub struct MoblinDto {
    /// Whether a password is set (read it with `…/moblin/connection`).
    pub has_password: bool,
    /// Moblin goes live when the OBS stream starts.
    pub start_with_stream: bool,
    /// Moblin stops when the OBS stream stops.
    pub stop_with_stream: bool,
}

/// What to enter in Moblin: Settings → Remote Control → Streamer.
#[derive(Debug, Serialize, ToSchema)]
pub struct MoblinSetup {
    /// The Assistant URL.
    pub url: String,
    /// The password, if one is set.
    pub password: Option<String>,
}

impl From<Switcher> for SwitcherDto {
    fn from(s: Switcher) -> Self {
        let interface_labels = s.interface_labels();
        Self {
            id: s.id,
            name: s.name,
            last_seen_at: s.last_seen_at,
            created_at: s.created_at,
            triggers: TriggersDto {
                low: s.trigger_low,
                offline: s.trigger_offline,
                rtt: s.trigger_rtt,
                rtt_offline: s.trigger_rtt_offline,
            },
            behaviour: BehaviourDto {
                bitrate_switcher_enabled: s.bitrate_switcher_enabled,
                only_switch_when_streaming: s.only_switch_when_streaming,
                instantly_switch_on_recover: s.instantly_switch_on_recover,
                auto_switch_notification: s.auto_switch_notification,
                retry_attempts: s.retry_attempts,
                auto_stop_on_raid: s.auto_stop_on_raid,
                announce_raid: s.announce_raid,
                record_while_streaming: s.record_while_streaming,
                recording_scene_ids: s.recording_scene_ids,
                switch_to_starting: s.switch_to_starting,
                switch_to_live: s.switch_to_live,
                offline_timeout: s.offline_timeout,
                ending_timeout: s.ending_timeout,
            },
            scenes: ScenesDto {
                live: s.scene_live,
                low: s.scene_low,
                offline: s.scene_offline,
                starting: s.scene_starting,
                ending: s.scene_ending,
                privacy: s.scene_privacy,
                refresh: s.scene_refresh,
            },
            obs: ObsDto {
                port: s.port,
                has_password: s.password.is_some(),
            },
            chat: ChatSettingsDto {
                command_prefix: s.command_prefix,
                language: s.language,
                public_commands_enabled: s.public_commands_enabled,
                mod_commands_enabled: s.mod_commands_enabled,
                threaded_replies: s.threaded_replies,
            },
            belabox: BelaboxDto {
                has_remote_key: s.belabox_remote_key.is_some(),
                start_with_stream: s.belabox_start_with_stream,
                stop_with_stream: s.belabox_stop_with_stream,
                interface_labels,
            },
            moblin: MoblinDto {
                has_password: s.moblin_password.is_some(),
                start_with_stream: s.moblin_start_with_stream,
                stop_with_stream: s.moblin_stop_with_stream,
            },
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSwitcher {
    pub name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RenameSwitcher {
    pub name: String,
}

/// The browser-source URL for OBS. It contains the connect token, so it is
/// only shown when the switcher is created or its token is replaced.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConnectLink {
    pub browser_source_url: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedSwitcher {
    pub switcher: SwitcherDto,
    pub browser_source_url: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetObs {
    pub port: i32,
    /// A new password. Leave out to keep the stored one.
    pub password: Option<String>,
    /// Remove the stored password.
    #[serde(default)]
    pub clear_password: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetBelabox {
    /// The BELABOX remote secret URL from BELABOX Cloud, or just its key
    /// (`https://remote.belabox.net/?key=…`). Leave out to keep the stored one.
    pub remote_key: Option<String>,
    /// Remove the stored key: BELABOX Cloud isn't used anymore.
    #[serde(default)]
    pub clear_remote_key: bool,
    /// Start BELABOX when the OBS stream starts.
    pub start_with_stream: bool,
    /// Stop BELABOX when the OBS stream stops.
    pub stop_with_stream: bool,
}

/// All the names for the BELABOX's interfaces; what is left out is removed.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetBelaboxInterfaceLabels {
    /// Name by interface key (`BelaboxInterface.key`). An empty name removes it.
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetMoblin {
    /// A password of your own (8–128 characters), e.g. the one Moblin already
    /// uses. Leave out to keep the stored one.
    pub password: Option<String>,
    /// Generate a new random password (wins over `password`).
    #[serde(default)]
    pub generate_password: bool,
    /// Remove the password: Moblin isn't used anymore.
    #[serde(default)]
    pub clear_password: bool,
    /// Moblin goes live when the OBS stream starts.
    pub start_with_stream: bool,
    /// Moblin stops when the OBS stream stops.
    pub stop_with_stream: bool,
}

/// A Moblin password someone typed: trimmed, 8–128 characters.
fn moblin_password_from(input: &str) -> Result<String, ApiError> {
    let password = input.trim();
    if !MOBLIN_PASSWORD_LEN.contains(&password.chars().count())
        || password.chars().any(char::is_control)
    {
        return Err(ApiError::bad_request(
            "invalid_password",
            "The password must be 8 to 128 characters",
        ));
    }
    Ok(password.to_owned())
}

fn switcher_name_conflict(error: db::Error) -> ApiError {
    if error.constraint() == Some("switchers_workspace_id_name_key") {
        ApiError::conflict(
            "name_taken",
            "A switcher with this name already exists here",
        )
    } else {
        error.into()
    }
}

/// Checks that each trigger is off or in range, and that the offline triggers
/// are stricter than the LOW ones (otherwise LOW would never be used).
pub fn validate_triggers(triggers: &TriggersDto) -> ApiResult<()> {
    control::check_triggers(&db::switchers::Triggers {
        low: triggers.low,
        offline: triggers.offline,
        rtt: triggers.rtt,
        rtt_offline: triggers.rtt_offline,
    })
    .map_err(|message| ApiError::bad_request("invalid_triggers", message))
}

/// List a workspace's switchers.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers", tag = "switchers",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = Vec<SwitcherSummaryDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn list_switchers(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<SwitcherView>,
) -> ApiResult<Json<Vec<SwitcherSummaryDto>>> {
    let access = authorized.into_access();
    let switchers = db::switchers::list_summaries(&state.db, access.workspace_id).await?;

    let mut list = Vec::with_capacity(switchers.len());
    for switcher in switchers {
        let status = state.status_of(switcher.id).await;
        list.push(SwitcherSummaryDto::new(switcher, status));
    }

    Ok(Json(list))
}

/// Create a switcher. Returns its browser-source URL, which is shown only once.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers", tag = "switchers",
    params(("workspace_id" = Uuid, Path)),
    request_body = CreateSwitcher,
    responses(
        (status = CREATED, body = CreatedSwitcher),
        (status = FORBIDDEN, description = "`plan_limit` when the workspace is full", body = ErrorBody),
        (status = CONFLICT, body = ErrorBody),
    ))]
async fn create_switcher(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<SwitcherManage>,
    Json(request): Json<CreateSwitcher>,
) -> ApiResult<(StatusCode, Json<CreatedSwitcher>)> {
    let access = authorized.into_access();
    let name = validate::name(&request.name)?;
    let token = common::tokens::generate();

    let switcher = db::switchers::create_within_plan(
        &state.db,
        access.workspace_id,
        name,
        &common::tokens::hash(&token),
    )
    .await
    .map_err(switcher_name_conflict)?;
    let Some(switcher) = switcher else {
        // A client workspace has no plan to upgrade, and its limit isn't a number to quote.
        let managed_by =
            db::workspaces::get_summary(&state.db, access.workspace_id, access.user.id())
                .await?
                .and_then(|workspace| workspace.organization_name);
        return Err(ApiError::forbidden(
            "plan_limit",
            match managed_by {
                Some(organization) => format!(
                    "This workspace has reached its switcher limit. Ask {organization} for more."
                ),
                None => {
                    "This workspace has reached its switcher limit. Upgrade the plan to add more."
                        .to_owned()
                }
            },
        ));
    };

    Ok((
        StatusCode::CREATED,
        Json(CreatedSwitcher {
            switcher: switcher.into(),
            browser_source_url: state.links.url(&token),
        }),
    ))
}

/// Get a switcher and its settings.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = OK, body = SwitcherDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn get_switcher(authorized: SwitcherAuthorized<SwitcherView>) -> Json<SwitcherDto> {
    Json(authorized.into_access().switcher.into())
}

/// Rename a switcher.
#[utoipa::path(patch, path = "/workspaces/{workspace_id}/switchers/{switcher_id}", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = RenameSwitcher,
    responses((status = OK, body = SwitcherDto), (status = CONFLICT, body = ErrorBody)))]
async fn rename_switcher(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<RenameSwitcher>,
) -> ApiResult<Json<SwitcherDto>> {
    let access = authorized.into_access();
    let name = validate::name(&request.name)?;
    let switcher = db::switchers::rename(&state.db, access.switcher.id, name)
        .await
        .map_err(switcher_name_conflict)?;

    Ok(Json(switcher.into()))
}

/// Delete a switcher and its stream servers.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Deleted"), (status = FORBIDDEN, body = ErrorBody)))]
async fn delete_switcher(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let switcher_id = access.switcher.id;
    db::switchers::delete(&state.db, switcher_id).await?;
    // Disconnects its browser source.
    state.notify_config(switcher_id).await;
    // A switcher paused for being over the plan's limit may now fit.
    state.notify_workspace(access.workspace.workspace_id).await;

    Ok(StatusCode::NO_CONTENT)
}

/// Replace the connect token. The old browser-source URL stops working. Not for
/// NOALBS support: the link gets the OBS password from obs-link.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/connect-token", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = OK, body = ConnectLink), (status = FORBIDDEN, description = "`support_secret` for NOALBS support", body = ErrorBody)))]
async fn rotate_connect_token(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
) -> ApiResult<Json<ConnectLink>> {
    let access = authorized.into_access();
    access.workspace.secrets_allowed()?;
    let token = common::tokens::generate();
    let switcher_id = access.switcher.id;
    db::switchers::set_connect_token_hash(&state.db, switcher_id, &common::tokens::hash(&token))
        .await?;
    // Disconnects the browser source using the old link.
    state.notify_config(switcher_id).await;

    Ok(Json(ConnectLink {
        browser_source_url: state.links.url(&token),
    }))
}

/// Replace the switcher's triggers.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/triggers", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = TriggersDto,
    responses((status = OK, body = SwitcherDto), (status = BAD_REQUEST, body = ErrorBody)))]
async fn set_triggers(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<TriggersDto>,
) -> ApiResult<Json<SwitcherDto>> {
    validate_triggers(&request)?;
    let switcher = db::switchers::set_triggers(
        &state.db,
        authorized.into_access().switcher.id,
        request.into(),
    )
    .await?;

    state.notify_config(switcher.id).await;

    Ok(Json(switcher.into()))
}

/// Replace the switcher's behaviour settings.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/behaviour", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = BehaviourDto,
    responses((status = OK, body = SwitcherDto), (status = BAD_REQUEST, body = ErrorBody)))]
async fn set_behaviour(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<BehaviourDto>,
) -> ApiResult<Json<SwitcherDto>> {
    if has_duplicate_scene_ids(&request.recording_scene_ids) {
        return Err(ApiError::bad_request(
            "invalid_behaviour",
            "Recording scenes must not contain duplicates",
        ));
    }
    if !(0..=MAX_RETRY_ATTEMPTS).contains(&request.retry_attempts) {
        return Err(ApiError::bad_request(
            "invalid_behaviour",
            "Retry attempts must be between 0 and 100",
        ));
    }
    let out_of_range = |timeout: Option<i32>| {
        timeout.is_some_and(|minutes| !(1..=MAX_TIMEOUT_MINUTES).contains(&minutes))
    };
    if out_of_range(request.offline_timeout) {
        return Err(ApiError::bad_request(
            "invalid_behaviour",
            "The offline timeout must be between 1 and 1440 minutes",
        ));
    }
    if out_of_range(request.ending_timeout) {
        return Err(ApiError::bad_request(
            "invalid_behaviour",
            "The ending timeout must be between 1 and 1440 minutes",
        ));
    }
    let switcher = db::switchers::set_behaviour(
        &state.db,
        authorized.into_access().switcher.id,
        request.into(),
    )
    .await?;

    state.notify_config(switcher.id).await;
    // The raid settings decide which raids the chat service subscribes to.
    state.notify_chat().await;

    Ok(Json(switcher.into()))
}

fn has_duplicate_scene_ids(ids: &[Uuid]) -> bool {
    let mut unique = ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    unique.len() != ids.len()
}

/// Replace which OBS scene is used for each role.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/scenes", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = ScenesDto,
    responses((status = OK, body = SwitcherDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn set_scenes(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<ScenesDto>,
) -> ApiResult<Json<SwitcherDto>> {
    let switcher = db::switchers::set_scenes(
        &state.db,
        authorized.into_access().switcher.id,
        request.into(),
    )
    .await?;

    state.notify_config(switcher.id).await;

    Ok(Json(switcher.into()))
}

/// Set how the browser source reaches obs-websocket on the streamer's machine.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetObs,
    responses((status = OK, body = SwitcherDto), (status = BAD_REQUEST, body = ErrorBody)))]
async fn set_obs(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<SetObs>,
) -> ApiResult<Json<SwitcherDto>> {
    if !(1..=65535).contains(&request.port) {
        return Err(ApiError::bad_request(
            "invalid_port",
            "The port must be between 1 and 65535",
        ));
    }

    let password = match (request.clear_password, request.password) {
        (true, _) => SecretChange::Clear,
        (false, None) => SecretChange::Keep,
        (false, Some(password)) if password.is_empty() => SecretChange::Clear,
        (false, Some(password)) if password.len() > MAX_PASSWORD_LEN => {
            return Err(ApiError::bad_request(
                "invalid_password",
                "The password is too long",
            ));
        }
        (false, Some(password)) => SecretChange::Set(state.data_key.encrypt(password.as_bytes())),
    };

    let switcher = db::switchers::set_obs(
        &state.db,
        authorized.into_access().switcher.id,
        request.port,
        password,
    )
    .await?;

    state.notify_config(switcher.id).await;

    Ok(Json(switcher.into()))
}

/// The BELABOX remote key from what was pasted: the key itself, or the link
/// BELABOX Cloud hands out (`https://remote.belabox.net/?key=…`). Trimmed,
/// 1–128 characters, no spaces inside.
fn remote_key_from(input: &str) -> Result<String, ApiError> {
    let input = input.trim();
    let key = match reqwest::Url::parse(input) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url
            .query_pairs()
            .find(|(name, _)| name == "key")
            .map(|(_, key)| key.trim().to_owned())
            .unwrap_or_default(),
        _ => input.to_owned(),
    };
    if key.is_empty()
        || key.len() > MAX_REMOTE_KEY_LEN
        || key.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(ApiError::bad_request(
            "invalid_remote_key",
            "That doesn't look like a BELABOX remote secret URL",
        ));
    }
    Ok(key)
}

/// Connect a BELABOX through BELABOX Cloud (or disconnect it), and whether it
/// starts and stops with the OBS stream.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetBelabox,
    responses((status = OK, body = SwitcherDto), (status = BAD_REQUEST, description = "`invalid_remote_key`", body = ErrorBody)))]
async fn set_belabox(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<SetBelabox>,
) -> ApiResult<Json<SwitcherDto>> {
    let remote_key = match (request.clear_remote_key, request.remote_key) {
        (true, _) => SecretChange::Clear,
        (false, None) => SecretChange::Keep,
        (false, Some(key)) => {
            SecretChange::Set(state.data_key.encrypt(remote_key_from(&key)?.as_bytes()))
        }
    };

    let switcher = db::switchers::set_belabox(
        &state.db,
        authorized.into_access().switcher.id,
        remote_key,
        StreamLink {
            start: request.start_with_stream,
            stop: request.stop_with_stream,
        },
    )
    .await?;

    state.notify_config(switcher.id).await;

    Ok(Json(switcher.into()))
}

/// Names for the BELABOX's network interfaces, saved by interface key. Replaces the
/// names saved before.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/interface-labels", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetBelaboxInterfaceLabels,
    responses((status = OK, body = SwitcherDto), (status = BAD_REQUEST, description = "`invalid_label`, `too_many_labels`", body = ErrorBody)))]
async fn set_belabox_interface_labels(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<SetBelaboxInterfaceLabels>,
) -> ApiResult<Json<SwitcherDto>> {
    let labels = interface_labels_from(request.labels)?;
    let switcher = db::switchers::set_belabox_interface_labels(
        &state.db,
        authorized.into_access().switcher.id,
        &labels,
    )
    .await?;

    Ok(Json(switcher.into()))
}

/// The interface names to store: trimmed, empty ones dropped, each 1–30
/// characters without control characters, at most 24.
fn interface_labels_from(
    labels: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, ApiError> {
    let mut checked = BTreeMap::new();
    for (key, label) in labels {
        let label = label.trim();
        if label.is_empty() {
            continue;
        }
        if key.is_empty()
            || key.len() > MAX_INTERFACE_KEY_LEN
            || key.chars().any(char::is_control)
            || label.chars().count() > MAX_INTERFACE_LABEL_LEN
            || label.chars().any(char::is_control)
        {
            return Err(ApiError::bad_request(
                "invalid_label",
                format!("A name can be 1–{MAX_INTERFACE_LABEL_LEN} characters"),
            ));
        }
        checked.insert(key, label.to_owned());
    }
    if checked.len() > MAX_INTERFACE_LABELS {
        return Err(ApiError::bad_request(
            "too_many_labels",
            format!("At most {MAX_INTERFACE_LABELS} interface names"),
        ));
    }
    Ok(checked)
}

/// Connect Moblin's remote control (or disconnect it), and whether Moblin starts
/// and stops with the OBS stream. A new or removed password disconnects the
/// Moblin connected now.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SetMoblin,
    responses((status = OK, body = SwitcherDto), (status = BAD_REQUEST, description = "`invalid_password`", body = ErrorBody)))]
async fn set_moblin(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<SetMoblin>,
) -> ApiResult<Json<SwitcherDto>> {
    let password = match (
        request.clear_password,
        request.generate_password,
        request.password,
    ) {
        (true, _, _) => SecretChange::Clear,
        (false, true, _) => SecretChange::Set(
            state
                .data_key
                .encrypt(common::tokens::password().as_bytes()),
        ),
        (false, false, None) => SecretChange::Keep,
        (false, false, Some(password)) => SecretChange::Set(
            state
                .data_key
                .encrypt(moblin_password_from(&password)?.as_bytes()),
        ),
    };

    let switcher = db::switchers::set_moblin(
        &state.db,
        authorized.into_access().switcher.id,
        password,
        StreamLink {
            start: request.start_with_stream,
            stop: request.stop_with_stream,
        },
    )
    .await?;

    state.notify_config(switcher.id).await;

    Ok(Json(switcher.into()))
}

/// The Assistant URL and password to enter in Moblin (the password is a
/// secret: only for members who manage the switcher, never NOALBS support).
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/connection", tag = "switchers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = OK, body = MoblinSetup), (status = FORBIDDEN, description = "`support_secret` for NOALBS support", body = ErrorBody)))]
async fn moblin_connection(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
) -> ApiResult<Json<MoblinSetup>> {
    let access = authorized.into_access();
    access.workspace.secrets_allowed()?;
    let switcher = access.switcher;
    let password = match &switcher.moblin_password {
        Some(encrypted) => {
            let bytes = state.data_key.decrypt(encrypted).map_err(|error| {
                tracing::error!(%error, switcher_id = %switcher.id, "Couldn't decrypt a Moblin password");
                ApiError::internal()
            })?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        None => None,
    };

    Ok(Json(MoblinSetup {
        url: state.links.moblin_url(switcher.id),
        password,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        TriggersDto, has_duplicate_scene_ids, interface_labels_from, moblin_password_from,
        remote_key_from, validate_triggers,
    };
    use uuid::Uuid;

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, label)| (key.to_string(), label.to_string()))
            .collect()
    }

    #[test]
    fn interface_labels_are_trimmed_and_empty_ones_dropped() {
        let checked =
            interface_labels_from(labels(&[("a1", " Phone A "), ("b2", "  "), ("c3", "")]))
                .unwrap();
        assert_eq!(checked, labels(&[("a1", "Phone A")]));
    }

    #[test]
    fn interface_labels_are_limited() {
        let too_long = "x".repeat(31);
        for bad in [
            labels(&[("a1", &too_long)]),
            labels(&[("a1", "line\nbreak")]),
            labels(&[("", "Phone")]),
        ] {
            assert!(interface_labels_from(bad).is_err());
        }
        let many: BTreeMap<String, String> = (0..25)
            .map(|i| (format!("k{i}"), "Phone".to_owned()))
            .collect();
        assert!(interface_labels_from(many).is_err());
        assert!(interface_labels_from(labels(&[("a1", &"é".repeat(30))])).is_ok());
    }

    #[test]
    fn moblin_passwords_are_trimmed_and_checked() {
        assert_eq!(
            moblin_password_from("  abcd1234  ").ok().as_deref(),
            Some("abcd1234")
        );
        for bad in ["", "short", "tab\there1", &"p".repeat(129)] {
            assert!(moblin_password_from(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn remote_keys_are_trimmed_and_checked() {
        assert_eq!(
            remote_key_from("  abcDEF123  ").ok().as_deref(),
            Some("abcDEF123")
        );
        for bad in ["", "   ", "two words", "tab\there", &"k".repeat(129)] {
            assert!(remote_key_from(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn remote_keys_come_out_of_belabox_cloud_links() {
        assert_eq!(
            remote_key_from(" https://remote.belabox.net/?key=0AwP58pd ")
                .ok()
                .as_deref(),
            Some("0AwP58pd")
        );
        assert_eq!(
            remote_key_from("https://remote.belabox.net/?lang=en&key=abc%2B1")
                .ok()
                .as_deref(),
            Some("abc+1")
        );
        for bad in [
            "https://remote.belabox.net/",
            "https://remote.belabox.net/?key=",
        ] {
            assert!(remote_key_from(bad).is_err(), "{bad:?}");
        }
    }

    fn triggers(
        low: Option<i32>,
        offline: Option<i32>,
        rtt: Option<i32>,
        rtt_offline: Option<i32>,
    ) -> TriggersDto {
        TriggersDto {
            low,
            offline,
            rtt,
            rtt_offline,
        }
    }

    #[test]
    fn accepts_defaults_and_off_triggers() {
        assert!(validate_triggers(&triggers(Some(800), None, Some(2500), None)).is_ok());
        assert!(validate_triggers(&triggers(None, None, None, None)).is_ok());
        assert!(validate_triggers(&triggers(Some(800), Some(150), Some(2500), Some(5000))).is_ok());
    }

    #[test]
    fn rejects_zero_out_of_range_and_inverted_triggers() {
        assert!(validate_triggers(&triggers(Some(0), None, None, None)).is_err());
        assert!(validate_triggers(&triggers(Some(200_000), None, None, None)).is_err());
        assert!(validate_triggers(&triggers(None, None, Some(-1), None)).is_err());
        assert!(validate_triggers(&triggers(Some(800), Some(800), None, None)).is_err());
        assert!(validate_triggers(&triggers(None, None, Some(2500), Some(2000))).is_err());
    }

    #[test]
    fn recording_scenes_must_be_unique() {
        let scene = Uuid::new_v4();
        assert!(!has_duplicate_scene_ids(&[scene, Uuid::new_v4()]));
        assert!(has_duplicate_scene_ids(&[scene, scene]));
    }
}
