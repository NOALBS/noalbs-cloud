use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use common::programs::Program;

pub const PRODUCTION_API_URL: &str = "https://app.noalbs.com/api";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not find the user configuration directory")]
    NoConfigDir,
    #[error("configuration I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub start_at_login: bool,
    pub api_url: String,
    pub dashboard_url: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            start_at_login: true,
            api_url: PRODUCTION_API_URL.to_owned(),
            dashboard_url: None,
        }
    }
}

impl AppSettings {
    pub fn effective(&self) -> Self {
        let (api_url, dashboard_url) = Self::environment_overrides();
        self.with_environment_overrides(api_url.as_deref(), dashboard_url.as_deref())
    }

    pub fn environment_overrides() -> (Option<String>, Option<String>) {
        let api_url = std::env::var("NOALBS_CONNECT_API_URL")
            .ok()
            .map(|url| url.trim().to_owned())
            .filter(|url| !url.is_empty());
        let dashboard_url = std::env::var("NOALBS_CONNECT_DASHBOARD_URL")
            .ok()
            .map(|url| url.trim().to_owned())
            .filter(|url| !url.is_empty());
        (api_url, dashboard_url)
    }

    pub fn with_environment_overrides(
        &self,
        api_url: Option<&str>,
        dashboard_url: Option<&str>,
    ) -> Self {
        let mut settings = self.clone();
        if let Some(api_url) = api_url.map(str::trim).filter(|url| !url.is_empty()) {
            settings.api_url = api_url.to_owned();
        }
        if let Some(dashboard_url) = dashboard_url.map(str::trim).filter(|url| !url.is_empty()) {
            settings.dashboard_url = Some(dashboard_url.to_owned());
        }
        settings
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InstallKind {
    #[default]
    Native,
    Flatpak,
    CustomBinary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    Linux,
    MacOS,
    Windows,
}

impl HostPlatform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOS
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOS => "macos",
            Self::Windows => "windows",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ObsLaunchConfig {
    pub install_kind: InstallKind,
    pub binary_path: Option<PathBuf>,
    pub config_dir_override: Option<PathBuf>,
    pub portable: bool,
    pub shared_config: bool,
    pub websocket_port: Option<u16>,
    pub websocket_password_override: Option<String>,
    pub profile: Option<String>,
    pub collection: Option<String>,
    pub scene: Option<String>,
    pub last_obs_version: Option<String>,
    pub minimize_to_tray: bool,
    pub disable_updater: bool,
    pub start_obs_with_connect: bool,
    pub use_dashboard_obs_settings: bool,
}

impl Default for ObsLaunchConfig {
    fn default() -> Self {
        Self {
            install_kind: InstallKind::Native,
            binary_path: None,
            config_dir_override: None,
            portable: false,
            shared_config: false,
            websocket_port: None,
            websocket_password_override: None,
            profile: None,
            collection: None,
            scene: None,
            last_obs_version: None,
            minimize_to_tray: false,
            disable_updater: false,
            start_obs_with_connect: false,
            use_dashboard_obs_settings: false,
        }
    }
}

pub fn windows_obs_install_root(binary_path: &Path) -> Option<PathBuf> {
    let architecture_dir = binary_path.parent()?;
    if !path_component_is(architecture_dir, "64bit") {
        return None;
    }
    let bin_dir = architecture_dir.parent()?;
    if !path_component_is(bin_dir, "bin") {
        return None;
    }
    bin_dir.parent().map(Path::to_owned)
}

fn path_component_is(path: &Path, expected: &str) -> bool {
    path.file_name()
        .is_some_and(|component| component.to_string_lossy().eq_ignore_ascii_case(expected))
}

pub fn validate_obs_config_for_platform(
    config: &ObsLaunchConfig,
    platform: HostPlatform,
) -> Result<(), &'static str> {
    if config.config_dir_override.is_some() {
        match platform {
            HostPlatform::Windows => {
                return Err(
                    "Separate OBS settings folders are not supported on Windows. Use a portable copy of OBS instead.",
                );
            }
            HostPlatform::MacOS => {
                return Err(
                    "OBS settings folder overrides are not supported on macOS. Use shared config with a distinct websocket port for multiple setups.",
                );
            }
            HostPlatform::Linux => {}
        }
    }
    if platform == HostPlatform::Windows
        && config.portable
        && config
            .binary_path
            .as_deref()
            .is_some_and(|path| windows_obs_install_root(path).is_none())
    {
        return Err("Portable OBS must be installed under a bin\\64bit folder.");
    }
    if platform != HostPlatform::Linux && matches!(config.install_kind, InstallKind::Flatpak) {
        return Err("Flatpak OBS is only supported on Linux.");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pairing {
    pub device_id: Uuid,
    pub switcher_id: Uuid,
    pub link_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Setup {
    pub id: Uuid,
    pub name: String,
    pub obs: ObsLaunchConfig,
    pub pairing: Option<Pairing>,
    #[serde(default)]
    pub programs: Vec<Program>,
    #[serde(default = "default_allow_dashboard_program_edits")]
    pub allow_dashboard_program_edits: bool,
}

fn default_allow_dashboard_program_edits() -> bool {
    true
}

impl Default for Setup {
    fn default() -> Self {
        Self::new("OBS".to_owned())
    }
}

impl Setup {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            obs: ObsLaunchConfig::default(),
            pairing: None,
            programs: Vec::new(),
            allow_dashboard_program_edits: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub settings: AppSettings,
    pub setups: Vec<Setup>,
    pub pending_token_deletions: Vec<uuid::Uuid>,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn default_path() -> Result<PathBuf, ConfigError> {
        let dirs = BaseDirs::new().ok_or(ConfigError::NoConfigDir)?;
        Ok(dirs.config_dir().join("noalbs-connect").join("config.json"))
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<AppConfig, ConfigError> {
        match File::open(&self.path) {
            Ok(mut file) => {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                Ok(serde_json::from_slice(&bytes)?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn load_or_backup(&self) -> (AppConfig, Option<String>) {
        match self.load() {
            Ok(config) => (config, None),
            Err(error) => {
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let backup = self
                    .path
                    .with_file_name(format!("config.json.broken-{timestamp}"));
                match fs::rename(&self.path, &backup) {
                    Ok(()) => (
                        AppConfig::default(),
                        Some(format!(
                            "Your Connect settings couldn't be read and were saved as {} ({error})",
                            backup.display()
                        )),
                    ),
                    Err(backup_error) => (
                        AppConfig::default(),
                        Some(format!(
                            "Your Connect settings couldn't be read and could not be saved as {}: {backup_error}",
                            backup.display()
                        )),
                    ),
                }
            }
        }
    }

    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        let parent = self.path.parent().ok_or(ConfigError::NoConfigDir)?;
        fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        temp.write_all(&serde_json::to_vec_pretty(config)?)?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, ConfigStore, HostPlatform, ObsLaunchConfig, PRODUCTION_API_URL, Setup,
        validate_obs_config_for_platform, windows_obs_install_root,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn config_round_trips_setups_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("noalbs-connect/config.json"));
        let mut config = AppConfig::default();
        config.setups.push(Setup::new("Studio".into()));
        let pending_token_deletion = uuid::Uuid::new_v4();
        config.pending_token_deletions.push(pending_token_deletion);
        store.save(&config).unwrap();
        let loaded = store.load().unwrap();
        assert!(loaded.settings.start_at_login);
        assert_eq!(loaded.setups[0].name, "Studio");
        assert!(!loaded.setups[0].obs.start_obs_with_connect);
        assert_eq!(loaded.pending_token_deletions, [pending_token_deletion]);
    }

    #[test]
    fn corrupt_config_is_preserved_under_a_broken_backup_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, b"{not valid json").unwrap();
        let store = ConfigStore::new(path.clone());

        let (config, error) = store.load_or_backup();

        assert!(config.setups.is_empty());
        assert!(error.unwrap().contains("were saved as"));
        assert!(!path.exists());
        let backup = std::fs::read_dir(dir.path())
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.json.broken-")
            })
            .unwrap()
            .path();
        assert_eq!(std::fs::read(backup).unwrap(), b"{not valid json");
    }

    #[test]
    fn environment_overrides_apply_after_loading_existing_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"settings":{"start_at_login":false},"setups":[]}"#,
        )
        .unwrap();
        let store = ConfigStore::new(path);
        let loaded = store.load().unwrap();

        let effective = loaded.settings.with_environment_overrides(
            Some("https://api.example/api"),
            Some("https://dashboard.example"),
        );

        assert_eq!(loaded.settings.api_url, PRODUCTION_API_URL);
        assert_eq!(loaded.settings.dashboard_url, None);
        assert_eq!(effective.api_url, "https://api.example/api");
        assert_eq!(
            effective.dashboard_url.as_deref(),
            Some("https://dashboard.example")
        );
        assert_eq!(
            loaded
                .settings
                .with_environment_overrides(Some("  "), None)
                .api_url,
            PRODUCTION_API_URL
        );
        store.save(&loaded).unwrap();
        let persisted = store.load().unwrap();
        assert_eq!(persisted.settings.api_url, PRODUCTION_API_URL);
        assert_eq!(persisted.settings.dashboard_url, None);
    }

    #[test]
    fn macos_and_windows_reject_config_dir_overrides_and_flatpak() {
        let mut config = ObsLaunchConfig {
            config_dir_override: Some("custom".into()),
            ..Default::default()
        };
        for platform in [HostPlatform::MacOS, HostPlatform::Windows] {
            assert!(validate_obs_config_for_platform(&config, platform).is_err());
        }
        config.config_dir_override = None;
        config.install_kind = super::InstallKind::Flatpak;
        for platform in [HostPlatform::MacOS, HostPlatform::Windows] {
            assert!(validate_obs_config_for_platform(&config, platform).is_err());
        }
        config.install_kind = super::InstallKind::Native;
        for platform in [HostPlatform::MacOS, HostPlatform::Windows] {
            assert!(validate_obs_config_for_platform(&config, platform).is_ok());
        }
        assert!(
            validate_obs_config_for_platform(&ObsLaunchConfig::default(), HostPlatform::Linux)
                .is_ok()
        );
    }

    #[test]
    fn windows_settings_folder_override_recommends_portable_obs() {
        let config = ObsLaunchConfig {
            config_dir_override: Some("custom".into()),
            ..Default::default()
        };

        assert_eq!(
            validate_obs_config_for_platform(&config, HostPlatform::Windows),
            Err(
                "Separate OBS settings folders are not supported on Windows. Use a portable copy of OBS instead."
            )
        );
    }

    #[test]
    fn portable_windows_obs_requires_the_standard_install_layout() {
        let config = ObsLaunchConfig {
            portable: true,
            binary_path: Some("custom/obs64.exe".into()),
            ..Default::default()
        };

        assert_eq!(
            validate_obs_config_for_platform(&config, HostPlatform::Windows),
            Err("Portable OBS must be installed under a bin\\64bit folder.")
        );
    }

    #[test]
    fn portable_windows_obs_with_the_standard_layout_is_valid() {
        let config = ObsLaunchConfig {
            portable: true,
            binary_path: Some("C:/obs-portable/bin/64bit/obs64.exe".into()),
            ..Default::default()
        };

        assert!(validate_obs_config_for_platform(&config, HostPlatform::Windows).is_ok());
    }

    #[test]
    fn windows_obs_install_root_is_above_bin_64bit() {
        assert_eq!(
            windows_obs_install_root(Path::new("C:/obs-portable/bin/64bit/obs64.exe")),
            Some(PathBuf::from("C:/obs-portable"))
        );
        assert_eq!(
            windows_obs_install_root(Path::new("C:/obs-portable/obs64.exe")),
            None
        );
    }
}
