use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Instant,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    config::{InstallKind, ObsLaunchConfig, Setup},
    relay::PcObsState,
};

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("OBS process I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("OBS websocket is disabled in its configuration")]
    WebsocketDisabled,
    #[error("OBS process is not identified; refusing to control it")]
    Unidentified,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObsWebsocketConfig {
    #[serde(default)]
    pub server_enabled: bool,
    #[serde(default = "default_obs_port")]
    pub server_port: u16,
    #[serde(default)]
    pub auth_required: bool,
    #[serde(default)]
    pub server_password: String,
}

fn default_obs_port() -> u16 {
    4455
}

pub fn read_obs_websocket_config(config_dir: &Path) -> Result<ObsWebsocketConfig, SupervisorError> {
    let path = config_dir.join("plugin_config/obs-websocket/config.json");
    let data = std::fs::read(path)?;
    serde_json::from_slice(&data)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error).into())
}

pub fn enable_obs_websocket(config_dir: &Path) -> Result<String, SupervisorError> {
    enable_obs_websocket_on_port(config_dir, None)
}

pub fn enable_obs_websocket_on_port(
    config_dir: &Path,
    websocket_port: Option<u16>,
) -> Result<String, SupervisorError> {
    let path = config_dir.join("plugin_config/obs-websocket/config.json");
    let mut config: Value = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            serde_json::json!({
                "alerts_enabled": false,
                "auth_required": true,
                "first_load": false,
                "server_enabled": true,
                "server_password": "",
                "server_port": 4455
            })
        }
        Err(error) => return Err(error.into()),
    };
    if !config.is_object() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "OBS websocket configuration must be a JSON object",
        )
        .into());
    }
    fs::create_dir_all(path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "websocket config has no parent",
        )
    })?)?;
    let password = Uuid::new_v4().simple().to_string();
    config["alerts_enabled"] = config
        .get("alerts_enabled")
        .cloned()
        .unwrap_or(Value::Bool(false));
    config["server_enabled"] = Value::Bool(true);
    config["auth_required"] = Value::Bool(true);
    config["first_load"] = Value::Bool(false);
    config["server_password"] = Value::String(password.clone());
    config["server_port"] = Value::from(websocket_port.unwrap_or(4455));
    let bytes = serde_json::to_vec_pretty(&config)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let temp = path.with_file_name(format!("config.json.tmp-{}", Uuid::new_v4()));
    let write_result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &path)?;
        Ok::<(), std::io::Error>(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result?;
    Ok(password)
}

pub fn config_dir(setup: &Setup) -> PathBuf {
    if let Some(path) = &setup.obs.config_dir_override {
        return path.clone();
    }
    match setup.obs.install_kind {
        InstallKind::Flatpak => directories::BaseDirs::new()
            .map(|dirs| {
                dirs.home_dir()
                    .join(".var/app/com.obsproject.Studio/config/obs-studio")
            })
            .unwrap_or_else(|| {
                dirs_config_home()
                    .join("..")
                    .join(".var/app/com.obsproject.Studio/config/obs-studio")
            }),
        _ => dirs_config_home().join("obs-studio"),
    }
}

fn dirs_config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|dirs| dirs.config_dir().to_owned())
                .unwrap_or_default()
        })
}

pub fn launch_arguments(
    config: &ObsLaunchConfig,
    multi: bool,
    resume: bool,
    scene: Option<&str>,
) -> Vec<String> {
    let mut args = vec!["--disable-missing-files-check".to_owned()];
    if multi {
        args.push("--multi".into());
    }
    if config.disable_updater {
        args.push("--disable-updater".into());
    }
    if config
        .last_obs_version
        .as_deref()
        .and_then(|version| version.split('.').next())
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major < 32)
    {
        args.push("--disable-shutdown-check".into());
    }
    if config.minimize_to_tray {
        args.push("--minimize-to-tray".into());
    }
    if let Some(value) = &config.profile {
        args.extend(["--profile".into(), value.clone()]);
    }
    if let Some(value) = &config.collection {
        args.extend(["--collection".into(), value.clone()]);
    }
    if let Some(value) = scene.or(config.scene.as_deref()) {
        args.extend(["--scene".into(), value.to_owned()]);
    }
    if resume {
        args.push("--startstreaming".into());
    }
    if config.shared_config {
        if let Some(port) = config.websocket_port {
            args.extend(["--websocket_port".into(), port.to_string()]);
        }
        if let Some(password) = &config.websocket_password_override {
            args.extend(["--websocket_password".into(), password.clone()]);
        }
    }
    args
}

#[derive(Debug)]
pub struct StartedObs {
    pub setup_id: Uuid,
    setup: Setup,
    pub pid: u32,
    pub child: Child,
    pub started: Instant,
    process_start_time: u64,
    multi: bool,
    config_dir: PathBuf,
}

pub struct Supervisor {
    children: HashMap<Uuid, StartedObs>,
    adopted: HashMap<Uuid, (Setup, u32, u64)>,
    connected: HashSet<Uuid>,
    crashed_pending: HashSet<Uuid>,
    shutdown_grace: std::time::Duration,
    waiting_for_input_after: std::time::Duration,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::with_shutdown_grace(std::time::Duration::from_secs(20))
    }
}

impl Supervisor {
    pub fn with_shutdown_grace(shutdown_grace: std::time::Duration) -> Self {
        Self {
            children: HashMap::new(),
            adopted: HashMap::new(),
            connected: HashSet::new(),
            crashed_pending: HashSet::new(),
            shutdown_grace,
            waiting_for_input_after: std::time::Duration::from_secs(60),
        }
    }

    pub fn with_timing(
        shutdown_grace: std::time::Duration,
        waiting_for_input_after: std::time::Duration,
    ) -> Self {
        Self {
            waiting_for_input_after,
            ..Self::with_shutdown_grace(shutdown_grace)
        }
    }

    pub fn start(
        &mut self,
        setup: &Setup,
        resume: bool,
        last_scene: Option<&str>,
    ) -> Result<u32, SupervisorError> {
        if let Some(item) = self.children.get_mut(&setup.id) {
            match item.child.try_wait() {
                Ok(None) => return Ok(item.pid),
                Ok(Some(status)) => {
                    if process_exited_unexpectedly(setup, item, &status) {
                        self.crashed_pending.insert(setup.id);
                    }
                }
                Err(_) => return Err(SupervisorError::Unidentified),
            }
        }
        self.children.remove(&setup.id);
        self.adopted
            .retain(|_, (_, pid, start_time)| process_start_time(*pid) == Some(*start_time));
        let sibling_identity = self.children.iter_mut().any(|(id, child)| {
            *id != setup.id
                && same_process_identity(setup, &child.setup)
                && child.child.try_wait().ok().flatten().is_none()
        }) || self
            .adopted
            .iter()
            .any(|(id, (other, _, _))| *id != setup.id && same_process_identity(setup, other));
        let existing = matching_obs_pids(setup);
        if !sibling_identity {
            if existing.len() == 1 {
                let pid = existing[0].as_u32();
                let start_time = process_start_time(pid).ok_or(SupervisorError::Unidentified)?;
                self.adopted
                    .insert(setup.id, (setup.clone(), pid, start_time));
                return Ok(pid);
            }
            if existing.len() > 1 {
                return Err(SupervisorError::Unidentified);
            }
        }
        let executable = setup
            .obs
            .binary_path
            .clone()
            .unwrap_or_else(default_obs_binary);
        let working_directory = executable.parent().map(Path::to_owned);
        let another_child_uses_config = self.children.iter_mut().any(|(id, child)| {
            *id != setup.id
                && child.config_dir == config_dir(setup)
                && child.child.try_wait().ok().flatten().is_none()
        });
        if self.crashed_pending.remove(&setup.id) || has_run_sentinel(setup) {
            prepare_after_crash(setup, another_child_uses_config)?;
        }
        let multi = any_obs_running()
            || self.children.iter_mut().any(|(id, child)| {
                *id != setup.id && child.child.try_wait().ok().flatten().is_none()
            });
        let args = launch_arguments(&setup.obs, multi, resume, last_scene);
        let mut command = if matches!(setup.obs.install_kind, InstallKind::Flatpak) {
            let mut cmd = Command::new("flatpak");
            cmd.arg("run");
            if let Some(config) = &setup.obs.config_dir_override {
                cmd.arg(format!(
                    "--env=XDG_CONFIG_HOME={}",
                    config.parent().unwrap_or(config).display()
                ));
            }
            cmd.arg("com.obsproject.Studio");
            cmd
        } else {
            Command::new(executable)
        };
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(path) = working_directory {
            command.current_dir(path);
        }
        // TODO: verify separate XDG_CONFIG_HOME behavior for native and Flathub OBS (§6.7).
        if cfg!(target_os = "linux")
            && let Some(config) = &setup.obs.config_dir_override
        {
            command.env("XDG_CONFIG_HOME", config.parent().unwrap_or(config));
        }
        let child = command.spawn()?;
        let pid = child.id();
        let process_start_time = wait_for_process_start_time(pid).unwrap_or_default();
        self.children.insert(
            setup.id,
            StartedObs {
                setup_id: setup.id,
                setup: setup.clone(),
                pid,
                child,
                started: Instant::now(),
                process_start_time,
                multi,
                config_dir: config_dir(setup),
            },
        );
        self.connected.remove(&setup.id);
        Ok(pid)
    }

    pub fn state(&mut self, setup: &Setup) -> PcObsState {
        if let Some(child) = self.children.get_mut(&setup.id) {
            match child.child.try_wait() {
                Ok(None) => {
                    if self.connected.contains(&setup.id) {
                        return PcObsState::Connected;
                    }
                    return if child.started.elapsed() >= self.waiting_for_input_after {
                        PcObsState::WaitingForInput
                    } else {
                        PcObsState::Starting
                    };
                }
                Ok(Some(status)) => {
                    let crashed = process_exited_unexpectedly(setup, child, &status);
                    self.children.remove(&setup.id);
                    self.connected.remove(&setup.id);
                    if crashed {
                        self.crashed_pending.insert(setup.id);
                    }
                    return if crashed {
                        PcObsState::Crashed
                    } else {
                        PcObsState::NotRunning
                    };
                }
                Err(_) => return PcObsState::NotResponding,
            }
        }
        match matching_obs_pids(setup).as_slice() {
            [_] => PcObsState::WaitingForInput,
            [] if executable_obs_pids(setup).is_empty() => PcObsState::NotRunning,
            _ => PcObsState::Unknown,
        }
    }

    pub fn mark_connected(&mut self, setup_id: Uuid, connected: bool) {
        if connected {
            self.connected.insert(setup_id);
        } else {
            self.connected.remove(&setup_id);
        }
    }

    pub fn stop(&mut self, setup: &Setup) -> Result<(), SupervisorError> {
        let (pid, expected_start_time, mut child) = match self.children.remove(&setup.id) {
            Some(item) => (item.pid, item.process_start_time, Some(item.child)),
            None => {
                let Some((pid, start_time)) = find_matching_obs_with_start_time(setup) else {
                    return Err(SupervisorError::Unidentified);
                };
                (pid.as_u32(), start_time, None)
            }
        };
        self.connected.remove(&setup.id);
        if process_start_time(pid) == Some(expected_start_time) {
            request_graceful_close(pid);
        }
        let deadline = Instant::now() + self.shutdown_grace;
        loop {
            let running = if let Some(child) = child.as_mut() {
                child.try_wait()?.is_none()
            } else {
                process_is_running(setup, pid, expected_start_time)
            };
            if !running {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if let Some(child) = child.as_mut() {
            child.kill()?;
            let _ = child.wait()?;
            self.crashed_pending.insert(setup.id);
        } else if process_is_running(setup, pid, expected_start_time) {
            force_kill(pid);
            self.crashed_pending.insert(setup.id);
        }
        self.adopted.remove(&setup.id);
        Ok(())
    }

    pub fn has_child(&self, setup_id: Uuid) -> bool {
        self.children.contains_key(&setup_id)
    }
}

pub fn find_matching_obs(setup: &Setup) -> Option<Pid> {
    let matches = matching_obs_pids(setup);
    (matches.len() == 1).then(|| matches[0])
}

pub fn same_obs_identity(left: &Setup, right: &Setup) -> bool {
    let binary = |setup: &Setup| {
        resolve_executable(
            setup
                .obs
                .binary_path
                .as_deref()
                .unwrap_or(&detected_obs_binary_for(&setup.obs.install_kind)),
        )
    };
    same_path(&binary(left), &binary(right)) && config_dir(left) == config_dir(right)
}

fn same_process_identity(left: &Setup, right: &Setup) -> bool {
    same_obs_identity(left, right)
        && (!(left.obs.shared_config && right.obs.shared_config)
            || left.obs.websocket_port == right.obs.websocket_port)
}

fn find_matching_obs_with_start_time(setup: &Setup) -> Option<(Pid, u64)> {
    let matches = matching_obs_pids(setup);
    if matches.len() != 1 {
        return None;
    }
    let pid = matches[0];
    Some((pid, process_start_time(pid.as_u32())?))
}

fn matching_obs_pids(setup: &Setup) -> Vec<Pid> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    let expected = setup
        .obs
        .binary_path
        .clone()
        .unwrap_or_else(default_obs_binary);
    let expected = resolve_executable(&expected);
    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let executable_matches = if matches!(setup.obs.install_kind, InstallKind::Flatpak) {
                process.exe().is_some_and(|exe| {
                    exe.file_name().is_some_and(|name| name == "flatpak")
                        && process
                            .cmd()
                            .iter()
                            .any(|arg| arg == "com.obsproject.Studio")
                })
            } else {
                process.exe().is_some_and(|exe| same_path(exe, &expected))
            };
            let config_matches = !cfg!(target_os = "linux")
                || setup.obs.config_dir_override.as_ref().is_none_or(|config| {
                    let expected = format!(
                        "XDG_CONFIG_HOME={}",
                        config.parent().unwrap_or(config).display()
                    );
                    process
                        .environ()
                        .iter()
                        .any(|entry| entry.to_string_lossy() == expected)
                        || (matches!(setup.obs.install_kind, InstallKind::Flatpak)
                            && process.cmd().iter().any(|argument| {
                                argument.to_string_lossy() == format!("--env={expected}")
                            }))
                });
            let port_matches = !setup.obs.shared_config
                || setup.obs.websocket_port.is_some_and(|port| {
                    command_has_option(process.cmd(), "--websocket_port", &port.to_string())
                });
            (executable_matches && config_matches && port_matches).then_some(*pid)
        })
        .collect()
}

fn command_has_option(arguments: &[std::ffi::OsString], option: &str, value: &str) -> bool {
    arguments
        .windows(2)
        .any(|pair| pair[0].to_string_lossy() == option && pair[1].to_string_lossy() == value)
        || arguments
            .iter()
            .any(|argument| argument.to_string_lossy() == format!("{option}={value}"))
}

fn executable_obs_pids(setup: &Setup) -> Vec<Pid> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    let expected = resolve_executable(
        setup
            .obs
            .binary_path
            .as_deref()
            .unwrap_or(&default_obs_binary()),
    );
    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let executable_matches = if matches!(setup.obs.install_kind, InstallKind::Flatpak) {
                process.exe().is_some_and(|exe| {
                    exe.file_name().is_some_and(|name| name == "flatpak")
                        && process
                            .cmd()
                            .iter()
                            .any(|argument| argument == "com.obsproject.Studio")
                })
            } else {
                process.exe().is_some_and(|exe| same_path(exe, &expected))
            };
            executable_matches.then_some(*pid)
        })
        .collect()
}

fn resolve_executable(path: &Path) -> PathBuf {
    if path.is_absolute() || path.components().count() > 1 {
        return path.to_owned();
    }
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join(path))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| path.to_owned())
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn any_obs_running() -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    system.processes().values().any(|process| {
        process.exe().is_some_and(|exe| {
            let name = exe.file_name().unwrap_or_default().to_string_lossy();
            if cfg!(target_os = "windows") {
                name.eq_ignore_ascii_case("obs64.exe")
            } else {
                name == "obs"
                    || (name == "flatpak"
                        && process
                            .cmd()
                            .iter()
                            .any(|arg| arg == "com.obsproject.Studio"))
            }
        })
    })
}

#[cfg(unix)]
fn request_graceful_close(pid: u32) {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
}

fn process_start_time(pid: u32) -> Option<u64> {
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::everything(),
    );
    system.process(pid).map(|process| process.start_time())
}

fn wait_for_process_start_time(pid: u32) -> Option<u64> {
    for _ in 0..50 {
        if let Some(start_time) = process_start_time(pid) {
            return Some(start_time);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

fn process_is_running(setup: &Setup, pid: u32, start_time: u64) -> bool {
    matching_obs_pids(setup)
        .iter()
        .any(|candidate| candidate.as_u32() == pid && process_start_time(pid) == Some(start_time))
}

fn sentinel_dir(setup: &Setup) -> PathBuf {
    config_dir(setup).join(".sentinel")
}

fn has_run_sentinel(setup: &Setup) -> bool {
    fs::read_dir(sentinel_dir(setup))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with("run_"))
}

fn prepare_after_crash(
    setup: &Setup,
    another_child_uses_config: bool,
) -> Result<(), SupervisorError> {
    let directory = sentinel_dir(setup);
    if another_child_uses_config || !matching_obs_pids(setup).is_empty() {
        return Ok(());
    }

    copy_latest_crash_report(setup)?;
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("run_") {
            let metadata = entry.file_type()?;
            if metadata.is_file() {
                fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}

fn copy_latest_crash_report(setup: &Setup) -> Result<(), SupervisorError> {
    let crash_dir = config_dir(setup).join("crashes");
    let mut reports = fs::read_dir(crash_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata
                .is_file()
                .then(|| (entry.path(), metadata.modified().ok()))
        })
        .collect::<Vec<_>>();
    reports.sort_by_key(|(_, modified)| *modified);
    let Some((latest, _)) = reports.pop() else {
        return Ok(());
    };

    let destination = config_dir(setup)
        .join("noalbs-connect/crash-reports")
        .join(setup.id.to_string());
    fs::create_dir_all(&destination)?;
    let file_name = latest
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    fs::copy(&latest, destination.join(file_name))?;

    let mut saved = fs::read_dir(&destination)?
        .flatten()
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((entry.path(), modified))
        })
        .collect::<Vec<_>>();
    saved.sort_by_key(|(_, modified)| *modified);
    while saved.len() > 5 {
        let (oldest, _) = saved.remove(0);
        fs::remove_file(oldest)?;
    }
    Ok(())
}

fn process_exited_unexpectedly(
    setup: &Setup,
    child: &StartedObs,
    status: &std::process::ExitStatus,
) -> bool {
    if !child.multi && sentinel_dir(setup).is_dir() {
        has_run_sentinel(setup)
    } else {
        !status.success()
    }
}

#[cfg(windows)]
fn request_graceful_close(pid: u32) {
    use windows_sys::Win32::{
        Foundation::{BOOL, HWND, LPARAM},
        UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE},
    };

    unsafe extern "system" fn close_process_windows(hwnd: HWND, target: LPARAM) -> BOOL {
        let target_pid = unsafe { *(target as *const u32) };
        let mut process_id = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut process_id);
            if process_id == target_pid {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
        1
    }

    unsafe {
        EnumWindows(Some(close_process_windows), (&pid as *const u32) as LPARAM);
    }
}

#[cfg(unix)]
fn force_kill(pid: u32) {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
}

#[cfg(windows)]
fn force_kill(pid: u32) {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            let _ = TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

#[cfg(windows)]
fn default_obs_binary() -> PathBuf {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ},
    };

    let registry_path = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let Ok(uninstall) = RegKey::predef(root).open_subkey_with_flags(registry_path, KEY_READ)
        else {
            continue;
        };
        for name in ["OBS Studio", "OBS Studio_is1"] {
            if let Ok(key) = uninstall.open_subkey_with_flags(name, KEY_READ)
                && let Ok(location) = key.get_value::<String, _>("InstallLocation")
            {
                let path = PathBuf::from(location).join("bin/64bit/obs64.exe");
                if path.is_file() {
                    return path;
                }
            }
        }
    }
    PathBuf::from(r"C:\Program Files\obs-studio\bin\64bit\obs64.exe")
}

#[cfg(not(windows))]
fn default_obs_binary() -> PathBuf {
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("obs"))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("obs"))
}

pub fn detected_obs_binary() -> PathBuf {
    default_obs_binary()
}

pub fn detected_obs_binary_for(install_kind: &InstallKind) -> PathBuf {
    if matches!(install_kind, InstallKind::Flatpak) {
        resolve_executable(Path::new("flatpak"))
    } else {
        default_obs_binary()
    }
}

pub fn detected_obs_install_kind() -> InstallKind {
    #[cfg(target_os = "linux")]
    {
        if default_obs_binary().is_file() {
            return InstallKind::Native;
        }
        if Command::new("flatpak")
            .args(["info", "com.obsproject.Studio"])
            .status()
            .is_ok_and(|status| status.success())
        {
            return InstallKind::Flatpak;
        }
    }
    InstallKind::Native
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::Path,
        process::Command,
        thread,
        time::{Duration, UNIX_EPOCH},
    };

    use tempfile::tempdir;

    use super::{
        Supervisor, SupervisorError, copy_latest_crash_report, find_matching_obs_with_start_time,
        launch_arguments, matching_obs_pids, prepare_after_crash, process_start_time,
        resolve_executable,
    };
    use crate::config::{InstallKind, ObsLaunchConfig, Setup};

    fn make_setup(script: &Path, config_dir: &Path) -> Setup {
        let mut setup = Setup::new("Fake OBS".into());
        setup.obs.install_kind = InstallKind::CustomBinary;
        setup.obs.binary_path = Some(script.to_owned());
        setup.obs.config_dir_override = Some(config_dir.to_owned());
        setup
    }

    fn make_manual_setup(binary: &Path, config_dir: &Path) -> Setup {
        let mut setup = Setup::new("Manual OBS".into());
        setup.obs.install_kind = InstallKind::CustomBinary;
        setup.obs.binary_path = Some(binary.to_owned());
        setup.obs.config_dir_override = Some(config_dir.to_owned());
        setup
    }

    fn make_script(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn wait_for_path(path: &Path) {
        for _ in 0..200 {
            if path.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {}", path.display());
    }

    #[test]
    fn shared_config_and_resume_options_are_scoped_to_the_launch() {
        let config = ObsLaunchConfig {
            shared_config: true,
            websocket_port: Some(4456),
            websocket_password_override: Some("test-password".into()),
            ..Default::default()
        };
        let args = launch_arguments(&config, true, true, Some("BRB"));
        assert!(args.contains(&"--multi".into()));
        assert!(args.contains(&"--startstreaming".into()));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--websocket_port", "4456"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--websocket_password", "test-password"])
        );
        assert!(args.windows(2).any(|pair| pair == ["--scene", "BRB"]));

        let non_shared = launch_arguments(&ObsLaunchConfig::default(), false, false, None);
        assert!(!non_shared.iter().any(|argument| {
            matches!(
                argument.as_str(),
                "--websocket_port" | "--websocket_password"
            )
        }));
    }

    #[test]
    fn enabling_websocket_creates_first_launch_config_without_password_regeneration() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");

        let password = super::enable_obs_websocket_on_port(&config_dir, Some(4458)).unwrap();
        let path = config_dir.join("plugin_config/obs-websocket/config.json");
        let config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

        assert_eq!(config["alerts_enabled"], false);
        assert_eq!(config["auth_required"], true);
        assert_eq!(config["first_load"], false);
        assert_eq!(config["server_enabled"], true);
        assert_eq!(config["server_password"], password);
        assert_eq!(config["server_port"], 4458);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn shared_config_process_matching_uses_its_websocket_port() {
        let directory = tempdir().unwrap();
        let xdg_config = directory.path().join("xdg");
        let config_dir = xdg_config.join("obs-studio");
        let binary = resolve_executable(Path::new("sh"));
        let mut setup_x = make_manual_setup(&binary, &config_dir);
        setup_x.obs.shared_config = true;
        setup_x.obs.websocket_port = Some(4456);
        let mut setup_y = setup_x.clone();
        setup_y.id = uuid::Uuid::new_v4();
        setup_y.obs.websocket_port = Some(4457);
        let mut process = Command::new(&binary)
            .args([
                "-c",
                "while :; do sleep 0.05; done",
                "fake-obs",
                "--websocket_port",
                "4456",
            ])
            .env("XDG_CONFIG_HOME", &xdg_config)
            .spawn()
            .unwrap();

        let mut matches = Vec::new();
        for _ in 0..100 {
            matches = matching_obs_pids(&setup_x);
            if !matches.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].as_u32(), process.id());
        let (_, start_time) = find_matching_obs_with_start_time(&setup_x).unwrap();
        assert_eq!(process_start_time(process.id()), Some(start_time));
        assert!(matching_obs_pids(&setup_y).is_empty());

        process.kill().unwrap();
        let _ = process.wait().unwrap();
    }

    #[test]
    fn launch_always_disables_missing_files_check_and_uses_legacy_shutdown_flag() {
        let config = ObsLaunchConfig {
            last_obs_version: Some("31.1.0".into()),
            disable_updater: true,
            minimize_to_tray: true,
            profile: Some("Live".into()),
            collection: Some("Main".into()),
            ..Default::default()
        };
        let args = launch_arguments(&config, false, false, None);
        assert!(args.contains(&"--disable-missing-files-check".into()));
        assert!(args.contains(&"--disable-shutdown-check".into()));
        assert!(args.contains(&"--disable-updater".into()));
        assert!(args.contains(&"--minimize-to-tray".into()));
        assert!(args.windows(2).any(|pair| pair == ["--profile", "Live"]));
        assert!(args.windows(2).any(|pair| pair == ["--collection", "Main"]));
    }

    #[test]
    fn clean_stop_removes_sentinel_and_unconnected_child_times_out() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let script = directory.path().join("fake-obs");
        let sentinel = config_dir.join(".sentinel");
        fs::create_dir_all(&sentinel).unwrap();
        make_script(
            &script,
            &format!(
                "mkdir -p '{}'; touch '{}/run_$$'; trap 'rm -f {}/run_*; exit 0' TERM; while :; do sleep 0.05; done",
                sentinel.display(),
                sentinel.display(),
                sentinel.display()
            ),
        );
        let setup = make_setup(&script, &config_dir);
        let mut supervisor = Supervisor::with_timing(Duration::from_secs(1), Duration::ZERO);
        let pid = supervisor.start(&setup, false, None).unwrap();
        assert!(pid > 0);
        for _ in 0..200 {
            if fs::read_dir(&sentinel)
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("run_"))
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            supervisor.state(&setup),
            crate::relay::PcObsState::WaitingForInput
        );
        supervisor.stop(&setup).unwrap();
        assert!(
            !fs::read_dir(&sentinel)
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("run_"))
        );
    }

    #[test]
    fn force_kill_cleans_stale_sentinel_before_next_start() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let script = directory.path().join("fake-obs");
        let sentinel = config_dir.join(".sentinel");
        let marker = directory.path().join("stale-sentinel-seen");
        fs::create_dir_all(&sentinel).unwrap();
        make_script(
            &script,
            &format!(
                "for f in '{}/run_'*; do if [ -e \"$f\" ]; then touch '{}'; fi; done; touch '{}/run_$$'; trap '' TERM; while :; do sleep 0.05; done",
                sentinel.display(),
                marker.display(),
                sentinel.display()
            ),
        );
        let setup = make_setup(&script, &config_dir);
        let mut supervisor = Supervisor::with_shutdown_grace(Duration::from_millis(100));
        supervisor.start(&setup, false, None).unwrap();
        for _ in 0..200 {
            if fs::read_dir(&sentinel)
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("run_"))
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        supervisor.stop(&setup).unwrap();
        assert!(
            fs::read_dir(&sentinel)
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("run_"))
        );
        supervisor.start(&setup, false, None).unwrap();
        thread::sleep(Duration::from_millis(100));
        assert!(!marker.exists());
        supervisor.stop(&setup).unwrap();
    }

    #[test]
    fn same_identity_setups_start_separate_processes_and_stop_isolated() {
        let directory = tempdir().unwrap();
        let script = directory.path().join("fake-obs");
        let args_path = directory.path().join("arguments");
        make_script(
            &script,
            &format!(
                "printf '%s\\n' \"$@\" > '{}'.$$; trap 'exit 0' TERM; while :; do sleep 0.05; done",
                args_path.display()
            ),
        );
        let setup_a = make_setup(&script, &directory.path().join("config-a"));
        let setup_b = make_setup(&script, &directory.path().join("config-a"));
        let mut supervisor = Supervisor::default();
        let pid_a = supervisor.start(&setup_a, false, None).unwrap();
        let pid_b = supervisor.start(&setup_b, false, None).unwrap();
        assert_ne!(pid_a, pid_b);
        assert!(
            supervisor
                .children
                .get(&setup_a.id)
                .unwrap()
                .process_start_time
                > 0
        );
        let args_for_b = directory.path().join(format!("arguments.{pid_b}"));
        wait_for_path(&args_for_b);
        assert!(
            fs::read_to_string(args_for_b)
                .unwrap()
                .lines()
                .any(|line| line == "--multi")
        );
        supervisor.stop(&setup_a).unwrap();
        let restarted_pid_a = supervisor.start(&setup_a, false, None).unwrap();
        assert_ne!(pid_a, restarted_pid_a);
        assert!(supervisor.has_child(setup_b.id));
        assert!(
            supervisor
                .children
                .get_mut(&setup_b.id)
                .unwrap()
                .child
                .try_wait()
                .unwrap()
                .is_none()
        );
        supervisor.stop(&setup_b).unwrap();
    }

    #[test]
    fn hand_started_obs_matches_executable_and_xdg_config() {
        let directory = tempdir().unwrap();
        let xdg_config = directory.path().join("xdg");
        let config_dir = xdg_config.join("obs-studio");
        let binary = resolve_executable(Path::new("sleep"));
        assert!(binary.is_file());
        let setup = make_manual_setup(&binary, &config_dir);
        let mut manual_obs = Command::new(&binary)
            .arg("30")
            .env("XDG_CONFIG_HOME", &xdg_config)
            .spawn()
            .unwrap();
        let mut supervisor = Supervisor::default();
        let pid = manual_obs.id();
        assert_eq!(supervisor.start(&setup, false, None).unwrap(), pid);
        assert_eq!(
            supervisor.state(&setup),
            crate::relay::PcObsState::WaitingForInput
        );
        supervisor.stop(&setup).unwrap();
        assert!(!manual_obs.wait().unwrap().success());
    }

    #[test]
    fn ambiguous_hand_started_obs_is_reported_and_never_controlled() {
        let directory = tempdir().unwrap();
        let xdg_config = directory.path().join("xdg");
        let config_dir = xdg_config.join("obs-studio");
        let binary = resolve_executable(Path::new("sleep"));
        assert!(binary.is_file());
        let setup = make_manual_setup(&binary, &config_dir);
        let mut first = Command::new(&binary)
            .arg("30")
            .env("XDG_CONFIG_HOME", &xdg_config)
            .spawn()
            .unwrap();
        let mut second = Command::new(&binary)
            .arg("30")
            .env("XDG_CONFIG_HOME", &xdg_config)
            .spawn()
            .unwrap();
        for _ in 0..100 {
            if matching_obs_pids(&setup).len() == 2 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let mut supervisor = Supervisor::default();
        assert_eq!(supervisor.state(&setup), crate::relay::PcObsState::Unknown);
        assert!(matches!(
            supervisor.start(&setup, false, None),
            Err(SupervisorError::Unidentified)
        ));
        assert!(matches!(
            supervisor.stop(&setup),
            Err(SupervisorError::Unidentified)
        ));
        assert!(first.try_wait().unwrap().is_none());
        assert!(second.try_wait().unwrap().is_none());
        first.kill().unwrap();
        second.kill().unwrap();
        let _ = first.wait().unwrap();
        let _ = second.wait().unwrap();
    }

    #[test]
    fn another_setup_using_the_same_config_keeps_existing_sentinels() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let script = directory.path().join("fake-obs");
        let sentinel = config_dir.join(".sentinel");
        let marker = directory.path().join("sentinel-was-present");
        make_script(
            &script,
            &format!(
                "mkdir -p '{}'; for f in '{}/run_'*; do if [ -e \"$f\" ]; then touch '{}'; fi; done; touch '{}/run_$$'; trap 'rm -f {}/run_*; exit 0' TERM; while :; do sleep 0.05; done",
                sentinel.display(),
                sentinel.display(),
                marker.display(),
                sentinel.display(),
                sentinel.display()
            ),
        );
        let setup_a = make_setup(&script, &config_dir);
        let setup_b = make_setup(&script, &config_dir);
        let mut supervisor = Supervisor::default();
        supervisor.start(&setup_a, false, None).unwrap();
        wait_for_path(&sentinel);
        supervisor.start(&setup_b, false, None).unwrap();
        wait_for_path(&marker);
        assert!(
            sentinel
                .read_dir()
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("run_"))
        );
        supervisor.stop(&setup_a).unwrap();
        supervisor.stop(&setup_b).unwrap();
    }

    fn make_crash_then_run_script(script: &Path, config_dir: &Path, args_path: &Path) {
        let sentinel = config_dir.join(".sentinel");
        let count = config_dir.join("starts");
        make_script(
            script,
            &format!(
                "mkdir -p '{}'; printf '%s\\n' \"$@\" > '{}'; n=0; [ ! -f '{}' ] || n=$(cat '{}'); n=$((n+1)); echo \"$n\" > '{}'; if [ \"$n\" -eq 1 ]; then touch '{}/run_crash'; exit 1; fi; trap 'rm -f {}/run_*; exit 0' TERM; while :; do sleep 0.05; done",
                config_dir.display(),
                args_path.display(),
                count.display(),
                count.display(),
                count.display(),
                sentinel.display(),
                sentinel.display()
            ),
        );
    }

    #[test]
    fn crash_restart_uses_restart_mode_without_streaming() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let script = directory.path().join("fake-obs");
        let args = directory.path().join("arguments");
        make_crash_then_run_script(&script, &config_dir, &args);
        let setup = make_setup(&script, &config_dir);
        let mut supervisor = Supervisor::default();
        supervisor.start(&setup, false, None).unwrap();
        let mut crash_state = crate::relay::PcObsState::Starting;
        for _ in 0..200 {
            crash_state = supervisor.state(&setup);
            if crash_state == crate::relay::PcObsState::Crashed {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(crash_state, crate::relay::PcObsState::Crashed);
        let action = crate::watchdog::Watchdog::default().decide(
            Duration::ZERO,
            crate::relay::PcObsState::Crashed,
            crate::relay::WatchdogMode::Restart,
            true,
            false,
        );
        assert_eq!(
            action,
            crate::watchdog::WatchdogAction::Restart { resume: false }
        );
        fs::remove_file(&args).unwrap();
        supervisor.start(&setup, false, None).unwrap();
        wait_for_path(&args);
        assert!(
            !fs::read_to_string(args)
                .unwrap()
                .lines()
                .any(|argument| argument == "--startstreaming")
        );
        supervisor.stop(&setup).unwrap();
    }

    #[test]
    fn restart_resume_passes_the_tracked_scene_only_for_active_streams() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let script = directory.path().join("fake-obs");
        let args = directory.path().join("arguments");
        make_crash_then_run_script(&script, &config_dir, &args);
        let setup = make_setup(&script, &config_dir);
        let mut supervisor = Supervisor::default();
        supervisor.start(&setup, false, None).unwrap();
        let mut crash_state = crate::relay::PcObsState::Starting;
        for _ in 0..200 {
            crash_state = supervisor.state(&setup);
            if crash_state == crate::relay::PcObsState::Crashed {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(crash_state, crate::relay::PcObsState::Crashed);
        let action = crate::watchdog::Watchdog::default().decide(
            Duration::ZERO,
            crate::relay::PcObsState::Crashed,
            crate::relay::WatchdogMode::RestartResume,
            true,
            false,
        );
        assert_eq!(
            action,
            crate::watchdog::WatchdogAction::Restart { resume: true }
        );
        fs::remove_file(&args).unwrap();
        supervisor.start(&setup, true, Some("Program")).unwrap();
        wait_for_path(&args);
        let arguments = fs::read_to_string(args).unwrap();
        assert!(
            arguments
                .lines()
                .any(|argument| argument == "--startstreaming")
        );
        assert!(
            arguments
                .lines()
                .collect::<Vec<_>>()
                .windows(2)
                .any(|pair| pair == ["--scene", "Program"])
        );
        supervisor.stop(&setup).unwrap();
    }

    #[test]
    fn crash_preparation_preserves_sentinels_while_config_is_in_use() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let script = directory.path().join("fake-obs");
        let sentinel = config_dir.join(".sentinel");
        fs::create_dir_all(&sentinel).unwrap();
        fs::write(sentinel.join("run_crash"), "running").unwrap();
        fs::write(sentinel.join("keep-this"), "other").unwrap();
        make_script(&script, "exit 0");
        let setup = make_setup(&script, &config_dir);

        prepare_after_crash(&setup, true).unwrap();
        assert!(sentinel.join("run_crash").exists());
        prepare_after_crash(&setup, false).unwrap();
        assert!(!sentinel.join("run_crash").exists());
        assert!(sentinel.join("keep-this").exists());
    }

    #[test]
    fn crash_report_archive_keeps_five_and_copies_newest() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let script = directory.path().join("fake-obs");
        let crashes = config_dir.join("crashes");
        fs::create_dir_all(&crashes).unwrap();
        for index in 0..6 {
            let path = crashes.join(format!("crash-{index}.txt"));
            fs::write(&path, index.to_string()).unwrap();
            let file = fs::File::open(&path).unwrap();
            file.set_times(
                std::fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(index + 1)),
            )
            .unwrap();
        }
        let setup = make_setup(&script, &config_dir);
        let destination = config_dir
            .join("noalbs-connect/crash-reports")
            .join(setup.id.to_string());
        fs::create_dir_all(&destination).unwrap();
        for index in 0..5 {
            fs::write(destination.join(format!("old-{index}")), "old").unwrap();
        }
        copy_latest_crash_report(&setup).unwrap();
        assert!(destination.join("crash-5.txt").exists());
        assert_eq!(fs::read_dir(destination).unwrap().count(), 5);
    }
}
