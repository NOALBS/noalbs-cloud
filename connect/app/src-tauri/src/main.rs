use std::{collections::HashMap, sync::Mutex};

use connect_core::{
    AppConfig, ConfigStore, PairingClient, Setup,
    credentials::TokenStore,
    pairing::ConnectOs,
    relay::{LinkConfig, ObsTelemetry, PcObsState, WatchdogMode, run_link, stop_active_streams},
    supervisor::{
        Supervisor, config_dir, detected_obs_binary_for, detected_obs_install_kind,
        enable_obs_websocket, read_obs_websocket_config,
    },
    watchdog::{Watchdog, WatchdogAction},
};
use serde::Serialize;
use std::time::Duration;
use tauri::{
    Emitter, Manager, State,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::watch;

struct LinkSession {
    state: watch::Sender<PcObsState>,
    effective_obs: watch::Receiver<Option<(u16, String)>>,
    task: tauri::async_runtime::JoinHandle<()>,
    monitor: tauri::async_runtime::JoinHandle<()>,
}

struct AppState {
    config: Mutex<AppConfig>,
    startup_error: Option<String>,
    store: ConfigStore,
    tokens: TokenStore,
    supervisor: Mutex<Supervisor>,
    link_sessions: Mutex<HashMap<uuid::Uuid, LinkSession>>,
}

#[derive(Clone, Serialize)]
struct SetupSummary {
    id: String,
    name: String,
    paired: bool,
    switcher_id: Option<String>,
    obs_state: String,
    obs: connect_core::config::ObsLaunchConfig,
    detected_binary: String,
    config_dir: String,
}

#[derive(Clone, Serialize)]
struct UiState {
    start_at_login: bool,
    api_url: String,
    startup_error: Option<String>,
    first_run: bool,
    setups: Vec<SetupSummary>,
}

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> UiState {
    let config = state.config.lock().expect("app config lock");
    let mut supervisor = state.supervisor.lock().expect("supervisor lock");
    let sessions = state.link_sessions.lock().expect("link session lock");
    UiState {
        start_at_login: config.settings.start_at_login,
        api_url: config.settings.api_url.clone(),
        startup_error: state.startup_error.clone(),
        first_run: config.setups.is_empty(),
        setups: config
            .setups
            .iter()
            .map(|setup| SetupSummary {
                id: setup.id.to_string(),
                name: setup.name.clone(),
                paired: setup.pairing.is_some(),
                switcher_id: setup
                    .pairing
                    .as_ref()
                    .map(|pair| pair.switcher_id.to_string()),
                obs_state: serde_json::to_value(
                    sessions
                        .get(&setup.id)
                        .map(|session| *session.state.borrow())
                        .unwrap_or_else(|| supervisor.state(setup)),
                )
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "unknown".into()),
                obs: setup.obs.clone(),
                detected_binary: setup
                    .obs
                    .binary_path
                    .clone()
                    .unwrap_or_else(|| detected_obs_binary_for(&setup.obs.install_kind))
                    .to_string_lossy()
                    .into_owned(),
                config_dir: config_dir(setup).to_string_lossy().into_owned(),
            })
            .collect(),
    }
}

#[tauri::command]
fn save_setup(
    setup_id: String,
    name: String,
    obs: connect_core::config::ObsLaunchConfig,
    state: State<'_, AppState>,
) -> Result<UiState, String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
    let setup = config
        .setups
        .iter_mut()
        .find(|setup| setup.id == id)
        .ok_or("setup not found")?;
    setup.name = if name.trim().is_empty() {
        "OBS".into()
    } else {
        name.trim().to_owned()
    };
    setup.obs = obs;
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    drop(config);
    Ok(get_state(state))
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
    state: State<'_, AppState>,
) -> Result<UiState, String> {
    let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
    config.settings.start_at_login = start_at_login;
    config.settings.api_url = api_url;
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    let autostart = app.autolaunch();
    if start_at_login {
        autostart.enable().map_err(|error| error.to_string())?;
    } else {
        autostart.disable().map_err(|error| error.to_string())?;
    }
    drop(config);
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
    let (api_url, name) = {
        let config = state.config.lock().map_err(|_| "config lock poisoned")?;
        let setup = config
            .setups
            .iter()
            .find(|setup| setup.id.to_string() == setup_id)
            .ok_or("setup not found")?;
        (config.settings.api_url.clone(), setup.name.clone())
    };
    let client = PairingClient::new(api_url);
    let os = if cfg!(target_os = "windows") {
        ConnectOs::Windows
    } else {
        ConnectOs::Linux
    };
    client
        .create(&name, os, env!("CARGO_PKG_VERSION"))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn pair_setup(
    app: tauri::AppHandle,
    setup_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (api_url, name) = {
        let config = state.config.lock().map_err(|_| "config lock poisoned")?;
        let setup = config
            .setups
            .iter()
            .find(|setup| setup.id.to_string() == setup_id)
            .ok_or("setup not found")?;
        (config.settings.api_url.clone(), setup.name.clone())
    };
    let client = PairingClient::new(api_url);
    let os = if cfg!(target_os = "windows") {
        ConnectOs::Windows
    } else {
        ConnectOs::Linux
    };
    let pending = client
        .create(&name, os, env!("CARGO_PKG_VERSION"))
        .await
        .map_err(|error| error.to_string())?;
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
        .await
        .map_err(|error| error.to_string())?;
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
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
fn start_obs(setup_id: String, state: State<'_, AppState>) -> Result<UiState, String> {
    let setup = find_setup(&state, &setup_id)?;
    state
        .supervisor
        .lock()
        .map_err(|_| "supervisor lock poisoned")?
        .start(&setup, false, None)
        .map_err(|error| error.to_string())?;
    set_link_state(state.inner(), setup.id, PcObsState::Starting);
    Ok(get_state(state))
}

#[tauri::command]
async fn stop_obs(setup_id: String, state: State<'_, AppState>) -> Result<UiState, String> {
    let setup = find_setup(&state, &setup_id)?;
    stop_setup_streams(&setup, state.inner()).await;
    state
        .supervisor
        .lock()
        .map_err(|_| "supervisor lock poisoned")?
        .stop(&setup)
        .map_err(|error| error.to_string())?;
    set_link_state(state.inner(), setup.id, PcObsState::NotRunning);
    Ok(get_state(state))
}

#[tauri::command]
async fn restart_obs(setup_id: String, state: State<'_, AppState>) -> Result<UiState, String> {
    let setup = find_setup(&state, &setup_id)?;
    stop_setup_streams(&setup, state.inner()).await;
    let mut supervisor = state
        .supervisor
        .lock()
        .map_err(|_| "supervisor lock poisoned")?;
    if supervisor.state(&setup) != connect_core::relay::PcObsState::NotRunning {
        supervisor.stop(&setup).map_err(|error| error.to_string())?;
    }
    supervisor
        .start(&setup, false, None)
        .map_err(|error| error.to_string())?;
    drop(supervisor);
    set_link_state(state.inner(), setup.id, PcObsState::Starting);
    Ok(get_state(state))
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
    enable_obs_websocket(&config_dir(&setup)).map_err(|error| error.to_string())?;
    drop(supervisor);
    if setup.pairing.is_some() {
        start_link_session(&app, state.inner(), setup)?;
    }
    Ok(get_state(state))
}

#[tauri::command]
fn unpair_setup(setup_id: String, state: State<'_, AppState>) -> Result<UiState, String> {
    let id = uuid::Uuid::parse_str(&setup_id).map_err(|error| error.to_string())?;
    state.tokens.delete(id).map_err(|error| error.to_string())?;
    if let Some(session) = state
        .link_sessions
        .lock()
        .map_err(|_| "link session lock poisoned")?
        .remove(&id)
    {
        session.task.abort();
        session.monitor.abort();
    }
    let mut config = state.config.lock().map_err(|_| "config lock poisoned")?;
    if let Some(setup) = config.setups.iter_mut().find(|setup| setup.id == id) {
        setup.pairing = None;
    }
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    drop(config);
    Ok(get_state(state))
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
    let link_config = LinkConfig {
        link_url: pairing.link_url.clone(),
        device_token: token,
        version: env!("CARGO_PKG_VERSION").into(),
        os: if cfg!(target_os = "windows") {
            "windows".into()
        } else {
            "linux".into()
        },
        obs_port,
        obs_password,
        use_dashboard_obs_settings: setup.obs.use_dashboard_obs_settings,
        state_tx: state_tx.clone(),
        telemetry_tx,
        effective_obs_tx,
    };
    let mut sessions = state
        .link_sessions
        .lock()
        .map_err(|_| "link session lock poisoned")?;
    if let Some(previous) = sessions.remove(&setup.id) {
        previous.task.abort();
        previous.monitor.abort();
    }
    let app_handle = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        if let Err(error) = run_link(link_config, state_rx, watchdog_tx).await {
            let _ = app_handle.emit("connect-error", error.to_string());
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
                    if process_state == PcObsState::NotRunning
                        && state == PcObsState::NotResponding
                    {
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
                            if incident == PcObsState::NotResponding {
                                stop_setup_streams(&monitor_setup, &app_state).await;
                            }
                            let mut supervisor = app_state.supervisor.lock().expect("supervisor lock");
                            if incident == PcObsState::NotResponding
                                && let Err(error) = supervisor.stop(&monitor_setup)
                            {
                                let _ = monitor_handle.emit("connect-error", error.to_string());
                                continue;
                            }
                            match supervisor.start(
                                &monitor_setup,
                                resume,
                                telemetry.last_program_scene.as_deref(),
                            ) {
                                Ok(_) => {
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
    sessions.insert(
        setup.id,
        LinkSession {
            state: state_tx.clone(),
            effective_obs: effective_obs_rx,
            task,
            monitor,
        },
    );
    drop(sessions);

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
    let menu = Menu::new(manager)?;
    let state = manager.state::<AppState>();
    let setups = state.config.lock().expect("app config lock").setups.clone();
    let mut supervisor = state.supervisor.lock().expect("supervisor lock");
    let sessions = state.link_sessions.lock().expect("link session lock");
    for setup in &setups {
        let status = if let Some(session) = sessions.get(&setup.id) {
            *session.state.borrow()
        } else {
            supervisor.state(setup)
        };
        let label = format!("{} — OBS {status:?}", setup.name);
        let item = MenuItem::with_id(
            manager,
            format!("setup-{}", setup.id),
            label,
            false,
            None::<&str>,
        )?;
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

fn main() {
    let store = ConfigStore::new(ConfigStore::default_path().expect("user config path"));
    let (config, startup_error) = store.load_or_backup();
    let tokens = TokenStore::default_store().expect("token store path");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--minimized"])
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            config: Mutex::new(config),
            startup_error,
            store,
            tokens,
            supervisor: Mutex::new(Supervisor::default()),
            link_sessions: Mutex::new(HashMap::new()),
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
            if state
                .config
                .lock()
                .expect("app config lock")
                .settings
                .start_at_login
            {
                let _ = app.autolaunch().enable();
            }
            let setups = state.config.lock().expect("app config lock").setups.clone();
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            add_setup,
            save_settings,
            save_setup,
            create_pairing,
            pair_setup,
            start_obs,
            stop_obs,
            restart_obs,
            enable_websocket,
            unpair_setup
        ])
        .run(tauri::generate_context!())
        .expect("failed to run NOALBS Cloud Connect");
}
