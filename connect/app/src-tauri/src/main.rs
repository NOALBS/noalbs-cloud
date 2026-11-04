#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use connect_core::{
    AppConfig, ConfigStore, HostPlatform, PairingClient, Setup,
    config::validate_obs_config_for_platform,
    credentials::TokenStore,
    logs::{ConnectLogUpload, ObsLogFile, encode_log_upload, select_obs_log_file},
    pairing::ConnectOs,
    relay::{
        LinkConfig, ObsAction, ObsCommandRequest, ObsTelemetry, PcObsState, WatchdogMode, run_link,
        stop_active_streams,
    },
    supervisor::{
        Supervisor, config_dir, detected_obs_binary_for, detected_obs_install_kind,
        read_obs_websocket_config, same_obs_binary, same_obs_identity_for_platform,
        setup_is_portable_for_platform,
    },
    url_security::{validate_dashboard_url, validate_verification_url},
    watchdog::{Watchdog, WatchdogAction},
};
use serde::Serialize;
use std::time::Duration;
use tauri::{
    Emitter, Manager, State,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
#[cfg(target_os = "macos")]
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{mpsc, watch};

struct LinkSession {
    state: watch::Sender<PcObsState>,
    effective_obs: watch::Receiver<Option<(u16, String)>>,
    task: tauri::async_runtime::JoinHandle<()>,
    monitor: tauri::async_runtime::JoinHandle<()>,
    command_consumer: tauri::async_runtime::JoinHandle<()>,
}

struct AppState {
    config: Mutex<AppConfig>,
    startup_error: Option<String>,
    store: ConfigStore,
    tokens: TokenStore,
    supervisor: Mutex<Supervisor>,
    link_sessions: Mutex<HashMap<uuid::Uuid, LinkSession>>,
    obs_actions_running: Mutex<HashSet<uuid::Uuid>>,
    pending_verification_urls: Mutex<HashMap<uuid::Uuid, String>>,
}

#[derive(Clone, Serialize)]
struct SetupSummary {
    id: String,
    name: String,
    paired: bool,
    switcher_id: Option<String>,
    obs_state: String,
    obs_process_running: bool,
    obs: connect_core::config::ObsLaunchConfig,
    detected_binary: String,
    config_dir: String,
}

#[derive(Clone, Serialize)]
struct UiState {
    platform: String,
    start_at_login: bool,
    api_url: String,
    dashboard_url: Option<String>,
    startup_error: Option<String>,
    first_run: bool,
    setups: Vec<SetupSummary>,
}

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> UiState {
    build_ui_state(&state)
}

fn build_ui_state(state: &AppState) -> UiState {
    let (setups, start_at_login, api_url, dashboard_url) = {
        let config = state.config.lock().expect("app config lock");
        let settings = config.settings.effective();
        (
            config.setups.clone(),
            config.settings.start_at_login,
            settings.api_url,
            settings.dashboard_url,
        )
    };
    let mut supervisor = state.supervisor.lock().expect("supervisor lock");
    supervisor.set_known_setups(&setups);
    let sessions = state.link_sessions.lock().expect("link session lock");
    UiState {
        platform: HostPlatform::current().as_str().to_owned(),
        start_at_login,
        api_url,
        dashboard_url,
        startup_error: state.startup_error.clone(),
        first_run: setups.is_empty(),
        setups: setups
            .iter()
            .map(|setup| {
                let link_state = sessions
                    .get(&setup.id)
                    .map(|session| *session.state.borrow());
                let process_state =
                    link_state.map_or_else(|| supervisor.state(setup), |_| PcObsState::Unknown);
                let obs_state =
                    serde_json::to_value(displayed_obs_state(process_state, link_state))
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .unwrap_or_else(|| "unknown".into());
                let obs_process_running = supervisor.owns_running_process(setup);
                SetupSummary {
                    id: setup.id.to_string(),
                    name: setup.name.clone(),
                    paired: setup.pairing.is_some(),
                    switcher_id: setup
                        .pairing
                        .as_ref()
                        .map(|pair| pair.switcher_id.to_string()),
                    obs_state,
                    obs_process_running,
                    obs: setup.obs.clone(),
                    detected_binary: setup
                        .obs
                        .binary_path
                        .clone()
                        .unwrap_or_else(|| detected_obs_binary_for(&setup.obs.install_kind))
                        .to_string_lossy()
                        .into_owned(),
                    config_dir: config_dir(setup).to_string_lossy().into_owned(),
                }
            })
            .collect(),
    }
}

#[tauri::command]
fn save_setup(
    app: tauri::AppHandle,
    setup_id: String,
    name: String,
    obs: connect_core::config::ObsLaunchConfig,
    state: State<'_, AppState>,
) -> Result<UiState, String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
    let index = config
        .setups
        .iter()
        .position(|setup| setup.id == id)
        .ok_or("setup not found")?;
    let mut updated_setup = config.setups[index].clone();
    updated_setup.name = if name.trim().is_empty() {
        "OBS".into()
    } else {
        name.trim().to_owned()
    };
    updated_setup.obs = obs;
    clear_unsupported_config_dir_override(&mut updated_setup.obs, HostPlatform::current());
    validate_setup_identity(&config.setups, &updated_setup)?;
    config.setups[index] = updated_setup.clone();
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    drop(config);
    if updated_setup.pairing.is_some() {
        start_link_session(&app, state.inner(), updated_setup)?;
    }
    Ok(get_state(state))
}

fn validate_setup_identity(
    setups: &[connect_core::config::Setup],
    candidate: &connect_core::config::Setup,
) -> Result<(), String> {
    validate_setup_identity_for_platform(setups, candidate, HostPlatform::current())
}

fn validate_setup_identity_for_platform(
    setups: &[connect_core::config::Setup],
    candidate: &connect_core::config::Setup,
    platform: HostPlatform,
) -> Result<(), String> {
    validate_obs_config_for_platform(&candidate.obs, platform).map_err(str::to_owned)?;
    for other in setups.iter().filter(|setup| setup.id != candidate.id) {
        if platform == HostPlatform::Windows && same_obs_binary(candidate, other) {
            let candidate_portable = setup_is_portable_for_platform(candidate, platform);
            let other_portable = setup_is_portable_for_platform(other, platform);
            if candidate_portable != other_portable {
                let error = if other_portable {
                    "This OBS is already used by another setup in portable mode; use a separate normal OBS install"
                } else {
                    "This OBS is already used by another setup in normal mode; use a separate portable copy of OBS"
                };
                return Err(error.into());
            }
        }
        if !same_obs_identity_for_platform(candidate, other, platform) {
            continue;
        }
        let can_share = candidate.obs.shared_config
            && other.obs.shared_config
            && candidate.obs.websocket_port.is_some()
            && other.obs.websocket_port.is_some()
            && candidate.obs.websocket_port != other.obs.websocket_port;
        if !can_share {
            return Err(
                "Another setup already uses this OBS binary and config directory. Shared-config setups must use distinct websocket ports."
                    .into(),
            );
        }
    }
    Ok(())
}

fn clear_unsupported_config_dir_override(
    obs: &mut connect_core::config::ObsLaunchConfig,
    platform: HostPlatform,
) -> bool {
    if obs.config_dir_override.is_some() && validate_obs_config_for_platform(obs, platform).is_err()
    {
        obs.config_dir_override = None;
        true
    } else {
        false
    }
}

fn remove_unsupported_config_dir_overrides(config: &mut AppConfig, platform: HostPlatform) -> bool {
    let mut removed = false;
    for setup in &mut config.setups {
        removed |= clear_unsupported_config_dir_override(&mut setup.obs, platform);
    }
    removed
}

#[tauri::command]
fn add_setup(name: String, state: State<'_, AppState>) -> Result<UiState, String> {
    let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
    let name = if name.trim().is_empty() {
        default_setup_name()
    } else {
        name
    };
    let mut setup = Setup::new(name);
    setup.obs.install_kind = detected_obs_install_kind();
    config.setups.push(setup);
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    drop(config);
    Ok(get_state(state))
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    start_at_login: bool,
    api_url: String,
    dashboard_url: Option<String>,
    state: State<'_, AppState>,
) -> Result<UiState, String> {
    let dashboard_url = dashboard_url.filter(|url| !url.trim().is_empty());
    let (api_url_override, dashboard_url_override) =
        connect_core::config::AppSettings::environment_overrides();
    let effective_dashboard_url = dashboard_url_override
        .as_deref()
        .or(dashboard_url.as_deref());
    if let Some(url) = effective_dashboard_url {
        validate_dashboard_url(url)?;
    }
    {
        let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
        config.settings.start_at_login = start_at_login;
        if api_url_override.is_none() {
            config.settings.api_url = api_url;
        }
        if dashboard_url_override.is_none() {
            config.settings.dashboard_url = dashboard_url;
        }
        state
            .store
            .save(&config)
            .map_err(|error| error.to_string())?;
    }
    let autostart = app.autolaunch();
    if start_at_login {
        autostart.enable().map_err(|error| error.to_string())?;
    } else {
        autostart.disable().map_err(|error| error.to_string())?;
    }
    Ok(get_state(state))
}

fn default_setup_name() -> String {
    let key = if cfg!(target_os = "windows") {
        "COMPUTERNAME"
    } else {
        "HOSTNAME"
    };
    std::env::var(key)
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "OBS".into())
}

#[tauri::command]
async fn create_pairing(
    setup_id: String,
    state: State<'_, AppState>,
) -> Result<connect_core::PairingCreated, String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    let (api_url, dashboard_url, name) = {
        let config = state.config.lock().map_err(|_| "config lock poisoned")?;
        let setup = config
            .setups
            .iter()
            .find(|setup| setup.id.to_string() == setup_id)
            .ok_or("setup not found")?;
        let settings = config.settings.effective();
        (settings.api_url, settings.dashboard_url, setup.name.clone())
    };
    let client = PairingClient::new_with_dashboard_url(api_url, dashboard_url)
        .map_err(|error| error.to_string())?;
    let pending = client
        .create(&name, ConnectOs::current(), env!("CARGO_PKG_VERSION"))
        .await
        .map_err(|error| error.to_string())?;
    state
        .pending_verification_urls
        .lock()
        .map_err(|_| "pending verification URL lock poisoned")?
        .insert(id, pending.verification_url.clone());
    Ok(pending)
}

#[tauri::command]
async fn pair_setup(
    app: tauri::AppHandle,
    setup_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    let (api_url, dashboard_url, name) = {
        let config = state.config.lock().map_err(|_| "config lock poisoned")?;
        let setup = config
            .setups
            .iter()
            .find(|setup| setup.id.to_string() == setup_id)
            .ok_or("setup not found")?;
        let settings = config.settings.effective();
        (settings.api_url, settings.dashboard_url, setup.name.clone())
    };
    let client = PairingClient::new_with_dashboard_url(api_url, dashboard_url)
        .map_err(|error| error.to_string())?;
    let pending = client
        .create(&name, ConnectOs::current(), env!("CARGO_PKG_VERSION"))
        .await
        .map_err(|error| error.to_string())?;
    state
        .pending_verification_urls
        .lock()
        .map_err(|_| "pending verification URL lock poisoned")?
        .insert(id, pending.verification_url.clone());
    let interval = pending.interval.max(1);
    app.emit("pairing-created", &pending)
        .map_err(|error| error.to_string())?;
    let result = client
        .poll_until_complete(
            &pending.device_code,
            Duration::from_secs(interval.into()),
            |status| {
                let _ = app.emit("pairing-status", format!("{status:?}"));
            },
        )
        .await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            if let Ok(mut urls) = state.pending_verification_urls.lock() {
                urls.remove(&id);
            }
            return Err(error.to_string());
        }
    };
    state
        .pending_verification_urls
        .lock()
        .map_err(|_| "pending verification URL lock poisoned")?
        .remove(&id);
    state
        .tokens
        .set(id, &result.device_token)
        .map_err(|error| error.to_string())?;
    let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
    let setup = config
        .setups
        .iter_mut()
        .find(|setup| setup.id == id)
        .ok_or("setup not found")?;
    setup.pairing = Some(result.pairing);
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    drop(config);
    let setup = find_setup(&state, &setup_id)?;
    if setup.obs.start_obs_with_connect {
        state
            .supervisor
            .lock()
            .map_err(|_| "supervisor lock poisoned")?
            .start(&setup, false, None)
            .map_err(|error| error.to_string())?;
    }
    if let Err(error) = start_link_session(&app, state.inner(), setup) {
        let _ = app.emit("connect-error", error);
    }
    app.emit("connect-state-changed", get_state(state))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_verification_url(
    app: tauri::AppHandle,
    setup_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    let verification_url = state
        .pending_verification_urls
        .lock()
        .map_err(|_| "pending verification URL lock poisoned")?
        .get(&id)
        .cloned()
        .ok_or("No active verification URL for this setup")?;
    let config = state.config.lock().map_err(|_| "config lock poisoned")?;
    let settings = config.settings.effective();
    validate_verification_url(
        &settings.api_url,
        settings.dashboard_url.as_deref(),
        &verification_url,
    )?;
    drop(config);
    app.opener()
        .open_url(verification_url, None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_obs(setup_id: String, state: State<'_, AppState>) -> Result<UiState, String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    run_obs_action(state.inner(), id, ObsAction::Start).await?;
    Ok(get_state(state))
}

#[tauri::command]
async fn stop_obs(setup_id: String, state: State<'_, AppState>) -> Result<UiState, String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    run_obs_action(state.inner(), id, ObsAction::Stop).await?;
    Ok(get_state(state))
}

#[tauri::command]
async fn restart_obs(setup_id: String, state: State<'_, AppState>) -> Result<UiState, String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    run_obs_action(state.inner(), id, ObsAction::Restart).await?;
    Ok(get_state(state))
}

struct ObsActionGuard<'a> {
    running: &'a Mutex<HashSet<uuid::Uuid>>,
    setup_id: uuid::Uuid,
}

impl Drop for ObsActionGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut running) = self.running.lock() {
            running.remove(&self.setup_id);
        }
    }
}

fn begin_obs_action(
    running: &Mutex<HashSet<uuid::Uuid>>,
    setup_id: uuid::Uuid,
) -> Result<ObsActionGuard<'_>, String> {
    let mut actions = running.lock().map_err(|_| "OBS action lock poisoned")?;
    if !actions.insert(setup_id) {
        return Err("Another OBS action is still running".into());
    }
    Ok(ObsActionGuard { running, setup_id })
}

async fn run_obs_action(
    state: &AppState,
    setup_id: uuid::Uuid,
    action: ObsAction,
) -> Result<(), String> {
    let _action_guard = begin_obs_action(&state.obs_actions_running, setup_id)?;
    let setup = state
        .config
        .lock()
        .map_err(|_| "config lock poisoned")?
        .setups
        .iter()
        .find(|setup| setup.id == setup_id)
        .cloned()
        .ok_or_else(|| "setup not found".to_string())?;

    if matches!(action, ObsAction::Stop | ObsAction::Restart)
        && !stop_setup_streams_with_timeout(
            Duration::from_secs(25),
            stop_setup_streams(&setup, state),
        )
        .await
    {
        tracing::warn!(setup_id = %setup.id, "Timed out stopping streams before OBS action");
    }
    let process_state = {
        let mut supervisor = state
            .supervisor
            .lock()
            .map_err(|_| "supervisor lock poisoned")?;
        match action {
            ObsAction::Start => {
                supervisor
                    .start(&setup, false, None)
                    .map_err(|error| error.to_string())?;
            }
            ObsAction::Stop => {
                supervisor.stop(&setup).map_err(|error| error.to_string())?;
            }
            ObsAction::Restart => {
                if supervisor.state(&setup) != PcObsState::NotRunning {
                    supervisor.stop(&setup).map_err(|error| error.to_string())?;
                }
                supervisor
                    .start(&setup, false, None)
                    .map_err(|error| error.to_string())?;
            }
            ObsAction::UploadLog => return Err("UploadLog is not an OBS process action".into()),
        }
        supervisor.state(&setup)
    };
    set_link_state(state, setup_id, process_state);
    Ok(())
}

fn create_log_upload(
    state: &AppState,
    setup_id: uuid::Uuid,
    kind: &str,
) -> Result<ConnectLogUpload, String> {
    let setup = state
        .config
        .lock()
        .map_err(|_| "config lock poisoned")?
        .setups
        .iter()
        .find(|setup| setup.id == setup_id)
        .cloned()
        .ok_or_else(|| "setup not found".to_string())?;
    let directory = config_dir(&setup).join(if kind == "crash_report" {
        "crashes"
    } else {
        "logs"
    });
    let files = std::fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then_some(ObsLogFile {
                path: entry.path(),
                modified_at: metadata.modified().ok(),
            })
        })
        .collect::<Vec<_>>();
    let start_time = state
        .supervisor
        .lock()
        .ok()
        .and_then(|supervisor| supervisor.obs_start_time(&setup));
    let selected = if kind == "crash_report" {
        select_crash_report_file(&files, start_time)
    } else {
        select_obs_log_file(&files, start_time)
    }
    .ok_or_else(|| format!("No {kind} file is available"))?;
    let content = std::fs::read(&selected.path).map_err(|error| error.to_string())?;
    let file_name = selected
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid log filename".to_string())?;
    let modified_at = selected
        .modified_at
        .map(|modified| chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339());
    encode_log_upload(kind, file_name, modified_at, &content).map_err(|error| error.to_string())
}

fn select_crash_report_file(
    files: &[ObsLogFile],
    obs_start_time: Option<std::time::SystemTime>,
) -> Option<&ObsLogFile> {
    files
        .iter()
        .filter_map(|file| Some((file, file.modified_at?)))
        .filter(|(_, modified_at)| {
            obs_start_time.is_none_or(|start_time| {
                start_time
                    .checked_sub(Duration::from_secs(2))
                    .is_some_and(|minimum_modified_at| *modified_at >= minimum_modified_at)
            })
        })
        .max_by_key(|(_, modified_at)| *modified_at)
        .map(|(file, _)| file)
}

async fn stop_setup_streams(setup: &Setup, state: &AppState) {
    let obs_state = state
        .supervisor
        .lock()
        .map(|mut supervisor| supervisor.state(setup))
        .unwrap_or(PcObsState::Unknown);
    if matches!(
        obs_state,
        PcObsState::Unknown | PcObsState::NotRunning | PcObsState::Crashed
    ) {
        return;
    }
    let effective = state.link_sessions.lock().ok().and_then(|sessions| {
        sessions
            .get(&setup.id)
            .and_then(|session| session.effective_obs.borrow().clone())
    });
    let connection = effective.or_else(|| {
        if setup.obs.use_dashboard_obs_settings {
            None
        } else {
            local_obs_connection(setup)
        }
    });
    if let Some((port, password)) = connection {
        let _ = stop_active_streams(port, &password).await;
    }
}

async fn stop_setup_streams_with_timeout<F>(deadline: Duration, future: F) -> bool
where
    F: std::future::Future<Output = ()>,
{
    tokio::time::timeout(deadline, future).await.is_ok()
}

fn local_obs_connection(setup: &Setup) -> Option<(u16, String)> {
    let local = read_obs_websocket_config(&config_dir(setup)).ok();
    let local_enabled = local.as_ref().is_some_and(|config| config.server_enabled);
    let port = if setup.obs.shared_config {
        setup.obs.websocket_port.or_else(|| {
            local
                .as_ref()
                .filter(|config| config.server_enabled)
                .map(|config| config.server_port)
        })
    } else {
        local
            .as_ref()
            .filter(|config| config.server_enabled)
            .map(|config| config.server_port)
    }?;
    let password = setup
        .obs
        .websocket_password_override
        .clone()
        .or_else(|| {
            local.as_ref().and_then(|config| {
                (config.auth_required && local_enabled).then(|| config.server_password.clone())
            })
        })
        .unwrap_or_default();
    Some((port, password))
}

#[tauri::command]
fn enable_websocket(
    app: tauri::AppHandle,
    setup_id: String,
    state: State<'_, AppState>,
) -> Result<UiState, String> {
    let setup = find_setup(&state, &setup_id)?;
    let mut supervisor = state
        .supervisor
        .lock()
        .map_err(|_| "supervisor lock poisoned")?;
    if supervisor.state(&setup) != connect_core::relay::PcObsState::NotRunning {
        return Err("Stop this setup's OBS before enabling its websocket server".into());
    }
    connect_core::supervisor::enable_obs_websocket_on_port(
        &config_dir(&setup),
        setup.obs.websocket_port,
    )
    .map_err(|error| error.to_string())?;
    drop(supervisor);
    if setup.pairing.is_some() {
        start_link_session(&app, state.inner(), setup)?;
    }
    Ok(get_state(state))
}

#[tauri::command]
fn unpair_setup(
    app: tauri::AppHandle,
    setup_id: String,
    state: State<'_, AppState>,
) -> Result<UiState, String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    clear_pairing_state(&state, id, true)?;
    let _ = app.emit("connect-state-changed", build_ui_state(&state));
    let _ = refresh_tray_menu(&app);
    Ok(get_state(state))
}

#[tauri::command]
fn remove_setup(
    app: tauri::AppHandle,
    setup_id: String,
    state: State<'_, AppState>,
) -> Result<UiState, String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    let token_delete_failed = remove_setup_state(&state, id)?;
    let _ = app.emit("connect-state-changed", build_ui_state(&state));
    let _ = refresh_tray_menu(&app);
    if token_delete_failed {
        let _ = app.emit(
            "connect-error",
            "The setup was removed, but stored-token cleanup could not be confirmed. It will be retried at startup.",
        );
    }
    Ok(get_state(state))
}

fn remove_setup_state(state: &AppState, setup_id: uuid::Uuid) -> Result<bool, String> {
    {
        let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
        if !config.setups.iter().any(|setup| setup.id == setup_id) {
            return Err("setup not found".into());
        }
        let mut updated_config = config.clone();
        updated_config.setups.retain(|setup| setup.id != setup_id);
        if !updated_config.pending_token_deletions.contains(&setup_id) {
            updated_config.pending_token_deletions.push(setup_id);
        }
        state
            .store
            .save(&updated_config)
            .map_err(|error| error.to_string())?;
        *config = updated_config;
    }
    if let Ok(mut pending) = state.pending_verification_urls.lock() {
        pending.remove(&setup_id);
    }
    remove_link_session(state, setup_id, true)?;
    let forget_result = state
        .supervisor
        .lock()
        .map_err(|_| "supervisor lock poisoned".to_owned())
        .map(|mut supervisor| supervisor.forget(setup_id));
    forget_result?;
    if let Err(error) = state.tokens.delete(setup_id) {
        tracing::warn!(setup_id = %setup_id, error = %error, "Failed to delete token for removed setup");
        return Ok(true);
    }
    if let Err(error) = clear_pending_token_deletion(state, setup_id) {
        tracing::warn!(setup_id = %setup_id, error = %error, "Failed to persist completed token deletion");
        return Ok(true);
    }
    Ok(false)
}

fn clear_pending_token_deletion(state: &AppState, setup_id: uuid::Uuid) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
    if !config.pending_token_deletions.contains(&setup_id) {
        return Ok(());
    }
    let mut updated_config = config.clone();
    updated_config
        .pending_token_deletions
        .retain(|pending_id| *pending_id != setup_id);
    state
        .store
        .save(&updated_config)
        .map_err(|error| error.to_string())?;
    *config = updated_config;
    Ok(())
}

fn retry_pending_token_deletions(
    pending: &[uuid::Uuid],
    mut delete_token: impl FnMut(uuid::Uuid) -> bool,
) -> Vec<uuid::Uuid> {
    let mut seen = HashSet::new();
    let mut remaining = Vec::new();
    for setup_id in pending.iter().copied() {
        if seen.insert(setup_id) && !delete_token(setup_id) {
            remaining.push(setup_id);
        }
    }
    remaining
}

fn cleanup_startup_tokens(
    store: &ConfigStore,
    tokens: &TokenStore,
    config: &mut AppConfig,
    config_loaded_without_recovery_error: bool,
) {
    if !config_loaded_without_recovery_error {
        return;
    }
    let mut updated_config = config.clone();
    let pending = updated_config.pending_token_deletions.clone();
    updated_config.pending_token_deletions =
        retry_pending_token_deletions(&pending, |setup_id| tokens.delete(setup_id).is_ok());
    if updated_config.pending_token_deletions != config.pending_token_deletions {
        match store.save(&updated_config) {
            Ok(()) => *config = updated_config,
            Err(error) => tracing::warn!(
                error = %error,
                "Failed to persist pending token deletion retries at startup"
            ),
        }
    }

    let active_setups = config
        .setups
        .iter()
        .map(|setup| setup.id)
        .collect::<HashSet<_>>();
    if let Err(error) = tokens.remove_orphan_fallback_tokens(&active_setups) {
        tracing::warn!(error = %error, "Failed to remove orphaned fallback tokens at startup");
    }
}

fn clear_pairing_state(
    state: &AppState,
    setup_id: uuid::Uuid,
    abort_link_task: bool,
) -> Result<(), String> {
    state
        .tokens
        .delete(setup_id)
        .map_err(|error| error.to_string())?;
    let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
    if let Some(setup) = config.setups.iter_mut().find(|setup| setup.id == setup_id) {
        setup.pairing = None;
    }
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    drop(config);
    remove_link_session(state, setup_id, abort_link_task)
}

fn remove_link_session(
    state: &AppState,
    setup_id: uuid::Uuid,
    abort_link_task: bool,
) -> Result<(), String> {
    let session = state
        .link_sessions
        .lock()
        .map_err(|_| "link session lock poisoned".to_owned())?
        .remove(&setup_id);
    if let Some(session) = session {
        if abort_link_task {
            session.task.abort();
        }
        session.monitor.abort();
        session.command_consumer.abort();
    }
    state
        .supervisor
        .lock()
        .map_err(|_| "supervisor lock poisoned")?
        .clear_connected(setup_id);
    Ok(())
}

fn finish_taken_over_session(app: &tauri::AppHandle, setup_id: uuid::Uuid) {
    let state = app.state::<AppState>();
    if let Err(error) = remove_link_session(&state, setup_id, false) {
        let _ = app.emit("connect-error", error);
        return;
    }
    let _ = app.emit("connect-state-changed", build_ui_state(&state));
    let _ = refresh_tray_menu(app);
    let _ = app.emit(
        "connect-error",
        "Another computer or browser source took over this switcher. Restart NOALBS Cloud Connect to connect again.",
    );
}

fn relay_error_requires_pairing_cleanup(error: &connect_core::relay::RelayError) -> bool {
    matches!(error, connect_core::relay::RelayError::InvalidToken)
}

fn forget_rejected_pairing(app: &tauri::AppHandle, setup_id: uuid::Uuid) {
    let state = app.state::<AppState>();
    match clear_pairing_state(&state, setup_id, false) {
        Ok(()) => {}
        Err(error) => {
            let _ = app.emit("connect-error", error);
            return;
        }
    }
    let _ = app.emit("connect-state-changed", build_ui_state(&state));
    let _ = refresh_tray_menu(app);
    let _ = app.emit(
        "connect-error",
        "This computer was removed from NOALBS Cloud. Pair it again to reconnect.",
    );
}

fn find_setup(state: &State<'_, AppState>, setup_id: &str) -> Result<Setup, String> {
    state
        .config
        .lock()
        .map_err(|_| "config lock poisoned")?
        .setups
        .iter()
        .find(|setup| setup.id.to_string() == setup_id)
        .cloned()
        .ok_or_else(|| "setup not found".into())
}

fn set_link_state(state: &AppState, id: uuid::Uuid, next: PcObsState) {
    if let Ok(sessions) = state.link_sessions.lock()
        && let Some(session) = sessions.get(&id)
    {
        let _ = session.state.send(next);
    }
}

fn should_publish_not_running(process_state: PcObsState, link_state: PcObsState) -> bool {
    process_state == PcObsState::NotRunning
        && matches!(
            link_state,
            PcObsState::Unknown
                | PcObsState::Starting
                | PcObsState::Connected
                | PcObsState::NotResponding
                | PcObsState::WaitingForInput
        )
}

fn displayed_obs_state(process_state: PcObsState, link_state: Option<PcObsState>) -> PcObsState {
    link_state.unwrap_or(process_state)
}

fn obs_state_label(state: PcObsState) -> &'static str {
    match state {
        PcObsState::Connected => "OBS connected",
        PcObsState::Starting => "Starting OBS…",
        PcObsState::WaitingForInput => "OBS is waiting for input",
        PcObsState::SafeMode => "OBS is asking about Safe Mode",
        PcObsState::WebsocketDisabled => "OBS websocket is off",
        PcObsState::NotResponding => "OBS is not responding",
        PcObsState::Crashed => "OBS crashed",
        PcObsState::NotRunning => "OBS not running",
        PcObsState::Unknown => "OBS running, not started by Connect",
    }
}

fn tray_setup_labels(state: &AppState) -> Vec<(String, String)> {
    let setups = state.config.lock().expect("app config lock").setups.clone();
    let mut supervisor = state.supervisor.lock().expect("supervisor lock");
    supervisor.set_known_setups(&setups);
    let sessions = state.link_sessions.lock().expect("link session lock");
    setups
        .iter()
        .map(|setup| {
            let link_state = sessions
                .get(&setup.id)
                .map(|session| *session.state.borrow());
            let process_state =
                link_state.map_or_else(|| supervisor.state(setup), |_| PcObsState::Unknown);
            let status = displayed_obs_state(process_state, link_state);
            (
                format!("setup-{}", setup.id),
                format!("{} — {}", setup.name, obs_state_label(status)),
            )
        })
        .collect()
}

fn setup_for_watchdog_restart(config: &AppConfig, fallback: &Setup) -> Setup {
    config
        .setups
        .iter()
        .find(|setup| setup.id == fallback.id)
        .cloned()
        .unwrap_or_else(|| fallback.clone())
}

fn start_link_session(
    app: &tauri::AppHandle,
    state: &AppState,
    setup: Setup,
) -> Result<(), String> {
    let pairing = setup.pairing.as_ref().ok_or("setup is not paired")?;
    let token = state
        .tokens
        .get(setup.id)
        .map_err(|error| error.to_string())?
        .ok_or("paired device token is missing")?;
    let effective_obs = if setup.obs.use_dashboard_obs_settings {
        None
    } else {
        local_obs_connection(&setup)
    };
    let (initial_state, obs_port, obs_password) = match &effective_obs {
        Some((port, password)) => (PcObsState::Unknown, Some(*port), password.clone()),
        None if setup.obs.use_dashboard_obs_settings => (PcObsState::Unknown, None, String::new()),
        None => (PcObsState::WebsocketDisabled, None, String::new()),
    };
    let (state_tx, state_rx) = watch::channel(initial_state);
    let (watchdog_tx, watchdog_rx) = watch::channel(WatchdogMode::Restart);
    let (telemetry_tx, telemetry_rx) = watch::channel(ObsTelemetry::default());
    let (effective_obs_tx, effective_obs_rx) = watch::channel(effective_obs);
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ObsCommandRequest>();
    let (upload_tx, upload_rx) = mpsc::unbounded_channel();
    let link_config = LinkConfig {
        link_url: pairing.link_url.clone(),
        device_token: token,
        version: env!("CARGO_PKG_VERSION").into(),
        os: ConnectOs::current().as_str().into(),
        obs_port,
        obs_password,
        use_dashboard_obs_settings: setup.obs.use_dashboard_obs_settings,
        state_tx: state_tx.clone(),
        telemetry_tx,
        effective_obs_tx,
        commands: command_tx,
        unsolicited_uploads: upload_rx,
    };
    let mut sessions = state
        .link_sessions
        .lock()
        .map_err(|_| "link session lock poisoned")?;
    if let Some(previous) = sessions.remove(&setup.id) {
        previous.task.abort();
        previous.monitor.abort();
        previous.command_consumer.abort();
    }
    let app_handle = app.clone();
    let link_setup_id = setup.id;
    let (link_start, link_started) = tokio::sync::oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        if link_started.await.is_err() {
            return;
        }
        match run_link(link_config, state_rx, watchdog_tx).await {
            Ok(()) => {}
            Err(error) if relay_error_requires_pairing_cleanup(&error) => {
                forget_rejected_pairing(&app_handle, link_setup_id);
            }
            Err(connect_core::relay::RelayError::TakenOver) => {
                finish_taken_over_session(&app_handle, link_setup_id);
            }
            Err(error) => {
                let _ = app_handle.emit("connect-error", error.to_string());
            }
        }
    });
    let monitor_handle = app.clone();
    let monitor_setup = setup.clone();
    let monitor_state = state_tx.subscribe();
    let mut monitor_telemetry = telemetry_rx.clone();
    let mut monitor_watchdog = watchdog_rx.clone();
    let monitor_state_tx = state_tx.clone();
    let monitor = tauri::async_runtime::spawn(async move {
        let monitor_state = monitor_state;
        let mut watchdog = Watchdog::default();
        let started_at = std::time::Instant::now();
        let mut handled_incident = None;
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let state = *monitor_state.borrow();
                    let app_state = monitor_handle.state::<AppState>();
                    let process_state = app_state
                        .supervisor
                        .lock()
                        .map(|mut supervisor| supervisor.state(&monitor_setup))
                        .unwrap_or(PcObsState::Unknown);
                    if process_state == PcObsState::WaitingForInput
                        && matches!(state, PcObsState::Unknown | PcObsState::Starting)
                    {
                        let _ = monitor_state_tx.send(PcObsState::WaitingForInput);
                    }
                    if should_publish_not_running(process_state, state) {
                        let _ = monitor_state_tx.send(PcObsState::NotRunning);
                    }
                    let incident = if process_state == PcObsState::Crashed {
                        Some(PcObsState::Crashed)
                    } else if state == PcObsState::NotResponding
                        && process_state != PcObsState::NotRunning
                    {
                        Some(PcObsState::NotResponding)
                    } else {
                        None
                    };
                    let Some(incident) = incident else {
                        handled_incident = None;
                        continue;
                    };
                    if handled_incident == Some(incident) {
                        continue;
                    }
                    handled_incident = Some(incident);
                    if incident == PcObsState::Crashed {
                        for kind in ["crash_report", "obs_log"] {
                            if let Ok(upload) = create_log_upload(&app_state, monitor_setup.id, kind)
                                && upload_tx.send(upload).is_err()
                            {
                                break;
                            }
                        }
                    }
                    let telemetry = monitor_telemetry.borrow().clone();
                    let watchdog_mode = *monitor_watchdog.borrow();
                    match watchdog.decide(
                        started_at.elapsed(),
                        incident,
                        watchdog_mode,
                        telemetry.stream_active,
                        false,
                    ) {
                        WatchdogAction::None => {}
                        WatchdogAction::StopAndReport => {
                            let _ = monitor_handle.emit(
                                "connect-error",
                                format!("Watchdog stopped after three restarts in ten minutes for {}", monitor_setup.name),
                            );
                        }
                        WatchdogAction::Restart { resume } => {
                            let restart_setup = app_state
                                .config
                                .lock()
                                .map(|config| {
                                    setup_for_watchdog_restart(&config, &monitor_setup)
                                })
                                .unwrap_or_else(|_| monitor_setup.clone());
                            if incident == PcObsState::NotResponding
                                && !stop_setup_streams_with_timeout(
                                    Duration::from_secs(25),
                                    stop_setup_streams(&restart_setup, &app_state),
                                )
                                .await
                            {
                                tracing::warn!(
                                    setup_id = %restart_setup.id,
                                    "Timed out stopping streams before OBS watchdog restart"
                                );
                            }
                            let restart_result = {
                                let mut supervisor =
                                    app_state.supervisor.lock().expect("supervisor lock");
                                if incident == PcObsState::NotResponding
                                    && let Err(error) = supervisor.stop(&restart_setup)
                                {
                                    Err(error)
                                } else {
                                    supervisor
                                        .start(
                                            &restart_setup,
                                            resume,
                                            telemetry.last_program_scene.as_deref(),
                                        )
                                        .map(|_| ())
                                }
                            };
                            match restart_result {
                                Ok(()) => {
                                    let _ = monitor_state_tx.send(PcObsState::Starting);
                                }
                                Err(error) => {
                                    let _ = monitor_handle.emit("connect-error", error.to_string());
                                }
                            }
                        }
                    }
                }
                changed = monitor_watchdog.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    handled_incident = None;
                }
                changed = monitor_telemetry.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let version = monitor_telemetry.borrow().obs_version.clone();
                    if let Some(version) = version
                        && let Ok(mut config) = monitor_handle
                            .state::<AppState>()
                            .config
                            .lock()
                        && let Some(setup) = config
                            .setups
                            .iter_mut()
                            .find(|setup| setup.id == monitor_setup.id)
                    {
                        setup.obs.last_obs_version = Some(version);
                        let _ = monitor_handle.state::<AppState>().store.save(&config);
                    }
                }
            }
        }
    });
    let command_app = app.clone();
    let command_setup_id = setup.id;
    let command_consumer = tauri::async_runtime::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            let app_state = command_app.state::<AppState>();
            let result = if command.action == ObsAction::UploadLog {
                create_log_upload(app_state.inner(), command_setup_id, "obs_log").map(Some)
            } else {
                run_obs_action(app_state.inner(), command_setup_id, command.action)
                    .await
                    .map(|()| None)
            };
            let _ = command.reply.send(result);
            let _ = command_app.emit(
                "connect-state-changed",
                get_state(command_app.state::<AppState>()),
            );
            let _ = refresh_tray_menu(&command_app);
        }
    });
    sessions.insert(
        setup.id,
        LinkSession {
            state: state_tx.clone(),
            effective_obs: effective_obs_rx,
            task,
            monitor,
            command_consumer,
        },
    );
    drop(sessions);
    state
        .supervisor
        .lock()
        .map_err(|_| "supervisor lock poisoned")?
        .clear_connected(setup.id);
    let _ = link_start.send(());

    let app_handle = app.clone();
    let mut state_rx = state_tx.subscribe();
    tauri::async_runtime::spawn(async move {
        while state_rx.changed().await.is_ok() {
            let state = app_handle.state::<AppState>();
            if let Ok(mut supervisor) = state.supervisor.lock() {
                let current_state = *state_rx.borrow();
                supervisor.mark_connected(setup.id, current_state == PcObsState::Connected);
            }
            let _ = app_handle.emit("connect-state-changed", get_state(state));
            let _ = refresh_tray_menu(&app_handle);
        }
    });
    Ok(())
}

#[cfg(target_os = "linux")]
fn tray_available() -> bool {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&connection) else {
        return false;
    };
    let Ok(name) = zbus::names::BusName::try_from("org.kde.StatusNotifierWatcher") else {
        return false;
    };
    proxy.name_has_owner(name).unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn tray_available() -> bool {
    true
}

fn build_tray_menu(manager: &impl Manager<tauri::Wry>) -> tauri::Result<Menu<tauri::Wry>> {
    let state = manager.state::<AppState>();
    let setup_labels = tray_setup_labels(&state);
    let menu = Menu::new(manager)?;
    for (id, label) in setup_labels {
        let item = MenuItem::with_id(manager, id, label, false, None::<&str>)?;
        menu.append(&item)?;
    }
    let open = MenuItem::with_id(manager, "open", "Open", true, None::<&str>)?;
    let quit = MenuItem::with_id(manager, "quit", "Quit", true, None::<&str>)?;
    menu.append(&open)?;
    menu.append(&quit)?;
    Ok(menu)
}

fn install_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let menu = build_tray_menu(app)?;
    let mut builder = TrayIconBuilder::with_id("connect-tray").menu(&menu);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn refresh_tray_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id("connect-tray") {
        tray.set_menu(Some(build_tray_menu(app)?))?;
    }
    Ok(())
}

fn start_ui_state_monitor(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last_states = None;
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let state = get_state(app.state::<AppState>());
            let current_states: HashMap<_, _> = state
                .setups
                .iter()
                .map(|setup| {
                    (
                        setup.id.clone(),
                        (
                            setup.name.clone(),
                            setup.obs_state.clone(),
                            setup.obs_process_running,
                        ),
                    )
                })
                .collect();
            if last_states.as_ref() != Some(&current_states) {
                last_states = Some(current_states);
                let _ = app.emit("connect-state-changed", state);
                let _ = refresh_tray_menu(&app);
            }
        }
    });
}

fn main() {
    let store = ConfigStore::new(ConfigStore::default_path().expect("user config path"));
    let (mut config, mut startup_error) = store.load_or_backup();
    let config_loaded_without_recovery_error = startup_error.is_none();
    if remove_unsupported_config_dir_overrides(&mut config, HostPlatform::current()) {
        let mut notice =
            "The separate settings folder isn't supported here and was removed.".to_owned();
        if let Err(error) = store.save(&config) {
            notice.push_str(&format!(
                " The updated configuration could not be saved: {error}"
            ));
        }
        startup_error = Some(
            startup_error
                .map(|error| format!("{error}\n{notice}"))
                .unwrap_or(notice),
        );
    }
    let tokens = TokenStore::default_store().expect("token store path");
    cleanup_startup_tokens(
        &store,
        &tokens,
        &mut config,
        config_loaded_without_recovery_error,
    );
    let autostart_builder = tauri_plugin_autostart::Builder::new().args(["--minimized"]);
    #[cfg(target_os = "macos")]
    let autostart_builder = autostart_builder.macos_launcher(MacosLauncher::LaunchAgent);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(autostart_builder.build())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            config: Mutex::new(config),
            startup_error,
            store,
            tokens,
            supervisor: Mutex::new(Supervisor::default()),
            link_sessions: Mutex::new(HashMap::new()),
            obs_actions_running: Mutex::new(HashSet::new()),
            pending_verification_urls: Mutex::new(HashMap::new()),
        })
        .setup(|app| {
            let has_tray = tray_available();
            if has_tray {
                install_tray(app)?;
            }
            let app_handle = app.handle().clone();
            if let Some(window) = app.get_webview_window("main") {
                let window_for_event = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if has_tray {
                            api.prevent_close();
                            let _ = window_for_event.hide();
                        } else {
                            app_handle.exit(0);
                        }
                    }
                });
            }
            if std::env::args().any(|argument| argument == "--minimized")
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.show();
                if has_tray {
                    let _ = window.hide();
                } else {
                    let _ = window.minimize();
                }
            }
            let state = app.state::<AppState>();
            let start_at_login = {
                let config = state.config.lock().expect("app config lock");
                config.settings.start_at_login
            };
            if start_at_login {
                let _ = app.autolaunch().enable();
            }
            let setups = state.config.lock().expect("app config lock").setups.clone();
            state
                .supervisor
                .lock()
                .expect("supervisor lock")
                .set_known_setups(&setups);
            for setup in setups {
                if setup.obs.start_obs_with_connect {
                    let _ = state
                        .supervisor
                        .lock()
                        .expect("supervisor lock")
                        .start(&setup, false, None);
                }
                if setup.pairing.is_some() {
                    let _ = start_link_session(app.handle(), state.inner(), setup);
                }
            }
            start_ui_state_monitor(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            add_setup,
            save_settings,
            save_setup,
            create_pairing,
            pair_setup,
            open_verification_url,
            start_obs,
            stop_obs,
            restart_obs,
            enable_websocket,
            unpair_setup,
            remove_setup
        ])
        .run(tauri::generate_context!())
        .expect("failed to run NOALBS Cloud Connect");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_report_selection_ignores_files_older_than_obs_start_slack() {
        let directory =
            std::env::temp_dir().join(format!("crash-selection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let start = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let stale_path = directory.join("stale.dmp");
        let fresh_path = directory.join("fresh.dmp");
        let stale = std::fs::File::create(&stale_path).unwrap();
        stale.set_modified(start - Duration::from_secs(3)).unwrap();
        let fresh = std::fs::File::create(&fresh_path).unwrap();
        fresh.set_modified(start - Duration::from_secs(1)).unwrap();
        let files = vec![
            ObsLogFile {
                path: stale_path.clone(),
                modified_at: Some(start - Duration::from_secs(3)),
            },
            ObsLogFile {
                path: fresh_path.clone(),
                modified_at: Some(start - Duration::from_secs(1)),
            },
        ];

        assert_eq!(
            select_crash_report_file(&files, Some(start)).unwrap().path,
            fresh_path
        );
        assert!(select_crash_report_file(&files[..1], Some(start)).is_none());
        assert_eq!(
            select_crash_report_file(&files[..1], None).unwrap().path,
            stale_path
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn stream_stop_timeout_returns_when_obs_side_hangs() {
        assert!(
            !stop_setup_streams_with_timeout(
                Duration::from_millis(1),
                std::future::pending::<()>(),
            )
            .await
        );
    }

    #[test]
    fn taken_over_relay_error_does_not_forget_pairing() {
        assert!(!relay_error_requires_pairing_cleanup(
            &connect_core::relay::RelayError::TakenOver
        ));
        assert!(relay_error_requires_pairing_cleanup(
            &connect_core::relay::RelayError::InvalidToken
        ));
    }

    #[test]
    fn mixed_normal_and_portable_setups_are_rejected_in_either_order() {
        let test_dir =
            std::env::temp_dir().join(format!("noalbs-connect-test-{}", uuid::Uuid::new_v4()));
        let binary = test_dir.join("obs/bin/64bit/obs64.exe");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, []).unwrap();

        let mut normal = Setup::new("Normal OBS".into());
        normal.obs.binary_path = Some(binary.clone());
        let mut portable = Setup::new("Portable OBS".into());
        portable.obs.binary_path = Some(binary);
        portable.obs.portable = true;

        assert_eq!(
            validate_setup_identity_for_platform(
                std::slice::from_ref(&normal),
                &portable,
                HostPlatform::Windows
            ),
            Err(
                "This OBS is already used by another setup in normal mode; use a separate portable copy of OBS"
                    .into()
            )
        );
        assert_eq!(
            validate_setup_identity_for_platform(
                std::slice::from_ref(&portable),
                &normal,
                HostPlatform::Windows
            ),
            Err(
                "This OBS is already used by another setup in portable mode; use a separate normal OBS install"
                    .into()
            )
        );

        std::fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn unsupported_config_dir_overrides_are_removed_only_on_rejecting_platforms() {
        let mut setup = Setup::new("Legacy setup".into());
        setup.obs.config_dir_override = Some(std::path::PathBuf::from("/legacy/obs-settings"));
        let mut second_setup = Setup::new("Another legacy setup".into());
        second_setup.obs.config_dir_override =
            Some(std::path::PathBuf::from("/legacy/other-obs-settings"));
        let mut config = AppConfig {
            setups: vec![setup, second_setup],
            ..Default::default()
        };

        assert!(remove_unsupported_config_dir_overrides(
            &mut config,
            HostPlatform::Windows
        ));
        assert!(
            config
                .setups
                .iter()
                .all(|setup| setup.obs.config_dir_override.is_none())
        );
        assert!(!remove_unsupported_config_dir_overrides(
            &mut config,
            HostPlatform::Windows
        ));

        for setup in &mut config.setups {
            setup.obs.config_dir_override = Some(std::path::PathBuf::from("/legacy/obs-settings"));
        }
        assert!(!remove_unsupported_config_dir_overrides(
            &mut config,
            HostPlatform::Linux
        ));
        assert!(
            config
                .setups
                .iter()
                .all(|setup| setup.obs.config_dir_override.is_some())
        );
    }

    #[test]
    fn watchdog_restart_uses_the_latest_setup_version() {
        let mut fallback = Setup::new("OBS".into());
        fallback.obs.last_obs_version = Some("33.0.1".into());
        let mut current = fallback.clone();
        current.obs.last_obs_version = Some("30.2.3".into());
        let config = AppConfig {
            setups: vec![current],
            ..Default::default()
        };

        let setup = setup_for_watchdog_restart(&config, &fallback);
        assert_eq!(setup.obs.last_obs_version.as_deref(), Some("30.2.3"));
    }

    #[test]
    fn process_exit_corrects_stale_starting_state_without_overwriting_disabled() {
        assert!(should_publish_not_running(
            PcObsState::NotRunning,
            PcObsState::Starting
        ));
        assert!(!should_publish_not_running(
            PcObsState::NotRunning,
            PcObsState::WebsocketDisabled
        ));
        assert!(!should_publish_not_running(
            PcObsState::Starting,
            PcObsState::Starting
        ));
    }

    #[test]
    fn unpaired_setup_uses_the_supervisor_obs_state() {
        assert_eq!(
            displayed_obs_state(PcObsState::Starting, None),
            PcObsState::Starting
        );
        assert_eq!(
            displayed_obs_state(PcObsState::NotRunning, None),
            PcObsState::NotRunning
        );
        assert_eq!(
            displayed_obs_state(PcObsState::Starting, Some(PcObsState::Connected)),
            PcObsState::Connected
        );
    }

    #[test]
    fn tray_obs_state_label_matches_the_window_label() {
        assert_eq!(obs_state_label(PcObsState::Starting), "Starting OBS…");
        assert_eq!(obs_state_label(PcObsState::NotRunning), "OBS not running");
    }

    #[test]
    fn obs_actions_are_exclusive_per_setup() {
        let running = Mutex::new(HashSet::new());
        let setup_id = uuid::Uuid::new_v4();
        let action = begin_obs_action(&running, setup_id).unwrap();
        assert!(matches!(
            begin_obs_action(&running, setup_id),
            Err(error) if error == "Another OBS action is still running"
        ));
        drop(action);
        assert!(begin_obs_action(&running, setup_id).is_ok());
    }

    #[test]
    fn tray_setup_labels_use_setup_ids_and_human_readable_states() {
        let mut setup = Setup::new("Smoke test".into());
        setup.obs.install_kind = connect_core::config::InstallKind::CustomBinary;
        setup.obs.binary_path = Some("/noalbs-connect-test-obs".into());
        let setup_id = setup.id;
        let state = AppState {
            config: Mutex::new(AppConfig {
                setups: vec![setup],
                ..Default::default()
            }),
            startup_error: None,
            store: ConfigStore::new(Default::default()),
            tokens: TokenStore::new(Default::default()),
            supervisor: Mutex::new(Supervisor::default()),
            link_sessions: Mutex::new(HashMap::new()),
            obs_actions_running: Mutex::new(HashSet::new()),
            pending_verification_urls: Mutex::new(HashMap::new()),
        };

        assert_eq!(
            tray_setup_labels(&state),
            vec![(
                format!("setup-{setup_id}"),
                "Smoke test — OBS not running".into()
            )]
        );
    }

    #[test]
    fn pending_token_deletion_retries_keep_failures_and_drop_completed_entries() {
        let failed_setup = uuid::Uuid::new_v4();
        let absent_setup = uuid::Uuid::new_v4();
        let pending = vec![failed_setup, absent_setup, failed_setup];

        let remaining =
            retry_pending_token_deletions(&pending, |setup_id| setup_id == absent_setup);

        assert_eq!(remaining, [failed_setup]);
        assert!(
            retry_pending_token_deletions(&remaining, |_| true).is_empty(),
            "successful deletion or confirmed absence should clear the retry id"
        );
    }

    #[test]
    fn broken_config_does_not_sweep_fallback_tokens() {
        let test_dir =
            std::env::temp_dir().join(format!("noalbs-connect-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let config_path = test_dir.join("config.json");
        std::fs::write(&config_path, b"{not valid json").unwrap();
        let store = ConfigStore::new(config_path);
        let tokens = TokenStore::new(test_dir.clone());
        let setup_id = uuid::Uuid::new_v4();
        let fallback_path = test_dir.join("tokens").join(setup_id.to_string());
        std::fs::create_dir_all(fallback_path.parent().unwrap()).unwrap();
        std::fs::write(&fallback_path, "orphan-token").unwrap();
        let (mut config, recovery_error) = store.load_or_backup();

        assert!(recovery_error.is_some());
        cleanup_startup_tokens(&store, &tokens, &mut config, recovery_error.is_none());

        assert_eq!(
            std::fs::read_to_string(fallback_path).unwrap(),
            "orphan-token"
        );
        std::fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn startup_sweep_removes_orphan_fallback_tokens_after_valid_config_load() {
        let test_dir =
            std::env::temp_dir().join(format!("noalbs-connect-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let store = ConfigStore::new(test_dir.join("config.json"));
        store.save(&AppConfig::default()).unwrap();
        let tokens = TokenStore::new(test_dir.clone());
        let orphaned_setup = uuid::Uuid::new_v4();
        let fallback_path = test_dir.join("tokens").join(orphaned_setup.to_string());
        std::fs::create_dir_all(fallback_path.parent().unwrap()).unwrap();
        std::fs::write(&fallback_path, "orphan-token").unwrap();
        let (mut config, recovery_error) = store.load_or_backup();

        cleanup_startup_tokens(&store, &tokens, &mut config, recovery_error.is_none());

        assert!(recovery_error.is_none());
        assert!(!fallback_path.exists());
        std::fs::remove_dir_all(test_dir).unwrap();
    }

    #[tokio::test]
    async fn clearing_pairing_removes_the_session_and_token() {
        let test_dir =
            std::env::temp_dir().join(format!("noalbs-connect-test-{}", uuid::Uuid::new_v4()));
        let mut setup = Setup::new("Rejected pairing".into());
        setup.obs.install_kind = connect_core::config::InstallKind::CustomBinary;
        setup.obs.binary_path = Some(test_dir.join("obs"));
        setup.pairing = Some(connect_core::config::Pairing {
            device_id: uuid::Uuid::new_v4(),
            switcher_id: uuid::Uuid::new_v4(),
            link_url: "ws://127.0.0.1:11000/link".into(),
        });
        let setup_id = setup.id;
        let store = ConfigStore::new(test_dir.join("config.json"));
        let tokens = TokenStore::new(test_dir.clone());
        tokens.set(setup_id, "rejected-token").unwrap();
        let state = AppState {
            config: Mutex::new(AppConfig {
                setups: vec![setup],
                ..Default::default()
            }),
            startup_error: None,
            store: store.clone(),
            tokens: tokens.clone(),
            supervisor: Mutex::new(Supervisor::default()),
            link_sessions: Mutex::new(HashMap::new()),
            obs_actions_running: Mutex::new(HashSet::new()),
            pending_verification_urls: Mutex::new(HashMap::new()),
        };
        let (state_tx, _) = watch::channel(PcObsState::Starting);
        let (_, effective_obs) = watch::channel(None);
        let link_task = tokio::spawn(std::future::pending::<()>());
        let link_task_abort = link_task.abort_handle();
        state.link_sessions.lock().unwrap().insert(
            setup_id,
            LinkSession {
                state: state_tx,
                effective_obs,
                task: tauri::async_runtime::JoinHandle::Tokio(link_task),
                monitor: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
                command_consumer: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
            },
        );

        clear_pairing_state(&state, setup_id, false).unwrap();
        let config = store.load().unwrap();
        let ui_state = build_ui_state(&state);

        assert!(state.link_sessions.lock().unwrap().is_empty());
        assert!(config.setups[0].pairing.is_none());
        assert_eq!(tokens.get(setup_id).unwrap(), None);
        assert!(!ui_state.setups[0].paired);
        assert_eq!(ui_state.setups[0].obs_state, "not_running");
        assert!(!link_task_abort.is_finished());
        link_task_abort.abort();
        std::fs::remove_dir_all(test_dir).unwrap();
    }

    #[tokio::test]
    async fn removing_setup_deletes_pairing_session_and_token() {
        let test_dir =
            std::env::temp_dir().join(format!("noalbs-connect-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let mut setup = Setup::new("Removed setup".into());
        setup.pairing = Some(connect_core::config::Pairing {
            device_id: uuid::Uuid::new_v4(),
            switcher_id: uuid::Uuid::new_v4(),
            link_url: "ws://127.0.0.1:11000/link".into(),
        });
        let setup_id = setup.id;
        let store = ConfigStore::new(test_dir.join("config.json"));
        let tokens = TokenStore::new(test_dir.clone());
        tokens.set(setup_id, "removed-token").unwrap();
        let state = AppState {
            config: Mutex::new(AppConfig {
                setups: vec![setup],
                ..Default::default()
            }),
            startup_error: None,
            store: store.clone(),
            tokens: tokens.clone(),
            supervisor: Mutex::new(Supervisor::default()),
            link_sessions: Mutex::new(HashMap::new()),
            obs_actions_running: Mutex::new(HashSet::new()),
            pending_verification_urls: Mutex::new(HashMap::new()),
        };
        let (state_tx, _) = watch::channel(PcObsState::Starting);
        let (_, effective_obs) = watch::channel(None);
        state.link_sessions.lock().unwrap().insert(
            setup_id,
            LinkSession {
                state: state_tx,
                effective_obs,
                task: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
                monitor: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
                command_consumer: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
            },
        );

        let cleanup_warning = remove_setup_state(&state, setup_id).unwrap();

        assert!(state.config.lock().unwrap().setups.is_empty());
        assert!(state.link_sessions.lock().unwrap().is_empty());
        assert_eq!(tokens.get(setup_id).unwrap(), None);
        let persisted_config = store.load().unwrap();
        assert!(persisted_config.setups.is_empty());
        if cleanup_warning {
            assert_eq!(persisted_config.pending_token_deletions, [setup_id]);
        } else {
            assert!(persisted_config.pending_token_deletions.is_empty());
        }
        std::fs::remove_dir_all(test_dir).unwrap();
    }

    #[tokio::test]
    async fn failed_setup_removal_save_keeps_config_and_link_session() {
        let test_dir =
            std::env::temp_dir().join(format!("noalbs-connect-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let blocked_parent = test_dir.join("not-a-directory");
        std::fs::write(&blocked_parent, "blocker").unwrap();
        let mut setup = Setup::new("Kept setup".into());
        setup.pairing = Some(connect_core::config::Pairing {
            device_id: uuid::Uuid::new_v4(),
            switcher_id: uuid::Uuid::new_v4(),
            link_url: "ws://127.0.0.1:11000/link".into(),
        });
        let setup_id = setup.id;
        let tokens = TokenStore::new(test_dir.clone());
        tokens.set(setup_id, "kept-token").unwrap();
        let state = AppState {
            config: Mutex::new(AppConfig {
                setups: vec![setup],
                ..Default::default()
            }),
            startup_error: None,
            store: ConfigStore::new(blocked_parent.join("config.json")),
            tokens: tokens.clone(),
            supervisor: Mutex::new(Supervisor::default()),
            link_sessions: Mutex::new(HashMap::new()),
            obs_actions_running: Mutex::new(HashSet::new()),
            pending_verification_urls: Mutex::new(HashMap::from([(
                setup_id,
                "verification-url".into(),
            )])),
        };
        let (state_tx, _) = watch::channel(PcObsState::Starting);
        let (_, effective_obs) = watch::channel(None);
        let link_task = tokio::spawn(std::future::pending::<()>());
        let link_task_abort = link_task.abort_handle();
        let command_consumer = tokio::spawn(std::future::pending::<()>());
        let command_consumer_abort = command_consumer.abort_handle();
        state.link_sessions.lock().unwrap().insert(
            setup_id,
            LinkSession {
                state: state_tx,
                effective_obs,
                task: tauri::async_runtime::JoinHandle::Tokio(link_task),
                monitor: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
                command_consumer: tauri::async_runtime::JoinHandle::Tokio(command_consumer),
            },
        );

        assert!(remove_setup_state(&state, setup_id).is_err());

        assert_eq!(state.config.lock().unwrap().setups[0].id, setup_id);
        assert!(state.config.lock().unwrap().setups[0].pairing.is_some());
        assert!(
            state
                .config
                .lock()
                .unwrap()
                .pending_token_deletions
                .is_empty()
        );
        assert_eq!(tokens.get(setup_id).unwrap().as_deref(), Some("kept-token"));
        assert!(state.link_sessions.lock().unwrap().contains_key(&setup_id));
        assert_eq!(
            state
                .pending_verification_urls
                .lock()
                .unwrap()
                .get(&setup_id)
                .map(String::as_str),
            Some("verification-url")
        );
        assert!(!link_task_abort.is_finished());
        assert!(!command_consumer_abort.is_finished());

        let session = state
            .link_sessions
            .lock()
            .unwrap()
            .remove(&setup_id)
            .unwrap();
        session.task.abort();
        session.monitor.abort();
        session.command_consumer.abort();
        std::fs::remove_dir_all(test_dir).unwrap();
    }

    #[tokio::test]
    async fn failed_token_deletion_does_not_block_setup_removal() {
        let test_dir =
            std::env::temp_dir().join(format!("noalbs-connect-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let mut setup = Setup::new("Kept setup".into());
        setup.pairing = Some(connect_core::config::Pairing {
            device_id: uuid::Uuid::new_v4(),
            switcher_id: uuid::Uuid::new_v4(),
            link_url: "ws://127.0.0.1:11000/link".into(),
        });
        let setup_id = setup.id;
        let store = ConfigStore::new(test_dir.join("config.json"));
        store
            .save(&AppConfig {
                setups: vec![setup.clone()],
                ..Default::default()
            })
            .unwrap();
        let tokens = TokenStore::new(test_dir.clone());
        let fallback_path = test_dir.join("tokens").join(setup_id.to_string());
        std::fs::create_dir_all(&fallback_path).unwrap();
        let state = AppState {
            config: Mutex::new(AppConfig {
                setups: vec![setup],
                ..Default::default()
            }),
            startup_error: None,
            store: store.clone(),
            tokens,
            supervisor: Mutex::new(Supervisor::default()),
            link_sessions: Mutex::new(HashMap::new()),
            obs_actions_running: Mutex::new(HashSet::new()),
            pending_verification_urls: Mutex::new(HashMap::from([(
                setup_id,
                "verification-url".into(),
            )])),
        };
        let (state_tx, _) = watch::channel(PcObsState::Starting);
        let (_, effective_obs) = watch::channel(None);
        let link_task = tokio::spawn(std::future::pending::<()>());
        let link_task_abort = link_task.abort_handle();
        let command_consumer = tokio::spawn(std::future::pending::<()>());
        let command_consumer_abort = command_consumer.abort_handle();
        state.link_sessions.lock().unwrap().insert(
            setup_id,
            LinkSession {
                state: state_tx,
                effective_obs,
                task: tauri::async_runtime::JoinHandle::Tokio(link_task),
                monitor: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
                command_consumer: tauri::async_runtime::JoinHandle::Tokio(command_consumer),
            },
        );

        assert!(remove_setup_state(&state, setup_id).unwrap());

        {
            let in_memory_config = state.config.lock().unwrap();
            assert!(in_memory_config.setups.is_empty());
            assert_eq!(in_memory_config.pending_token_deletions, [setup_id]);
        }
        let persisted_config = store.load().unwrap();
        assert!(persisted_config.setups.is_empty());
        assert_eq!(persisted_config.pending_token_deletions, [setup_id]);
        assert!(!state.link_sessions.lock().unwrap().contains_key(&setup_id));
        assert!(fallback_path.is_dir());
        assert!(
            !state
                .pending_verification_urls
                .lock()
                .unwrap()
                .contains_key(&setup_id)
        );
        tokio::task::yield_now().await;
        assert!(link_task_abort.is_finished());
        assert!(command_consumer_abort.is_finished());
        std::fs::remove_dir_all(test_dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn clearing_pairing_falls_back_to_the_running_obs_state() {
        use std::os::unix::fs::PermissionsExt;

        let test_dir =
            std::env::temp_dir().join(format!("noalbs-connect-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let obs_binary = test_dir.join("fake-obs");
        std::fs::write(
            &obs_binary,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf 'OBS Studio 33.0.1\\n'; exit 0; fi\ntrap 'exit 0' TERM\nwhile :; do sleep 0.05; done\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&obs_binary).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&obs_binary, permissions).unwrap();

        let mut setup = Setup::new("Rejected pairing".into());
        setup.obs.install_kind = connect_core::config::InstallKind::CustomBinary;
        setup.obs.binary_path = Some(obs_binary);
        setup.pairing = Some(connect_core::config::Pairing {
            device_id: uuid::Uuid::new_v4(),
            switcher_id: uuid::Uuid::new_v4(),
            link_url: "ws://127.0.0.1:11000/link".into(),
        });
        let setup_id = setup.id;
        let store = ConfigStore::new(test_dir.join("config.json"));
        let tokens = TokenStore::new(test_dir.clone());
        tokens.set(setup_id, "rejected-token").unwrap();
        let state = AppState {
            config: Mutex::new(AppConfig {
                setups: vec![setup.clone()],
                ..Default::default()
            }),
            startup_error: None,
            store,
            tokens,
            supervisor: Mutex::new(Supervisor::with_shutdown_grace(Duration::from_secs(1))),
            link_sessions: Mutex::new(HashMap::new()),
            obs_actions_running: Mutex::new(HashSet::new()),
            pending_verification_urls: Mutex::new(HashMap::new()),
        };
        {
            let mut supervisor = state.supervisor.lock().unwrap();
            supervisor.start(&setup, false, None).unwrap();
            supervisor.mark_connected(setup_id, true);
            assert_eq!(supervisor.state(&setup), PcObsState::Connected);
        }
        let (state_tx, _) = watch::channel(PcObsState::Connected);
        let (_, effective_obs) = watch::channel(None);
        state.link_sessions.lock().unwrap().insert(
            setup_id,
            LinkSession {
                state: state_tx,
                effective_obs,
                task: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
                monitor: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
                command_consumer: tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    std::future::pending::<()>(),
                )),
            },
        );

        clear_pairing_state(&state, setup_id, false).unwrap();
        let mut supervisor = state.supervisor.lock().unwrap();
        assert_eq!(supervisor.state(&setup), PcObsState::Starting);
        assert!(supervisor.owns_running_process(&setup));
        supervisor.stop(&setup).unwrap();
        drop(supervisor);
        assert_eq!(build_ui_state(&state).setups[0].obs_state, "not_running");
        std::fs::remove_dir_all(test_dir).unwrap();
    }
}
