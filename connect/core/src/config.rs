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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ObsLaunchConfig {
    pub install_kind: InstallKind,
    pub binary_path: Option<PathBuf>,
    pub config_dir_override: Option<PathBuf>,
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub settings: AppSettings,
    pub setups: Vec<Setup>,
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
    use super::{AppConfig, ConfigStore, PRODUCTION_API_URL, Setup};

    #[test]
    fn config_round_trips_setups_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("noalbs-connect/config.json"));
        let mut config = AppConfig::default();
        config.setups.push(Setup::new("Studio".into()));
        store.save(&config).unwrap();
        let loaded = store.load().unwrap();
        assert!(loaded.settings.start_at_login);
        assert_eq!(loaded.setups[0].name, "Studio");
        assert!(!loaded.setups[0].obs.start_obs_with_connect);
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
}
