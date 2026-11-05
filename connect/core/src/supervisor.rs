use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    config::{
        HostPlatform, InstallKind, ObsLaunchConfig, Setup, validate_obs_config_for_platform,
        windows_obs_install_root,
    },
    relay::PcObsState,
};

const WINDOWS_PORTABLE_MARKERS: [&str; 4] = [
    "portable_mode",
    "obs_portable_mode",
    "portable_mode.txt",
    "obs_portable_mode.txt",
];

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("OBS process I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("OBS websocket is disabled in its configuration")]
    WebsocketDisabled,
    #[error("OBS process is not identified; refusing to control it")]
    Unidentified,
    #[error("OBS did not exit after being force-closed")]
    DidNotExit,
    #[error("{0}")]
    UnsupportedPlatformConfig(&'static str),
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
    let port = websocket_port.unwrap_or_else(|| {
        config
            .get("server_port")
            .and_then(Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0)
            .unwrap_or(4455)
    });
    config["server_port"] = Value::from(port);
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
    config_dir_for_platform(setup, HostPlatform::current())
}

fn config_dir_for_platform(setup: &Setup, platform: HostPlatform) -> PathBuf {
    let binary = setup
        .obs
        .binary_path
        .clone()
        .unwrap_or_else(default_obs_binary);
    if is_portable_obs_for_platform(&setup.obs, &binary, platform)
        && let Some(root) = windows_obs_install_root(&binary)
    {
        return root.join("config").join("obs-studio");
    }
    if let Some(path) = &setup.obs.config_dir_override {
        return path.clone();
    }
    match platform {
        HostPlatform::MacOS => directories::BaseDirs::new()
            .map(|dirs| macos_obs_config_dir(dirs.home_dir()))
            .unwrap_or_default(),
        HostPlatform::Windows => directories::BaseDirs::new()
            .map(|dirs| dirs.config_dir().join("obs-studio"))
            .unwrap_or_default(),
        HostPlatform::Linux if matches!(setup.obs.install_kind, InstallKind::Flatpak) => {
            directories::BaseDirs::new()
                .map(|dirs| {
                    dirs.home_dir()
                        .join(".var/app/com.obsproject.Studio/config/obs-studio")
                })
                .unwrap_or_else(|| {
                    dirs_config_home()
                        .join("..")
                        .join(".var/app/com.obsproject.Studio/config/obs-studio")
                })
        }
        HostPlatform::Linux => dirs_config_home().join("obs-studio"),
    }
}

fn is_portable_obs_for_platform(
    config: &ObsLaunchConfig,
    binary_path: &Path,
    platform: HostPlatform,
) -> bool {
    platform == HostPlatform::Windows
        && (config.portable || windows_obs_has_portable_marker(binary_path))
}

fn windows_obs_has_portable_marker(binary_path: &Path) -> bool {
    windows_obs_install_root(binary_path).is_some_and(|root| {
        WINDOWS_PORTABLE_MARKERS
            .iter()
            .any(|marker| root.join(marker).is_file())
    })
}

fn command_line_matches_portable_mode(
    portable: bool,
    marker_present: bool,
    platform: HostPlatform,
    arguments: &[std::ffi::OsString],
) -> bool {
    if platform != HostPlatform::Windows || marker_present {
        return true;
    }
    let process_is_portable = arguments
        .iter()
        .any(|argument| matches!(argument.to_string_lossy().as_ref(), "--portable" | "-p"));
    portable == process_is_portable
}

fn should_launch_multi(
    portable: bool,
    any_obs_running: bool,
    another_child_running: bool,
    another_child_uses_config: bool,
) -> bool {
    if portable {
        another_child_uses_config
    } else {
        any_obs_running || another_child_running
    }
}

pub fn macos_obs_config_dir(home: &Path) -> PathBuf {
    home.join("Library/Application Support/obs-studio")
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
    obs_version: Option<&str>,
) -> Vec<String> {
    let binary = config
        .binary_path
        .clone()
        .unwrap_or_else(default_obs_binary);
    launch_arguments_for_platform(
        config,
        &binary,
        HostPlatform::current(),
        multi,
        resume,
        scene,
        obs_version,
    )
}

fn launch_arguments_for_platform(
    config: &ObsLaunchConfig,
    binary_path: &Path,
    platform: HostPlatform,
    multi: bool,
    resume: bool,
    scene: Option<&str>,
    obs_version: Option<&str>,
) -> Vec<String> {
    let mut args = vec!["--disable-missing-files-check".to_owned()];
    if is_portable_obs_for_platform(config, binary_path, platform) {
        args.push("--portable".into());
    }
    if multi {
        args.push("--multi".into());
    }
    if config.disable_updater {
        args.push("--disable-updater".into());
    }
    if should_disable_shutdown_check(obs_version) {
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

fn should_disable_shutdown_check(obs_version: Option<&str>) -> bool {
    let major = obs_version.and_then(|version| {
        version
            .split(|character: char| !character.is_ascii_digit() && character != '.')
            .filter(|part| part.contains('.'))
            .find_map(|part| part.split('.').next()?.parse::<u32>().ok())
    });
    major.is_none_or(|major| major < 32)
}

fn obs_version_for_launch(setup: &Setup, flatpak_binary: &Path) -> Option<String> {
    let mut command = if matches!(&setup.obs.install_kind, InstallKind::Flatpak) {
        let mut command = Command::new(flatpak_binary);
        command.arg("run");
        if let Some(config) = &setup.obs.config_dir_override {
            command.arg(format!(
                "--env=XDG_CONFIG_HOME={}",
                config.parent().unwrap_or(config).display()
            ));
        }
        command.args(["com.obsproject.Studio", "--version"]);
        command
    } else {
        let mut command = Command::new(
            setup
                .obs
                .binary_path
                .clone()
                .unwrap_or_else(default_obs_binary),
        );
        command.arg("--version");
        command
    };
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().ok()?;
                return if output.status.success() {
                    Some(format!(
                        "{}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ))
                } else {
                    None
                };
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
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
    is_flatpak: bool,
    flatpak_wrapper_start_time: Option<u64>,
    flatpak_obs: Option<(u32, u64)>,
}

pub struct Supervisor {
    children: HashMap<Uuid, StartedObs>,
    adopted: HashMap<Uuid, (Setup, u32, u64)>,
    known_setups: Vec<Setup>,
    connected: HashSet<Uuid>,
    crashed_pending: HashSet<Uuid>,
    crashed_state: HashSet<Uuid>,
    crashed_start_times: HashMap<Uuid, u64>,
    shutdown_grace: std::time::Duration,
    waiting_for_input_after: std::time::Duration,
    flatpak_resolution_timeout: Duration,
    flatpak_binary: PathBuf,
    allow_config_dir_override: bool,
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
            known_setups: Vec::new(),
            connected: HashSet::new(),
            crashed_pending: HashSet::new(),
            crashed_state: HashSet::new(),
            crashed_start_times: HashMap::new(),
            shutdown_grace,
            waiting_for_input_after: std::time::Duration::from_secs(60),
            flatpak_resolution_timeout: Duration::from_secs(10),
            flatpak_binary: PathBuf::from("flatpak"),
            allow_config_dir_override: cfg!(test),
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

    pub fn set_known_setups(&mut self, setups: &[Setup]) {
        self.known_setups = setups.to_vec();
    }

    pub fn obs_start_time(&self, setup: &Setup) -> Option<SystemTime> {
        let seconds = self
            .children
            .get(&setup.id)
            .map(|child| child.process_start_time)
            .or_else(|| self.adopted.get(&setup.id).map(|(_, _, start)| *start))
            .or_else(|| self.crashed_start_times.get(&setup.id).copied())?;
        (seconds > 0).then(|| UNIX_EPOCH + Duration::from_secs(seconds))
    }

    fn matching_obs_pids(&self, setup: &Setup) -> Vec<Pid> {
        matching_obs_pids_for_setups(setup, &self.known_setups)
    }

    pub fn start(
        &mut self,
        setup: &Setup,
        resume: bool,
        last_scene: Option<&str>,
    ) -> Result<u32, SupervisorError> {
        let crash_cleanup_pending = self.crashed_pending.contains(&setup.id);
        self.crashed_state.remove(&setup.id);
        self.crashed_start_times.remove(&setup.id);
        if !self.allow_config_dir_override {
            validate_obs_config_for_platform(&setup.obs, HostPlatform::current())
                .map_err(SupervisorError::UnsupportedPlatformConfig)?;
        }
        let mut replace_flatpak_child = false;
        if let Some(item) = self.children.get_mut(&setup.id) {
            if item.is_flatpak {
                let wrapper_pid = item.child.id();
                let wrapper_finished = item.child.try_wait()?.is_some();
                if item.flatpak_obs.is_none()
                    && !wrapper_finished
                    && let Some((pid, start_time)) = find_flatpak_obs_pid(wrapper_pid)
                {
                    item.flatpak_obs = Some((pid, start_time));
                    item.pid = pid;
                    item.process_start_time = start_time;
                }
                match item.flatpak_obs {
                    Some((pid, start_time)) if process_start_time(pid) == Some(start_time) => {
                        return Ok(pid);
                    }
                    Some(_) => {
                        if !reap_flatpak_wrapper(item, self.shutdown_grace)? {
                            return Err(SupervisorError::Unidentified);
                        }
                        replace_flatpak_child = true;
                    }
                    None if !wrapper_finished
                        && item.started.elapsed() < self.flatpak_resolution_timeout =>
                    {
                        return Ok(wrapper_pid);
                    }
                    None => {
                        if !reap_flatpak_wrapper(item, self.shutdown_grace)? {
                            return Err(SupervisorError::Unidentified);
                        }
                        replace_flatpak_child = true;
                    }
                }
                if has_run_sentinel(setup) {
                    self.crashed_pending.insert(setup.id);
                }
            } else {
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
        }
        if replace_flatpak_child || self.children.contains_key(&setup.id) {
            self.children.remove(&setup.id);
        }
        self.adopted
            .retain(|_, (_, pid, start_time)| process_start_time(*pid) == Some(*start_time));
        let sibling_identity = self.children.iter_mut().any(|(id, child)| {
            *id != setup.id
                && same_process_identity(setup, &child.setup)
                && started_obs_is_running(child)
        }) || self
            .adopted
            .iter()
            .any(|(id, (other, _, _))| *id != setup.id && same_process_identity(setup, other));
        let existing = self.matching_obs_pids(setup);
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
        let is_flatpak = matches!(setup.obs.install_kind, InstallKind::Flatpak);
        let executable = if is_flatpak {
            self.flatpak_binary.clone()
        } else {
            setup
                .obs
                .binary_path
                .clone()
                .unwrap_or_else(default_obs_binary)
        };
        let working_directory = if is_flatpak {
            None
        } else {
            executable
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(Path::to_owned)
        };
        let another_child_uses_config = self.children.iter_mut().any(|(id, child)| {
            *id != setup.id
                && child.config_dir == config_dir(setup)
                && started_obs_is_running(child)
        });
        if crash_cleanup_pending
            || self.crashed_pending.remove(&setup.id)
            || has_run_sentinel(setup)
        {
            self.crashed_pending.remove(&setup.id);
            prepare_after_crash(setup, another_child_uses_config)?;
        }
        let portable =
            is_portable_obs_for_platform(&setup.obs, &executable, HostPlatform::current());
        let another_child_running = self
            .children
            .iter_mut()
            .any(|(id, child)| *id != setup.id && started_obs_is_running(child));
        let another_portable_instance_running =
            portable && any_obs_running_for_executable(&executable);
        let multi = should_launch_multi(
            portable,
            !portable && any_obs_running(),
            another_child_running,
            another_child_uses_config || another_portable_instance_running,
        );
        let obs_version = obs_version_for_launch(setup, &self.flatpak_binary);
        let args = launch_arguments_for_platform(
            &setup.obs,
            &executable,
            HostPlatform::current(),
            multi,
            resume,
            last_scene,
            obs_version.as_deref(),
        );
        let mut command = if is_flatpak {
            let mut cmd = Command::new(&self.flatpak_binary);
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
        let launch_started = Instant::now();
        let child = command.spawn()?;
        let flatpak_wrapper_start_time = is_flatpak
            .then(|| wait_for_process_start_time(child.id()))
            .flatten();
        let flatpak_obs = if is_flatpak {
            wait_for_flatpak_obs_pid_with_timeout(child.id(), self.flatpak_resolution_timeout)
        } else {
            None
        };
        let (pid, process_start_time) = flatpak_obs.unwrap_or_else(|| {
            let pid = child.id();
            (pid, wait_for_process_start_time(pid).unwrap_or_default())
        });
        self.children.insert(
            setup.id,
            StartedObs {
                setup_id: setup.id,
                setup: setup.clone(),
                pid,
                child,
                started: launch_started,
                process_start_time,
                multi,
                config_dir: config_dir(setup),
                is_flatpak,
                flatpak_wrapper_start_time,
                flatpak_obs,
            },
        );
        self.connected.remove(&setup.id);
        Ok(pid)
    }

    pub fn state(&mut self, setup: &Setup) -> PcObsState {
        if self.crashed_state.contains(&setup.id) {
            if self.matching_obs_pids(setup).is_empty() {
                return PcObsState::Crashed;
            }
            self.crashed_state.remove(&setup.id);
        }
        if self
            .children
            .get(&setup.id)
            .is_some_and(|child| child.is_flatpak)
        {
            let mut remove_child = false;
            let mut crashed = false;
            let state = {
                let child = self
                    .children
                    .get_mut(&setup.id)
                    .expect("Flatpak child exists");
                let wrapper_finished = match child.child.try_wait() {
                    Ok(status) => status.is_some(),
                    Err(_) => return PcObsState::NotResponding,
                };
                if child.flatpak_obs.is_none()
                    && !wrapper_finished
                    && let Some((pid, start_time)) = find_flatpak_obs_pid(child.child.id())
                {
                    child.flatpak_obs = Some((pid, start_time));
                    child.pid = pid;
                    child.process_start_time = start_time;
                }
                let obs_running = child
                    .flatpak_obs
                    .is_some_and(|(pid, start_time)| process_start_time(pid) == Some(start_time));
                if obs_running {
                    if self.connected.contains(&setup.id) {
                        PcObsState::Connected
                    } else if child.started.elapsed() >= self.waiting_for_input_after {
                        PcObsState::WaitingForInput
                    } else {
                        PcObsState::Starting
                    }
                } else if child.flatpak_obs.is_some() || wrapper_finished {
                    remove_child = wrapper_finished;
                    crashed = has_run_sentinel(setup);
                    if crashed {
                        PcObsState::Crashed
                    } else {
                        PcObsState::NotRunning
                    }
                } else if child.started.elapsed() >= self.waiting_for_input_after {
                    PcObsState::WaitingForInput
                } else {
                    PcObsState::Starting
                }
            };
            if crashed {
                self.crashed_pending.insert(setup.id);
                self.crashed_state.insert(setup.id);
                if let Some(child) = self.children.get(&setup.id) {
                    self.crashed_start_times
                        .insert(setup.id, child.process_start_time);
                }
            }
            if remove_child {
                self.children.remove(&setup.id);
                self.connected.remove(&setup.id);
            }
            return state;
        }
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
                    if crashed {
                        self.crashed_start_times
                            .insert(setup.id, child.process_start_time);
                    }
                    self.children.remove(&setup.id);
                    self.connected.remove(&setup.id);
                    if crashed {
                        self.crashed_pending.insert(setup.id);
                        self.crashed_state.insert(setup.id);
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
        match self.matching_obs_pids(setup).as_slice() {
            [_] => PcObsState::WaitingForInput,
            [] if executable_obs_pids(setup).is_empty() => PcObsState::NotRunning,
            _ => PcObsState::Unknown,
        }
    }

    pub fn owns_running_process(&mut self, setup: &Setup) -> bool {
        let child_running = if let Some(child) = self.children.get_mut(&setup.id) {
            if child.is_flatpak {
                let wrapper_pid = child.child.id();
                let wrapper_finished = match child.child.try_wait() {
                    Ok(Some(_)) => true,
                    Ok(None) => false,
                    Err(_) => return false,
                };
                if child.flatpak_obs.is_none()
                    && !wrapper_finished
                    && let Some((pid, start_time)) = find_flatpak_obs_pid(wrapper_pid)
                {
                    child.flatpak_obs = Some((pid, start_time));
                    child.pid = pid;
                    child.process_start_time = start_time;
                }
                if let Some((pid, start_time)) = child.flatpak_obs {
                    process_start_time(pid) == Some(start_time)
                } else {
                    !wrapper_finished
                        && child.flatpak_wrapper_start_time.is_some_and(|start_time| {
                            process_start_time(wrapper_pid) == Some(start_time)
                        })
                }
            } else {
                child.child.try_wait().is_ok_and(|status| status.is_none())
                    && process_start_time(child.pid) == Some(child.process_start_time)
            }
        } else {
            false
        };
        child_running
            || self
                .adopted
                .get(&setup.id)
                .is_some_and(|(_, pid, start_time)| process_start_time(*pid) == Some(*start_time))
            || find_matching_obs_with_start_time_for_setups(setup, &self.known_setups).is_some()
    }

    pub fn mark_connected(&mut self, setup_id: Uuid, connected: bool) {
        if connected {
            self.connected.insert(setup_id);
        } else {
            self.clear_connected(setup_id);
        }
    }

    pub fn clear_connected(&mut self, setup_id: Uuid) {
        self.connected.remove(&setup_id);
    }

    pub fn forget(&mut self, setup_id: Uuid) {
        if let Some(child) = self.children.remove(&setup_id) {
            let mut child_process = child.child;
            std::thread::spawn(move || {
                let _ = child_process.wait();
            });
        }
        self.adopted.remove(&setup_id);
        self.crashed_pending.remove(&setup_id);
        self.crashed_state.remove(&setup_id);
        self.connected.remove(&setup_id);
    }

    pub fn stop(&mut self, setup: &Setup) -> Result<(), SupervisorError> {
        self.crashed_state.remove(&setup.id);
        self.crashed_start_times.remove(&setup.id);
        if self
            .children
            .get(&setup.id)
            .is_some_and(|child| child.is_flatpak)
        {
            return self.stop_flatpak(setup);
        }
        let (pid, expected_start_time, mut child) = match self.children.remove(&setup.id) {
            Some(item) => (item.pid, item.process_start_time, Some(item.child)),
            None => {
                if self.matching_obs_pids(setup).is_empty() {
                    self.connected.remove(&setup.id);
                    self.adopted.remove(&setup.id);
                    return Ok(());
                }
                let Some((pid, start_time)) =
                    find_matching_obs_with_start_time_for_setups(setup, &self.known_setups)
                else {
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
            let running = if matches!(setup.obs.install_kind, InstallKind::Flatpak) {
                process_start_time(pid) == Some(expected_start_time)
            } else if let Some(child) = child.as_mut() {
                child.try_wait()?.is_none()
            } else {
                process_is_running_for_setups(setup, pid, expected_start_time, &self.known_setups)
            };
            if !running {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if matches!(setup.obs.install_kind, InstallKind::Flatpak)
            && process_start_time(pid) == Some(expected_start_time)
        {
            force_kill(pid);
            let reap_deadline = Instant::now() + Duration::from_secs(5);
            while process_start_time(pid) == Some(expected_start_time)
                && Instant::now() < reap_deadline
            {
                std::thread::sleep(Duration::from_millis(100));
            }
            self.crashed_pending.insert(setup.id);
        } else if child.is_some() && !matches!(setup.obs.install_kind, InstallKind::Flatpak) {
            let child = child.take().expect("child was checked above");
            Self::reap_child_after_kill(child)?;
            self.crashed_pending.insert(setup.id);
        } else if process_is_running_for_setups(setup, pid, expected_start_time, &self.known_setups)
        {
            force_kill(pid);
            let reap_deadline = Instant::now() + Duration::from_secs(5);
            while process_is_running_for_setups(setup, pid, expected_start_time, &self.known_setups)
                && Instant::now() < reap_deadline
            {
                std::thread::sleep(Duration::from_millis(100));
            }
            self.crashed_pending.insert(setup.id);
        }
        self.adopted.remove(&setup.id);
        Ok(())
    }

    fn reap_child_after_kill(mut child: std::process::Child) -> Result<(), SupervisorError> {
        child.kill()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait()?.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        if child.try_wait()?.is_some() {
            Ok(())
        } else {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Err(SupervisorError::DidNotExit)
        }
    }

    fn stop_flatpak_unresolved(&mut self, setup: &Setup) -> Result<(), SupervisorError> {
        self.connected.remove(&setup.id);
        let Some(item) = self.children.get_mut(&setup.id) else {
            return Err(SupervisorError::Unidentified);
        };
        let wrapper_pid = item.child.id();
        let wrapper_start_time = item
            .flatpak_wrapper_start_time
            .ok_or(SupervisorError::Unidentified)?;
        let descendants = descendant_processes(wrapper_pid);
        for &(pid, start_time) in &descendants {
            if process_start_time(pid) == Some(start_time) {
                request_graceful_close(pid);
            }
        }
        if item.child.try_wait()?.is_none()
            && process_start_time(wrapper_pid) == Some(wrapper_start_time)
        {
            request_graceful_close(wrapper_pid);
        }
        let deadline = Instant::now() + self.shutdown_grace;
        loop {
            let wrapper_running = item.child.try_wait()?.is_none();
            let descendant_running = descendants
                .iter()
                .any(|(pid, start_time)| process_start_time(*pid) == Some(*start_time));
            if !wrapper_running && !descendant_running {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        for &(pid, start_time) in &descendants {
            if process_start_time(pid) == Some(start_time) {
                force_kill(pid);
            }
        }
        if item.child.try_wait()?.is_none()
            && process_start_time(wrapper_pid) == Some(wrapper_start_time)
        {
            force_kill(wrapper_pid);
        }
        let reap_deadline = Instant::now() + Duration::from_secs(5);
        while (item.child.try_wait()?.is_none()
            || descendants
                .iter()
                .any(|(pid, start_time)| process_start_time(*pid) == Some(*start_time)))
            && Instant::now() < reap_deadline
        {
            std::thread::sleep(Duration::from_millis(100));
        }
        if item.child.try_wait()?.is_none() {
            return Err(SupervisorError::Unidentified);
        }
        self.children.remove(&setup.id);
        self.adopted.remove(&setup.id);
        Ok(())
    }

    fn stop_flatpak(&mut self, setup: &Setup) -> Result<(), SupervisorError> {
        if self
            .children
            .get(&setup.id)
            .is_some_and(|child| child.flatpak_obs.is_none())
        {
            return self.stop_flatpak_unresolved(setup);
        }

        self.connected.remove(&setup.id);
        let Some(item) = self.children.get_mut(&setup.id) else {
            return Err(SupervisorError::Unidentified);
        };
        let Some((pid, start_time)) = item.flatpak_obs else {
            return Err(SupervisorError::Unidentified);
        };
        let wrapper_pid = item.child.id();
        let wrapper_start_time = item
            .flatpak_wrapper_start_time
            .ok_or(SupervisorError::Unidentified)?;
        if process_start_time(pid) == Some(start_time) {
            request_graceful_close(pid);
        }
        let deadline = Instant::now() + self.shutdown_grace;
        while process_start_time(pid) == Some(start_time) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        if process_start_time(pid) == Some(start_time) {
            force_kill(pid);
            self.crashed_pending.insert(setup.id);
        }

        let wrapper_deadline = Instant::now() + self.shutdown_grace;
        while item.child.try_wait()?.is_none() && Instant::now() < wrapper_deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        if item.child.try_wait()?.is_none() {
            if process_start_time(wrapper_pid) != Some(wrapper_start_time) {
                return Err(SupervisorError::Unidentified);
            }
            request_graceful_close(wrapper_pid);
            let terminate_deadline = Instant::now() + self.shutdown_grace;
            while item.child.try_wait()?.is_none() && Instant::now() < terminate_deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        if item.child.try_wait()?.is_none()
            && process_start_time(wrapper_pid) == Some(wrapper_start_time)
        {
            force_kill(wrapper_pid);
        }
        let reap_deadline = Instant::now() + Duration::from_secs(5);
        while item.child.try_wait()?.is_none() && Instant::now() < reap_deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        let wrapper_reaped = item.child.try_wait()?.is_some();
        let obs_running = process_start_time(pid) == Some(start_time);
        if wrapper_reaped && !obs_running {
            self.children.remove(&setup.id);
            self.adopted.remove(&setup.id);
        } else if !wrapper_reaped {
            return Err(SupervisorError::Unidentified);
        }
        Ok(())
    }

    pub fn has_child(&self, setup_id: Uuid) -> bool {
        self.children.contains_key(&setup_id)
    }
}

fn reap_flatpak_wrapper(
    item: &mut StartedObs,
    shutdown_grace: Duration,
) -> Result<bool, SupervisorError> {
    let pid = item.child.id();
    if item.child.try_wait()?.is_some() {
        return Ok(true);
    }
    let Some(start_time) = item.flatpak_wrapper_start_time else {
        return Err(SupervisorError::Unidentified);
    };
    if process_start_time(pid) != Some(start_time) {
        return Ok(item.child.try_wait()?.is_some());
    }
    request_graceful_close(pid);

    let terminate_deadline = Instant::now() + shutdown_grace;
    while item.child.try_wait()?.is_none() && Instant::now() < terminate_deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    if item.child.try_wait()?.is_none() && process_start_time(pid) == Some(start_time) {
        force_kill(pid);
    }
    let reap_deadline = Instant::now() + Duration::from_secs(5);
    while item.child.try_wait()?.is_none() && Instant::now() < reap_deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(item.child.try_wait()?.is_some())
}

pub fn find_matching_obs(setup: &Setup) -> Option<Pid> {
    let matches = matching_obs_pids(setup);
    (matches.len() == 1).then(|| matches[0])
}

pub fn same_obs_identity(left: &Setup, right: &Setup) -> bool {
    same_obs_identity_for_platform(left, right, HostPlatform::current())
}

pub fn same_obs_binary(left: &Setup, right: &Setup) -> bool {
    let binary = |setup: &Setup| {
        resolve_executable(
            setup
                .obs
                .binary_path
                .as_deref()
                .unwrap_or(&detected_obs_binary_for(&setup.obs.install_kind)),
        )
    };
    same_path(&binary(left), &binary(right))
}

pub fn setup_is_portable_for_platform(setup: &Setup, platform: HostPlatform) -> bool {
    let binary = setup
        .obs
        .binary_path
        .clone()
        .unwrap_or_else(|| detected_obs_binary_for(&setup.obs.install_kind));
    let binary = resolve_executable(&binary);
    is_portable_obs_for_platform(&setup.obs, &binary, platform)
}

pub fn same_obs_identity_for_platform(left: &Setup, right: &Setup, platform: HostPlatform) -> bool {
    same_obs_binary(left, right)
        && config_dir_for_platform(left, platform) == config_dir_for_platform(right, platform)
}

fn same_process_identity(left: &Setup, right: &Setup) -> bool {
    same_obs_identity(left, right)
        && (!(left.obs.shared_config && right.obs.shared_config)
            || left.obs.websocket_port == right.obs.websocket_port)
}

fn started_obs_is_running(child: &mut StartedObs) -> bool {
    if child.is_flatpak
        && child
            .flatpak_obs
            .is_some_and(|(pid, start_time)| process_start_time(pid) == Some(start_time))
    {
        return true;
    }
    child.child.try_wait().ok().flatten().is_none()
}

fn find_matching_obs_with_start_time_for_setups(
    setup: &Setup,
    setups: &[Setup],
) -> Option<(Pid, u64)> {
    let matches = matching_obs_pids_for_setups(setup, setups);
    find_matching_obs_start_time(matches)
}

fn find_matching_obs_start_time(matches: Vec<Pid>) -> Option<(Pid, u64)> {
    if matches.len() != 1 {
        return None;
    }
    let pid = matches[0];
    Some((pid, process_start_time(pid.as_u32())?))
}

fn matching_obs_pids(setup: &Setup) -> Vec<Pid> {
    matching_obs_pids_for_setups(setup, std::slice::from_ref(setup))
}

fn matching_obs_pids_for_setups(setup: &Setup, setups: &[Setup]) -> Vec<Pid> {
    let mut known_setups = setups.to_vec();
    if !known_setups
        .iter()
        .any(|candidate| candidate.id == setup.id)
    {
        known_setups.push(setup.clone());
    }
    let shared_config_ports = known_setups
        .iter()
        .filter(|candidate| {
            candidate.obs.shared_config
                && same_obs_identity_for_platform(candidate, setup, HostPlatform::current())
        })
        .filter_map(|candidate| candidate.obs.websocket_port)
        .collect::<Vec<_>>();
    let config_server_port = setup
        .obs
        .shared_config
        .then(|| read_obs_websocket_config(&config_dir(setup)).ok())
        .flatten()
        .map(|config| config.server_port);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything().without_tasks(),
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
        .filter(|(_, process)| process.thread_kind().is_none())
        .filter_map(|(pid, process)| {
            let executable_matches = if matches!(setup.obs.install_kind, InstallKind::Flatpak) {
                is_flatpak_obs_process(pid.as_u32(), process)
            } else {
                process_executable_matches(process, &expected)
            };
            (executable_matches
                && process_matches_obs_config_with_shared_ports(
                    setup,
                    HostPlatform::current(),
                    process.environ(),
                    process.cmd(),
                    &shared_config_ports,
                    config_server_port,
                ))
            .then_some(*pid)
        })
        .collect()
}

fn any_obs_running_for_executable(executable: &Path) -> bool {
    let expected = resolve_executable(executable);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything().without_tasks(),
    );
    system
        .processes()
        .values()
        .filter(|process| process.thread_kind().is_none())
        .any(|process| process_executable_matches(process, &expected))
}

#[cfg(all(test, unix))]
fn process_matches_obs_config(
    setup: &Setup,
    platform: HostPlatform,
    environ: &[std::ffi::OsString],
    arguments: &[std::ffi::OsString],
) -> bool {
    process_matches_obs_config_with_shared_ports(
        setup,
        platform,
        environ,
        arguments,
        &setup.obs.websocket_port.into_iter().collect::<Vec<_>>(),
        read_obs_websocket_config(&config_dir_for_platform(setup, platform))
            .ok()
            .map(|config| config.server_port),
    )
}

fn process_matches_obs_config_with_shared_ports(
    setup: &Setup,
    platform: HostPlatform,
    environ: &[std::ffi::OsString],
    arguments: &[std::ffi::OsString],
    shared_config_ports: &[u16],
    config_server_port: Option<u16>,
) -> bool {
    let config_matches = match platform {
        HostPlatform::Linux => setup.obs.config_dir_override.as_ref().is_none_or(|config| {
            let expected = format!(
                "XDG_CONFIG_HOME={}",
                config.parent().unwrap_or(config).display()
            );
            environ
                .iter()
                .any(|entry| entry.to_string_lossy() == expected)
                || (matches!(setup.obs.install_kind, InstallKind::Flatpak)
                    && arguments
                        .iter()
                        .any(|argument| argument.to_string_lossy() == format!("--env={expected}")))
        }),
        HostPlatform::MacOS | HostPlatform::Windows => true,
    };
    let port_matches = !setup.obs.shared_config
        || shared_config_port_matches(
            setup.obs.websocket_port,
            shared_config_ports,
            config_server_port,
            arguments,
        );
    let expected_binary = setup
        .obs
        .binary_path
        .clone()
        .unwrap_or_else(|| detected_obs_binary_for(&setup.obs.install_kind));
    let portable_mode_matches = command_line_matches_portable_mode(
        setup.obs.portable,
        windows_obs_has_portable_marker(&resolve_executable(&expected_binary)),
        platform,
        arguments,
    );
    config_matches && port_matches && portable_mode_matches
}

fn shared_config_port_matches(
    setup_port: Option<u16>,
    shared_config_ports: &[u16],
    config_server_port: Option<u16>,
    arguments: &[std::ffi::OsString],
) -> bool {
    let has_port_argument = command_has_option_name(arguments, "--websocket_port");
    if has_port_argument {
        return setup_port.is_some_and(|port| {
            command_has_option(arguments, "--websocket_port", &port.to_string())
        });
    }
    let Some(config_server_port) = config_server_port else {
        return false;
    };
    setup_port == Some(config_server_port)
        && shared_config_ports
            .iter()
            .filter(|port| **port == config_server_port)
            .count()
            == 1
}

fn process_executable_matches(process: &sysinfo::Process, expected: &Path) -> bool {
    if process.exe().is_some_and(|exe| same_path(exe, expected)) {
        return true;
    }
    if !cfg!(target_os = "linux") {
        return false;
    }
    let Some(exe) = process.exe() else {
        return false;
    };
    is_nix_wrapped_binary(exe, expected)
        && process.cmd().first().is_some_and(|argv0| {
            // Nix/makeWrapper may exec a different binary while preserving argv[0].
            same_path(&resolve_executable(Path::new(argv0)), expected)
        })
}

fn is_nix_wrapped_binary(exe: &Path, expected: &Path) -> bool {
    // Nix makeWrapper/wrapProgram names the underlying binary .<name>-wrapped.
    let Some(expected_name) = expected.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(exe_name) = exe.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    exe_name
        .strip_prefix(&format!(".{expected_name}-wrapped"))
        .is_some_and(|suffix| suffix.chars().all(|character| character == '_'))
}

fn command_has_option(arguments: &[std::ffi::OsString], option: &str, value: &str) -> bool {
    let option_with_equals = format!("{option}=");
    arguments.iter().enumerate().any(|(index, argument)| {
        let argument = argument.to_string_lossy();
        argument.strip_prefix(&option_with_equals) == Some(value)
            || (argument == option
                && arguments
                    .get(index + 1)
                    .is_some_and(|next| next.to_string_lossy() == value))
    })
}

fn command_has_option_name(arguments: &[std::ffi::OsString], option: &str) -> bool {
    let option_with_equals = format!("{option}=");
    arguments.iter().any(|argument| {
        let argument = argument.to_string_lossy();
        argument == option || argument.starts_with(&option_with_equals)
    })
}

fn executable_obs_pids(setup: &Setup) -> Vec<Pid> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything().without_tasks(),
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
        .filter(|(_, process)| process.thread_kind().is_none())
        .filter_map(|(pid, process)| {
            let executable_matches = if matches!(setup.obs.install_kind, InstallKind::Flatpak) {
                is_flatpak_obs_process(pid.as_u32(), process)
            } else {
                process_executable_matches(process, &expected)
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
        ProcessRefreshKind::everything().without_tasks(),
    );
    system.processes().values().any(|process| {
        process.thread_kind().is_none()
            && is_obs_process_name(&process.name().to_string_lossy(), HostPlatform::current())
    })
}

fn is_obs_process_name(name: &str, platform: HostPlatform) -> bool {
    match platform {
        HostPlatform::Windows => name.eq_ignore_ascii_case("obs64.exe"),
        HostPlatform::MacOS => name.eq_ignore_ascii_case("obs"),
        HostPlatform::Linux => name == "obs",
    }
}

fn is_flatpak_obs_process(pid: u32, process: &sysinfo::Process) -> bool {
    process.name().to_string_lossy() == "obs"
        && fs::read_to_string(format!("/proc/{pid}/root/.flatpak-info")).is_ok_and(|contents| {
            contents
                .lines()
                .any(|line| line.trim() == "name=com.obsproject.Studio")
        })
}

fn wait_for_flatpak_obs_pid_with_timeout(
    wrapper_pid: u32,
    timeout: Duration,
) -> Option<(u32, u64)> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(obs) = find_flatpak_obs_pid(wrapper_pid) {
            return Some(obs);
        }
        if process_start_time(wrapper_pid).is_none() || Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn find_flatpak_obs_pid(wrapper_pid: u32) -> Option<(u32, u64)> {
    let wrapper_pid = Pid::from_u32(wrapper_pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything().without_tasks(),
    );
    let pid = find_descendant_pid(&system, wrapper_pid, |pid, process| {
        is_flatpak_obs_process(pid.as_u32(), process)
    })?;
    Some((pid.as_u32(), process_start_time(pid.as_u32())?))
}

fn find_descendant_pid(
    system: &System,
    ancestor: Pid,
    mut matches: impl FnMut(Pid, &sysinfo::Process) -> bool,
) -> Option<Pid> {
    system.processes().iter().find_map(|(pid, process)| {
        (process.thread_kind().is_none()
            && matches(*pid, process)
            && is_process_descendant(system, *pid, ancestor))
        .then_some(*pid)
    })
}

fn descendant_processes(wrapper_pid: u32) -> Vec<(u32, u64)> {
    let ancestor = Pid::from_u32(wrapper_pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything().without_tasks(),
    );
    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            (process.thread_kind().is_none()
                && is_process_descendant(&system, *pid, ancestor)
                && is_flatpak_shutdown_target(process))
            .then_some((pid.as_u32(), process.start_time()))
        })
        .collect()
}

fn is_flatpak_shutdown_target(process: &sysinfo::Process) -> bool {
    let is_target_name =
        |name: &std::ffi::OsStr| matches!(name.to_string_lossy().as_ref(), "obs" | "bwrap");
    is_target_name(process.name())
        || process
            .exe()
            .and_then(Path::file_name)
            .is_some_and(is_target_name)
}

fn is_process_descendant(system: &System, pid: Pid, ancestor: Pid) -> bool {
    let mut parent = system.process(pid).and_then(|process| process.parent());
    while let Some(parent_pid) = parent {
        if parent_pid == ancestor {
            return true;
        }
        parent = system
            .process(parent_pid)
            .and_then(|process| process.parent());
    }
    false
}

#[cfg(all(unix, not(target_os = "macos")))]
fn request_graceful_close(pid: u32) {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
}

#[cfg(target_os = "macos")]
fn request_graceful_close(pid: u32) {
    use objc2_app_kit::NSRunningApplication;
    if let Some(application) =
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
    {
        application.terminate();
    }
}

fn process_start_time(pid: u32) -> Option<u64> {
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::everything().without_tasks(),
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

fn process_is_running_for_setups(
    setup: &Setup,
    pid: u32,
    start_time: u64,
    setups: &[Setup],
) -> bool {
    matching_obs_pids_for_setups(setup, setups)
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
    let copied = destination.join(file_name);
    fs::copy(&latest, &copied)?;
    fs::File::options()
        .write(true)
        .open(&copied)?
        .set_modified(std::time::SystemTime::now())?;

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

#[cfg(all(not(windows), not(target_os = "macos")))]
fn default_obs_binary() -> PathBuf {
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("obs"))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("obs"))
}

pub fn macos_obs_binary_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications/OBS.app/Contents/MacOS/OBS"),
        home.join("Applications/OBS.app/Contents/MacOS/OBS"),
    ]
}

#[cfg(target_os = "macos")]
fn default_obs_binary() -> PathBuf {
    let home = directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_owned())
        .unwrap_or_default();
    macos_obs_binary_candidates(&home)
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("/Applications/OBS.app/Contents/MacOS/OBS"))
}

pub fn detected_obs_binary() -> PathBuf {
    default_obs_binary()
}

pub fn detected_obs_binary_for(install_kind: &InstallKind) -> PathBuf {
    if cfg!(target_os = "linux") && matches!(install_kind, InstallKind::Flatpak) {
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
    #[cfg(target_os = "macos")]
    use std::sync::OnceLock;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::Command,
        thread,
        time::{Duration, Instant, UNIX_EPOCH},
    };

    use tempfile::tempdir;

    #[cfg(target_os = "linux")]
    use super::resolve_executable;
    use super::{
        StartedObs, Supervisor, WINDOWS_PORTABLE_MARKERS, config_dir_for_platform,
        copy_latest_crash_report, is_obs_process_name, is_portable_obs_for_platform,
        launch_arguments, launch_arguments_for_platform, macos_obs_binary_candidates,
        macos_obs_config_dir, prepare_after_crash, process_start_time,
        same_obs_identity_for_platform, should_launch_multi,
    };
    use super::{SupervisorError, find_matching_obs_with_start_time_for_setups, matching_obs_pids};
    use crate::config::{HostPlatform, InstallKind, ObsLaunchConfig, Setup};

    #[test]
    fn force_closed_process_that_does_not_exit_has_an_error() {
        assert_eq!(
            SupervisorError::DidNotExit.to_string(),
            "OBS did not exit after being force-closed"
        );
    }

    #[test]
    fn macos_discovery_candidates_and_config_path_are_platform_specific() {
        let home = Path::new("/Users/tester");
        assert_eq!(
            macos_obs_binary_candidates(home),
            vec![
                PathBuf::from("/Applications/OBS.app/Contents/MacOS/OBS"),
                PathBuf::from("/Users/tester/Applications/OBS.app/Contents/MacOS/OBS")
            ]
        );
        assert_eq!(
            macos_obs_config_dir(home),
            PathBuf::from("/Users/tester/Library/Application Support/obs-studio")
        );
    }

    #[test]
    fn windows_portable_detection_supports_markers_and_explicit_setting() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("obs-portable");
        let binary = root.join("bin/64bit/obs64.exe");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        let config = ObsLaunchConfig {
            binary_path: Some(binary.clone()),
            ..Default::default()
        };

        assert!(!is_portable_obs_for_platform(
            &config,
            &binary,
            HostPlatform::Windows
        ));
        for marker in WINDOWS_PORTABLE_MARKERS {
            fs::write(root.join(marker), "").unwrap();
            assert!(is_portable_obs_for_platform(
                &config,
                &binary,
                HostPlatform::Windows
            ));
            fs::remove_file(root.join(marker)).unwrap();
        }
        let explicitly_portable = ObsLaunchConfig {
            portable: true,
            ..config
        };
        assert!(is_portable_obs_for_platform(
            &explicitly_portable,
            &binary,
            HostPlatform::Windows
        ));
        assert!(!is_portable_obs_for_platform(
            &explicitly_portable,
            &binary,
            HostPlatform::MacOS
        ));
    }

    #[test]
    fn portable_command_line_matching_separates_flag_only_and_normal_setups() {
        let portable_args = ["obs64.exe", "--portable"].map(std::ffi::OsString::from);
        let short_portable_args = ["obs64.exe", "-p"].map(std::ffi::OsString::from);
        let normal_args = ["obs64.exe", "--multi"].map(std::ffi::OsString::from);

        assert!(super::command_line_matches_portable_mode(
            true,
            false,
            HostPlatform::Windows,
            &portable_args
        ));
        assert!(super::command_line_matches_portable_mode(
            true,
            false,
            HostPlatform::Windows,
            &short_portable_args
        ));
        assert!(!super::command_line_matches_portable_mode(
            true,
            false,
            HostPlatform::Windows,
            &normal_args
        ));
        assert!(super::command_line_matches_portable_mode(
            false,
            false,
            HostPlatform::Windows,
            &normal_args
        ));
        assert!(!super::command_line_matches_portable_mode(
            false,
            false,
            HostPlatform::Windows,
            &portable_args
        ));
        assert!(super::command_line_matches_portable_mode(
            false,
            true,
            HostPlatform::Windows,
            &portable_args
        ));
        assert!(super::command_line_matches_portable_mode(
            true,
            false,
            HostPlatform::MacOS,
            &normal_args
        ));
    }

    #[test]
    fn windows_portable_settings_folder_is_relative_to_the_install_root() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("obs-portable");
        let binary = root.join("bin/64bit/obs64.exe");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(root.join("portable_mode"), "").unwrap();
        let mut setup = Setup::new("Portable OBS".into());
        setup.obs.binary_path = Some(binary.clone());

        let portable_config_dir = root.join("config/obs-studio");
        assert_eq!(
            config_dir_for_platform(&setup, HostPlatform::Windows),
            portable_config_dir
        );

        fs::remove_file(root.join("portable_mode")).unwrap();
        assert_ne!(
            config_dir_for_platform(&setup, HostPlatform::Windows),
            portable_config_dir
        );
        setup.obs.portable = true;
        assert_eq!(
            config_dir_for_platform(&setup, HostPlatform::Windows),
            portable_config_dir
        );
    }

    #[test]
    fn portable_launch_arguments_are_windows_only() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("obs-portable");
        let binary = root.join("bin/64bit/obs64.exe");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(root.join("portable_mode"), "").unwrap();
        let config = ObsLaunchConfig {
            binary_path: Some(binary.clone()),
            ..Default::default()
        };

        let windows_args = launch_arguments_for_platform(
            &config,
            &binary,
            HostPlatform::Windows,
            false,
            false,
            None,
            None,
        );
        assert!(windows_args.iter().any(|arg| arg == "--portable"));
        fs::remove_file(root.join("portable_mode")).unwrap();
        let windows_normal_args = launch_arguments_for_platform(
            &config,
            &binary,
            HostPlatform::Windows,
            false,
            false,
            None,
            None,
        );
        assert!(!windows_normal_args.iter().any(|arg| arg == "--portable"));
        let macos_args = launch_arguments_for_platform(
            &config,
            &binary,
            HostPlatform::MacOS,
            false,
            false,
            None,
            None,
        );
        assert!(!macos_args.iter().any(|arg| arg == "--portable"));
        let explicitly_portable = ObsLaunchConfig {
            portable: true,
            ..config
        };
        let explicit_args = launch_arguments_for_platform(
            &explicitly_portable,
            &binary,
            HostPlatform::Windows,
            false,
            false,
            None,
            None,
        );
        assert!(explicit_args.iter().any(|arg| arg == "--portable"));
    }

    #[test]
    fn portable_installs_are_isolated_and_only_same_install_needs_multi() {
        let directory = tempdir().unwrap();
        let first_binary = directory.path().join("first/bin/64bit/obs64.exe");
        let second_binary = directory.path().join("second/bin/64bit/obs64.exe");
        let mut first = Setup::new("First portable OBS".into());
        first.obs.binary_path = Some(first_binary.clone());
        first.obs.portable = true;
        let mut same_install = Setup::new("Same portable OBS".into());
        same_install.obs.binary_path = Some(first_binary);
        same_install.obs.portable = true;
        let mut other_install = Setup::new("Other portable OBS".into());
        other_install.obs.binary_path = Some(second_binary);
        other_install.obs.portable = true;

        assert!(same_obs_identity_for_platform(
            &first,
            &same_install,
            HostPlatform::Windows
        ));
        assert!(!same_obs_identity_for_platform(
            &first,
            &other_install,
            HostPlatform::Windows
        ));
        assert!(!should_launch_multi(true, true, true, false));
        assert!(should_launch_multi(true, false, true, true));
        assert!(should_launch_multi(false, true, false, false));
    }

    #[test]
    fn obs_process_name_matches_host_platform() {
        assert!(is_obs_process_name("OBS", HostPlatform::MacOS));
        assert!(is_obs_process_name("obs", HostPlatform::MacOS));
        assert!(is_obs_process_name("obs", HostPlatform::Linux));
        assert!(is_obs_process_name("obs64.exe", HostPlatform::Windows));
    }

    #[test]
    fn stop_is_a_noop_when_obs_is_not_running() {
        let directory = tempdir().unwrap();
        let mut setup = Setup::new("Fake OBS".into());
        setup.obs.install_kind = InstallKind::CustomBinary;
        setup.obs.binary_path = Some(directory.path().join("missing-obs"));
        let mut supervisor = Supervisor::default();

        assert!(supervisor.stop(&setup).is_ok());
        assert_eq!(
            supervisor.state(&setup),
            crate::relay::PcObsState::NotRunning
        );
    }

    #[test]
    fn clearing_connected_state_falls_back_to_process_state() {
        let directory = tempdir().unwrap();
        let script = make_script(
            &directory.path().join("fake-obs"),
            "trap 'exit 0' TERM; while :; do sleep 0.05; done",
        );
        let setup = make_setup(&script, &directory.path().join("config"));
        let mut supervisor =
            Supervisor::with_timing(Duration::from_secs(1), Duration::from_secs(60));

        supervisor.start(&setup, false, None).unwrap();
        supervisor.mark_connected(setup.id, true);
        assert_eq!(
            supervisor.state(&setup),
            crate::relay::PcObsState::Connected
        );

        supervisor.clear_connected(setup.id);

        assert_eq!(supervisor.state(&setup), crate::relay::PcObsState::Starting);
        supervisor.stop(&setup).unwrap();
    }

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

    fn make_flatpak_setup(config_dir: &Path) -> Setup {
        let mut setup = Setup::new("Flatpak OBS".into());
        setup.obs.install_kind = InstallKind::Flatpak;
        setup.obs.config_dir_override = Some(config_dir.to_owned());
        setup
    }

    fn make_script(path: &Path, body: &str) -> PathBuf {
        make_script_with_version(path, "OBS Studio 33.0.1", body)
    }

    fn make_script_with_version(path: &Path, version: &str, body: &str) -> PathBuf {
        let version = version.replace('\'', "'\\''");
        #[cfg(not(target_os = "macos"))]
        {
            fs::write(
                path,
                format!(
                    "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '{version}'; exit 0; fi\nFAKE_OBS_PID=$$\n{body}\n"
                ),
            )
            .unwrap();
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
            path.to_owned()
        }
        #[cfg(target_os = "macos")]
        {
            let executable_name = path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("fake-obs"))
                .to_string_lossy();
            let mut app_bundle = path.as_os_str().to_owned();
            app_bundle.push(".app");
            let app_bundle = PathBuf::from(app_bundle);
            let contents = app_bundle.join("Contents");
            let macos = contents.join("MacOS");
            let resources = contents.join("Resources");
            fs::create_dir_all(&macos).unwrap();
            fs::create_dir_all(&resources).unwrap();
            fs::write(
                contents.join("Info.plist"),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>{executable_name}</string><key>CFBundleIdentifier</key><string>com.noalbs.connect.fakeobs.{}</string><key>CFBundlePackageType</key><string>APPL</string><key>LSUIElement</key><true/></dict></plist>\n",
                    uuid::Uuid::new_v4()
                ),
            )
            .unwrap();
            fs::write(
                resources.join("body.sh"),
                format!(
                    "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf '%s\\n' '{version}'; exit 0; fi\nFAKE_OBS_PID=$PPID; (while kill -0 \"$FAKE_OBS_PID\" 2>/dev/null; do sleep 0.1; done; kill -KILL $$) 2>/dev/null &\n{body}\n"
                ),
            )
            .unwrap();
            fs::copy(
                macos_fake_obs_binary(),
                macos.join(executable_name.as_ref()),
            )
            .unwrap();
            macos.join(executable_name.as_ref())
        }
    }

    #[cfg(target_os = "macos")]
    fn macos_fake_obs_binary() -> &'static PathBuf {
        static BINARY: OnceLock<PathBuf> = OnceLock::new();
        BINARY.get_or_init(|| {
            let directory =
                std::env::temp_dir().join(format!("connect-fake-obs-{}", std::process::id()));
            fs::create_dir_all(&directory).unwrap();
            let source = directory.join("fake-obs.m");
            let binary = directory.join("fake-obs");
            fs::write(
                &source,
                r#"
#import <AppKit/AppKit.h>
#import <dispatch/dispatch.h>
#import <spawn.h>
#import <sys/wait.h>
#import <signal.h>
#import <stdlib.h>
#import <unistd.h>

extern char **environ;
static int fake_argc;
static char **fake_argv;
static pid_t fake_child = -1;

@interface FakeObsDelegate : NSObject <NSApplicationDelegate>
@end

@implementation FakeObsDelegate
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    NSString *script = [[[NSBundle mainBundle] bundlePath]
        stringByAppendingPathComponent:@"Contents/Resources/body.sh"];
    char **arguments = calloc((size_t)fake_argc + 2, sizeof(char *));
    arguments[0] = "/bin/sh";
    arguments[1] = (char *)[script fileSystemRepresentation];
    for (int index = 1; index < fake_argc; index++) {
        arguments[index + 1] = fake_argv[index];
    }
    int result = posix_spawn(&fake_child, "/bin/sh", NULL, NULL, arguments, environ);
    free(arguments);
    if (result != 0) {
        exit(127);
    }
    dispatch_async(dispatch_get_global_queue(DISPATCH_QUEUE_PRIORITY_DEFAULT, 0), ^{
        int status = 0;
        waitpid(fake_child, &status, 0);
        if (WIFEXITED(status)) {
            exit(WEXITSTATUS(status));
        }
        if (WIFSIGNALED(status)) {
            exit(128 + WTERMSIG(status));
        }
        exit(1);
    });
}

- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender {
    if (fake_child > 0) {
        kill(fake_child, SIGTERM);
    }
    return NSTerminateCancel;
}
@end

int main(int argc, char **argv) {
    @autoreleasepool {
        fake_argc = argc;
        fake_argv = argv;
        NSApplication *application = [NSApplication sharedApplication];
        [application setActivationPolicy:NSApplicationActivationPolicyAccessory];
        FakeObsDelegate *delegate = [FakeObsDelegate new];
        [application setDelegate:delegate];
        [application run];
        return 0;
    }
}
"#,
            )
            .unwrap();
            let status = Command::new("cc")
                .args(["-x", "objective-c", "-fobjc-arc", "-framework", "AppKit"])
                .arg(&source)
                .arg("-o")
                .arg(&binary)
                .status()
                .unwrap();
            assert!(status.success(), "failed to compile macOS fake OBS");
            binary
        })
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
        let args = launch_arguments(&config, true, true, Some("BRB"), Some("31.1.0"));
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

        let non_shared = launch_arguments(
            &ObsLaunchConfig::default(),
            false,
            false,
            None,
            Some("33.0.1"),
        );
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
    fn enabling_websocket_preserves_a_valid_existing_port_without_an_override() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let path = config_dir.join("plugin_config/obs-websocket/config.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"server_port":4459}"#).unwrap();

        super::enable_obs_websocket_on_port(&config_dir, None).unwrap();
        let config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(config["server_port"], 4459);

        fs::write(&path, r#"{"server_port":"invalid"}"#).unwrap();
        super::enable_obs_websocket_on_port(&config_dir, None).unwrap();
        let config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(config["server_port"], 4455);
    }

    #[test]
    fn flatpak_wrapper_descendant_resolution_finds_its_child_process() {
        let directory = tempdir().unwrap();
        let script = directory.path().join("fake-wrapper");
        let script = make_script(
            &script,
            "sleep 10 & child=$!; trap 'kill \"$child\"; wait \"$child\"; exit 0' TERM; wait \"$child\"",
        );
        let mut wrapper = Command::new(&script).spawn().unwrap();
        let mut system = sysinfo::System::new();
        let mut descendant = None;
        for _ in 0..100 {
            system.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::All,
                true,
                sysinfo::ProcessRefreshKind::everything().without_tasks(),
            );
            descendant = super::find_descendant_pid(
                &system,
                sysinfo::Pid::from_u32(wrapper.id()),
                |_, process| process.name().to_string_lossy() == "sleep",
            );
            if descendant.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let descendant = descendant.unwrap();
        assert_ne!(descendant.as_u32(), wrapper.id());
        super::request_graceful_close(wrapper.id());
        let _ = wrapper.wait().unwrap();
    }

    // Flatpak is only available on Linux.
    #[cfg(target_os = "linux")]
    #[test]
    fn unresolved_flatpak_stop_only_signals_obs_named_descendants() {
        let directory = tempdir().unwrap();
        let script = directory.path().join("fake-flatpak-wrapper");
        let obs_binary = directory.path().join("obs");
        fs::copy(resolve_executable(Path::new("sleep")), &obs_binary).unwrap();
        let obs_pid_path = directory.path().join("obs-pid");
        let unrelated_pid_path = directory.path().join("unrelated-pid");
        let script = make_script(
            &script,
            &format!(
                "'{}' 30 & obs=$!; sleep 30 & unrelated=$!; printf '%s' \"$obs\" > '{}'; printf '%s' \"$unrelated\" > '{}'; wait \"$obs\"",
                obs_binary.display(),
                obs_pid_path.display(),
                unrelated_pid_path.display()
            ),
        );
        let wrapper = Command::new(&script).spawn().unwrap();
        let wrapper_pid = wrapper.id();
        wait_for_path(&obs_pid_path);
        wait_for_path(&unrelated_pid_path);
        let obs_pid: u32 = fs::read_to_string(&obs_pid_path).unwrap().parse().unwrap();
        let unrelated_pid: u32 = fs::read_to_string(&unrelated_pid_path)
            .unwrap()
            .parse()
            .unwrap();
        let wrapper_start_time = process_start_time(wrapper_pid).unwrap();
        assert!(
            super::wait_for_flatpak_obs_pid_with_timeout(wrapper_pid, Duration::from_millis(20))
                .is_none()
        );

        let setup = make_flatpak_setup(&directory.path().join("config"));
        let mut supervisor =
            Supervisor::with_timing(Duration::from_secs(1), Duration::from_millis(300));
        supervisor.children.insert(
            setup.id,
            StartedObs {
                setup_id: setup.id,
                setup: setup.clone(),
                pid: wrapper_pid,
                child: wrapper,
                started: Instant::now(),
                process_start_time: wrapper_start_time,
                multi: false,
                config_dir: super::config_dir(&setup),
                is_flatpak: true,
                flatpak_wrapper_start_time: Some(wrapper_start_time),
                flatpak_obs: None,
            },
        );

        assert_eq!(supervisor.state(&setup), crate::relay::PcObsState::Starting);
        assert!(supervisor.owns_running_process(&setup));
        assert_eq!(supervisor.start(&setup, false, None).unwrap(), wrapper_pid);
        thread::sleep(Duration::from_millis(320));
        assert_eq!(
            supervisor.state(&setup),
            crate::relay::PcObsState::WaitingForInput
        );
        assert!(supervisor.owns_running_process(&setup));
        supervisor.stop(&setup).unwrap();

        assert!(!supervisor.has_child(setup.id));
        assert_eq!(process_start_time(wrapper_pid), None);
        assert_eq!(process_start_time(obs_pid), None);
        assert!(process_start_time(unrelated_pid).is_some());
        super::force_kill(unrelated_pid);
        for _ in 0..100 {
            if process_start_time(unrelated_pid).is_none() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(process_start_time(unrelated_pid), None);
    }

    #[test]
    fn flatpak_stop_reaps_its_wrapper_after_obs_exits() {
        let directory = tempdir().unwrap();
        let script = directory.path().join("fake-flatpak-wrapper");
        let child_pid_path = directory.path().join("child-pid");
        let script = make_script(
            &script,
            &format!(
                "sleep 30 & child=$!; printf '%s' \"$child\" > '{}'; trap '' TERM; wait \"$child\"; while :; do :; done",
                child_pid_path.display()
            ),
        );
        let wrapper = Command::new(&script).spawn().unwrap();
        let wrapper_pid = wrapper.id();
        wait_for_path(&child_pid_path);
        let obs_pid: u32 = fs::read_to_string(&child_pid_path)
            .unwrap()
            .parse()
            .unwrap();
        let obs_start_time = process_start_time(obs_pid).unwrap();
        let wrapper_start_time = process_start_time(wrapper_pid).unwrap();

        let setup = make_flatpak_setup(&directory.path().join("config"));
        let mut supervisor =
            Supervisor::with_timing(Duration::from_secs(1), Duration::from_secs(60));
        supervisor.children.insert(
            setup.id,
            StartedObs {
                setup_id: setup.id,
                setup: setup.clone(),
                pid: obs_pid,
                child: wrapper,
                started: Instant::now(),
                process_start_time: obs_start_time,
                multi: false,
                config_dir: super::config_dir(&setup),
                is_flatpak: true,
                flatpak_wrapper_start_time: Some(wrapper_start_time),
                flatpak_obs: Some((obs_pid, obs_start_time)),
            },
        );

        supervisor.stop(&setup).unwrap();

        assert!(!supervisor.has_child(setup.id));
        assert_eq!(process_start_time(wrapper_pid), None);
        assert_eq!(process_start_time(obs_pid), None);
        assert_ne!(wrapper_start_time, 0);
    }

    // Flatpak is only available on Linux.
    #[cfg(target_os = "linux")]
    #[test]
    fn start_replaces_a_flatpak_wrapper_after_its_resolved_obs_exits() {
        let directory = tempdir().unwrap();
        let obs_binary = directory.path().join("obs");
        fs::copy(resolve_executable(Path::new("sleep")), &obs_binary).unwrap();
        let old_script = directory.path().join("old-flatpak-wrapper");
        let obs_pid_path = directory.path().join("old-obs-pid");
        let other_pid_path = directory.path().join("old-other-pid");
        let old_script = make_script(
            &old_script,
            &format!(
                "'{}' 30 & obs=$!; sleep 30 & other=$!; printf '%s' \"$obs\" > '{}'; printf '%s' \"$other\" > '{}'; trap 'kill \"$obs\" \"$other\" 2>/dev/null; wait \"$obs\" \"$other\" 2>/dev/null; exit 0' TERM; wait \"$obs\"; wait \"$other\"",
                obs_binary.display(),
                obs_pid_path.display(),
                other_pid_path.display()
            ),
        );
        let wrapper = Command::new(&old_script).spawn().unwrap();
        let old_wrapper_pid = wrapper.id();
        wait_for_path(&obs_pid_path);
        let obs_pid: u32 = fs::read_to_string(&obs_pid_path).unwrap().parse().unwrap();
        let obs_start_time = process_start_time(obs_pid).unwrap();
        let wrapper_start_time = process_start_time(old_wrapper_pid).unwrap();

        let fake_flatpak = directory.path().join("flatpak");
        let new_wrapper_pid_path = directory.path().join("new-wrapper-pid");
        let fake_flatpak = make_script(
            &fake_flatpak,
            &format!(
                "case \"$*\" in *' --version') printf '%s\\n' 'OBS Studio 33.0.1'; exit 0;; esac; sleep 30 & child=$!; printf '%s' \"$child\" > '{}'; trap 'kill \"$child\" 2>/dev/null; wait \"$child\" 2>/dev/null; exit 0' TERM; wait \"$child\"",
                new_wrapper_pid_path.display()
            ),
        );
        let setup = make_flatpak_setup(&directory.path().join("config"));
        let mut supervisor =
            Supervisor::with_timing(Duration::from_millis(200), Duration::from_secs(60));
        supervisor.flatpak_binary = fake_flatpak;
        supervisor.flatpak_resolution_timeout = Duration::from_millis(100);
        supervisor.children.insert(
            setup.id,
            StartedObs {
                setup_id: setup.id,
                setup: setup.clone(),
                pid: obs_pid,
                child: wrapper,
                started: Instant::now(),
                process_start_time: obs_start_time,
                multi: false,
                config_dir: super::config_dir(&setup),
                is_flatpak: true,
                flatpak_wrapper_start_time: Some(wrapper_start_time),
                flatpak_obs: Some((obs_pid, obs_start_time)),
            },
        );

        super::request_graceful_close(obs_pid);
        for _ in 0..100 {
            if process_start_time(obs_pid).is_none() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(process_start_time(obs_pid), None);

        let new_pid = supervisor.start(&setup, false, None).unwrap();
        wait_for_path(&new_wrapper_pid_path);
        assert_ne!(new_pid, old_wrapper_pid);
        assert_eq!(process_start_time(old_wrapper_pid), None);
        assert!(supervisor.owns_running_process(&setup));
        supervisor.stop(&setup).unwrap();
        assert!(!supervisor.has_child(setup.id));
    }

    #[test]
    fn shared_config_process_matching_uses_its_websocket_port() {
        let directory = tempdir().unwrap();
        let xdg_config = directory.path().join("xdg");
        let config_dir = xdg_config.join("obs-studio");
        #[cfg(target_os = "linux")]
        let binary = resolve_executable(Path::new("sh"));
        #[cfg(target_os = "macos")]
        let binary = make_script(
            &directory.path().join("shared-config-obs"),
            "while :; do sleep 0.05; done",
        );
        let mut setup_x = make_manual_setup(&binary, &config_dir);
        setup_x.obs.shared_config = true;
        setup_x.obs.websocket_port = Some(4456);
        let mut setup_y = setup_x.clone();
        setup_y.id = uuid::Uuid::new_v4();
        setup_y.obs.websocket_port = Some(4457);
        #[cfg(target_os = "linux")]
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
        #[cfg(target_os = "macos")]
        let mut process = Command::new(&binary)
            .args(["--websocket_port", "4456"])
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
        let (_, start_time) =
            find_matching_obs_with_start_time_for_setups(&setup_x, &[setup_x.clone()]).unwrap();
        assert_eq!(process_start_time(process.id()), Some(start_time));
        assert!(matching_obs_pids(&setup_y).is_empty());

        process.kill().unwrap();
        let _ = process.wait().unwrap();
    }

    #[test]
    fn windows_shared_config_matching_uses_its_websocket_port() {
        let mut setup = Setup::new("Windows setup".into());
        setup.obs.shared_config = true;
        setup.obs.websocket_port = Some(4456);
        let args =
            ["obs64.exe", "--multi", "--websocket_port", "4456"].map(std::ffi::OsString::from);
        let other_port =
            ["obs64.exe", "--multi", "--websocket_port", "4457"].map(std::ffi::OsString::from);

        assert!(super::process_matches_obs_config(
            &setup,
            HostPlatform::Windows,
            &[],
            &args
        ));
        assert!(!super::process_matches_obs_config(
            &setup,
            HostPlatform::Windows,
            &[],
            &other_port
        ));
    }

    #[test]
    fn shared_config_without_a_port_argument_matches_only_a_unique_config_port() {
        let no_port_argument = ["obs64.exe"].map(std::ffi::OsString::from);
        let configured_ports = [4456, 4457];
        assert!(super::shared_config_port_matches(
            Some(4456),
            &configured_ports,
            Some(4456),
            &no_port_argument
        ));
        assert!(!super::shared_config_port_matches(
            Some(4456),
            &[4456, 4456],
            Some(4456),
            &no_port_argument
        ));
        assert!(!super::shared_config_port_matches(
            Some(4457),
            &configured_ports,
            Some(4456),
            &no_port_argument
        ));

        let explicit_port = ["obs64.exe", "--websocket_port", "4457"].map(std::ffi::OsString::from);
        assert!(super::shared_config_port_matches(
            Some(4457),
            &[4456, 4457],
            Some(4456),
            &explicit_port
        ));

        let equals_port = ["obs64.exe", "--websocket_port=4457"].map(std::ffi::OsString::from);
        assert!(super::command_has_option(
            &equals_port,
            "--websocket_port",
            "4457"
        ));
        assert!(!super::command_has_option(
            &equals_port,
            "--websocket_port",
            "4456"
        ));
        assert!(super::shared_config_port_matches(
            Some(4457),
            &[4456, 4457],
            Some(4456),
            &equals_port
        ));
        assert!(!super::shared_config_port_matches(
            Some(4456),
            &[4456],
            Some(4456),
            &equals_port
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn forgetting_setup_allows_replacement_to_adopt_its_running_process() {
        let directory = tempdir().unwrap();
        let xdg_config = directory.path().join("xdg");
        let config_dir = xdg_config.join("obs-studio");
        let binary = resolve_executable(Path::new("sleep"));
        let mut manual_obs = Command::new(&binary)
            .arg("30")
            .env("XDG_CONFIG_HOME", &xdg_config)
            .spawn()
            .unwrap();
        let pid = manual_obs.id();
        let start_time = (0..100)
            .find_map(|_| {
                let start_time = process_start_time(pid);
                if start_time.is_none() {
                    thread::sleep(Duration::from_millis(10));
                }
                start_time
            })
            .expect("OBS process start time");
        let setup = make_manual_setup(&binary, &config_dir);
        let mut replacement = setup.clone();
        replacement.id = uuid::Uuid::new_v4();
        let mut supervisor = Supervisor::with_shutdown_grace(Duration::from_millis(100));
        let placeholder_child = Command::new("true").spawn().unwrap();
        let placeholder_pid = placeholder_child.id();
        supervisor.children.insert(
            setup.id,
            StartedObs {
                setup_id: setup.id,
                setup: setup.clone(),
                pid: placeholder_pid,
                child: placeholder_child,
                started: Instant::now(),
                process_start_time: process_start_time(placeholder_pid).unwrap_or_default(),
                multi: false,
                config_dir: config_dir.clone(),
                is_flatpak: false,
                flatpak_wrapper_start_time: None,
                flatpak_obs: None,
            },
        );
        supervisor
            .adopted
            .insert(setup.id, (setup.clone(), pid, start_time));
        supervisor.connected.insert(setup.id);
        supervisor.crashed_pending.insert(setup.id);
        supervisor.crashed_state.insert(setup.id);

        supervisor.forget(setup.id);

        let placeholder_process = PathBuf::from(format!("/proc/{placeholder_pid}"));
        let reaped = (0..200).any(|_| {
            if !placeholder_process.exists() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
            false
        });
        assert!(reaped, "forgotten child was not reaped");
        assert!(!supervisor.has_child(setup.id));
        assert!(!supervisor.adopted.contains_key(&setup.id));
        assert!(!supervisor.connected.contains(&setup.id));
        assert!(!supervisor.crashed_pending.contains(&setup.id));
        assert!(!supervisor.crashed_state.contains(&setup.id));
        assert_eq!(process_start_time(pid), Some(start_time));
        assert_eq!(supervisor.start(&replacement, false, None).unwrap(), pid);
        assert!(supervisor.adopted.contains_key(&replacement.id));
        assert!(!supervisor.has_child(replacement.id));

        supervisor.stop(&replacement).unwrap();
        assert!(!manual_obs.wait().unwrap().success());
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
        let args = launch_arguments(&config, false, false, None, Some("31.1.0"));
        assert!(args.contains(&"--disable-missing-files-check".into()));
        assert!(args.contains(&"--disable-shutdown-check".into()));
        assert!(args.contains(&"--disable-updater".into()));
        assert!(args.contains(&"--minimize-to-tray".into()));
        assert!(args.windows(2).any(|pair| pair == ["--profile", "Live"]));
        assert!(args.windows(2).any(|pair| pair == ["--collection", "Main"]));
    }

    #[test]
    fn unknown_obs_version_uses_legacy_shutdown_check_flag() {
        assert!(super::should_disable_shutdown_check(None));
        assert!(super::should_disable_shutdown_check(Some("unknown")));
        assert!(super::should_disable_shutdown_check(Some(
            "OBS Studio - 30.2.3"
        )));
        assert!(!super::should_disable_shutdown_check(Some(
            "OBS Studio - 33.0.1"
        )));
    }

    #[test]
    fn watchdog_restart_uses_the_current_binary_version_for_launch_flags() {
        let directory = tempdir().unwrap();
        let script = directory.path().join("fake-obs");
        let args_path = directory.path().join("arguments");
        let script = make_script_with_version(
            &script,
            "OBS Studio 30.2.3",
            &format!(
                "printf '%s\\n' \"$@\" > '{}'; trap 'exit 0' TERM; while :; do sleep 0.05; done",
                args_path.display()
            ),
        );
        let mut setup = make_setup(&script, &directory.path().join("config"));
        setup.obs.last_obs_version = Some("33.0.1".into());

        let mut supervisor = Supervisor::default();
        supervisor.start(&setup, true, None).unwrap();
        wait_for_path(&args_path);
        let args = fs::read_to_string(&args_path).unwrap();
        let starts_streaming = args.lines().any(|arg| arg == "--startstreaming");
        let disables_shutdown_check = args.lines().any(|arg| arg == "--disable-shutdown-check");
        supervisor.stop(&setup).unwrap();
        assert!(starts_streaming);
        assert!(disables_shutdown_check);
    }

    #[test]
    fn clean_stop_removes_sentinel_and_unconnected_child_times_out() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let script = directory.path().join("fake-obs");
        let sentinel = config_dir.join(".sentinel");
        fs::create_dir_all(&sentinel).unwrap();
        let script = make_script(
            &script,
            &format!(
                "mkdir -p '{}'; touch '{}/run_$FAKE_OBS_PID'; trap 'rm -f {}/run_*; exit 0' TERM; while :; do sleep 0.05; done",
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
        let script = make_script(
            &script,
            &format!(
                "for f in '{}/run_'*; do if [ -e \"$f\" ]; then touch '{}'; fi; done; touch '{}/run_$FAKE_OBS_PID'; trap '' TERM; while :; do sleep 0.05; done",
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
        let script = make_script(
            &script,
            &format!(
                "printf '%s\\n' \"$@\" > '{}'.$FAKE_OBS_PID; trap 'exit 0' TERM; while :; do sleep 0.05; done",
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
        supervisor.stop(&setup_a).unwrap();
    }

    #[test]
    fn hand_started_obs_matches_executable_and_xdg_config() {
        let directory = tempdir().unwrap();
        let xdg_config = directory.path().join("xdg");
        let config_dir = xdg_config.join("obs-studio");
        // XDG_CONFIG_HOME scopes Linux only; macOS has no override and matches by executable.
        #[cfg(target_os = "linux")]
        let binary = {
            let binary = resolve_executable(Path::new("sleep"));
            assert!(binary.is_file());
            binary
        };
        #[cfg(target_os = "macos")]
        let binary = make_script(
            &directory.path().join("hand-started-obs"),
            "while :; do sleep 0.05; done",
        );
        let setup = make_manual_setup(&binary, &config_dir);
        let mut manual_obs = Command::new(&binary)
            .arg("30")
            .env("XDG_CONFIG_HOME", &xdg_config)
            .spawn()
            .unwrap();
        let mut supervisor = Supervisor::default();
        let pid = manual_obs.id();
        for _ in 0..100 {
            if matching_obs_pids(&setup).len() == 1 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(supervisor.owns_running_process(&setup));
        assert_eq!(supervisor.start(&setup, false, None).unwrap(), pid);
        assert_eq!(
            supervisor.state(&setup),
            crate::relay::PcObsState::WaitingForInput
        );
        supervisor.stop(&setup).unwrap();
        assert!(!manual_obs.wait().unwrap().success());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn hand_started_obs_matches_argv0_preserving_wrapper() {
        struct ChildGuard(std::process::Child);

        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let directory = tempdir().unwrap();
        let wrapped_binary = directory.path().join(".obs-wrapped");
        let bash_binary = resolve_executable(Path::new("bash"));
        assert!(bash_binary.is_file());
        fs::copy(bash_binary, &wrapped_binary).unwrap();

        let wrapper = directory.path().join("obs");
        fs::write(
            &wrapper,
            format!(
                "#!/usr/bin/env bash\nexec -a \"$0\" \"{}\" -c 'sleep \"$1\"; :' -- \"$@\"\n",
                wrapped_binary.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&wrapper).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wrapper, permissions).unwrap();

        let child = ChildGuard(Command::new(&wrapper).arg("30").spawn().unwrap());
        let child_pid = child.0.id();
        let mut setup = Setup::new("Wrapped OBS".into());
        setup.obs.install_kind = InstallKind::CustomBinary;
        setup.obs.binary_path = Some(wrapper);

        let mut matches = Vec::new();
        for _ in 0..100 {
            matches = matching_obs_pids(&setup);
            if matches.iter().any(|pid| pid.as_u32() == child_pid) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            matches.iter().any(|pid| pid.as_u32() == child_pid),
            "child was not matched: exe={:?}, cmdline={:?}",
            fs::read_link(format!("/proc/{child_pid}/exe")),
            fs::read(format!("/proc/{child_pid}/cmdline"))
        );

        let mut supervisor = Supervisor::default();
        assert_eq!(
            supervisor.state(&setup),
            crate::relay::PcObsState::WaitingForInput
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn multithreaded_hand_started_obs_is_matched_once() {
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let helper = thread::spawn(move || {
            release_receiver.recv().unwrap();
        });

        let mut setup = Setup::new("Current process".into());
        setup.obs.install_kind = InstallKind::CustomBinary;
        setup.obs.binary_path = Some(std::env::current_exe().unwrap());
        let expected = vec![sysinfo::Pid::from_u32(std::process::id())];
        let matching_pids = matching_obs_pids(&setup);
        let executable_pids = super::executable_obs_pids(&setup);

        release_sender.send(()).unwrap();
        helper.join().unwrap();

        assert_eq!(matching_pids, expected);
        assert_eq!(executable_pids, expected);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn nix_wrapped_binary_match_uses_make_wrapper_file_name() {
        let expected = Path::new("/usr/bin/obs");
        for name in [".obs-wrapped", ".obs-wrapped_", ".obs-wrapped__"] {
            assert!(
                super::is_nix_wrapped_binary(Path::new(name), expected),
                "{name} should match"
            );
        }
        for name in ["sleep", "obs-wrapped", ".obs-wrappedx", ".other-wrapped"] {
            assert!(
                !super::is_nix_wrapped_binary(Path::new(name), expected),
                "{name} should not match"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn argv0_wrapper_with_non_nix_executable_is_not_matched() {
        struct ChildGuard(std::process::Child);

        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let directory = tempdir().unwrap();
        let other_binary = directory.path().join("other-program");
        let bash_binary = resolve_executable(Path::new("bash"));
        assert!(bash_binary.is_file());
        fs::copy(bash_binary, &other_binary).unwrap();

        let wrapper = directory.path().join("obs");
        fs::write(
            &wrapper,
            format!(
                "#!/usr/bin/env bash\nexec -a \"$0\" \"{}\" -c 'sleep \"$1\"; :' -- \"$@\"\n",
                other_binary.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&wrapper).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wrapper, permissions).unwrap();

        let child = ChildGuard(Command::new(&wrapper).arg("30").spawn().unwrap());
        let child_pid = child.0.id();
        let mut executable = None;
        for _ in 0..100 {
            executable = fs::read_link(format!("/proc/{child_pid}/exe")).ok();
            if executable.as_deref() == Some(other_binary.as_path()) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(executable.as_deref(), Some(other_binary.as_path()));

        let mut setup = Setup::new("Wrapped OBS".into());
        setup.obs.install_kind = InstallKind::CustomBinary;
        setup.obs.binary_path = Some(wrapper);
        assert!(
            !matching_obs_pids(&setup)
                .iter()
                .any(|pid| pid.as_u32() == child_pid)
        );
        assert!(
            !super::executable_obs_pids(&setup)
                .iter()
                .any(|pid| pid.as_u32() == child_pid)
        );
    }

    #[test]
    fn ambiguous_hand_started_obs_is_reported_and_never_controlled() {
        let directory = tempdir().unwrap();
        let xdg_config = directory.path().join("xdg");
        let config_dir = xdg_config.join("obs-studio");
        #[cfg(target_os = "linux")]
        let binary = {
            let binary = resolve_executable(Path::new("sleep"));
            assert!(binary.is_file());
            binary
        };
        #[cfg(target_os = "macos")]
        let binary = make_script(
            &directory.path().join("ambiguous-hand-started-obs"),
            "while :; do sleep 0.05; done",
        );
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
        assert!(!supervisor.owns_running_process(&setup));
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
        let script = make_script(
            &script,
            &format!(
                "mkdir -p '{}'; for f in '{}/run_'*; do if [ -e \"$f\" ]; then touch '{}'; fi; done; touch '{}/run_$FAKE_OBS_PID'; trap 'rm -f {}/run_*; exit 0' TERM; while :; do sleep 0.05; done",
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

    fn make_crash_then_run_script(script: &Path, config_dir: &Path, args_path: &Path) -> PathBuf {
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
        )
    }

    #[test]
    fn crash_restart_uses_restart_mode_without_streaming() {
        let directory = tempdir().unwrap();
        let config_dir = directory.path().join("obs-studio");
        let script = directory.path().join("fake-obs");
        let args = directory.path().join("arguments");
        let script = make_crash_then_run_script(&script, &config_dir, &args);
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
        assert_eq!(supervisor.state(&setup), crate::relay::PcObsState::Crashed);
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
        assert_ne!(supervisor.state(&setup), crate::relay::PcObsState::Crashed);
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
        let script = make_crash_then_run_script(&script, &config_dir, &args);
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
        let script = make_script(&script, "exit 0");
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
