use std::{sync::Mutex, time::Duration};

use semver::Version;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

#[derive(Clone, Default, Serialize)]
pub(crate) struct UpdateState {
    pub supported: bool,
    pub bundle: Option<String>,
    pub current_version: String,
    pub checking: bool,
    pub available: Option<String>,
    pub installing: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub(crate) struct UpdateManager {
    state: Mutex<UpdateState>,
}

impl UpdateManager {
    pub(crate) fn initialize(&self, current_version: String) {
        let bundle_type = tauri::utils::platform::bundle_type();
        let state = UpdateState {
            supported: updates_supported(bundle_type.as_ref(), cfg!(debug_assertions)),
            bundle: bundle_type.as_ref().map(bundle_name).map(str::to_owned),
            current_version,
            ..UpdateState::default()
        };
        *self.state.lock().expect("update state lock") = state;
    }

    pub(crate) fn snapshot(&self) -> UpdateState {
        self.state.lock().expect("update state lock").clone()
    }

    fn update(&self, f: impl FnOnce(&mut UpdateState)) {
        f(&mut self.state.lock().expect("update state lock"));
    }

    fn begin_check(&self) -> bool {
        let mut state = self.state.lock().expect("update state lock");
        if !state.supported || state.checking || state.installing {
            return false;
        }
        state.checking = true;
        state.error = None;
        true
    }

    fn begin_install(&self) -> bool {
        let mut state = self.state.lock().expect("update state lock");
        if !state.supported || state.checking || state.installing {
            return false;
        }
        state.checking = true;
        state.installing = true;
        state.error = None;
        true
    }
}

fn updates_supported(
    bundle_type: Option<&tauri::utils::config::BundleType>,
    debug_assertions: bool,
) -> bool {
    bundle_type.is_some() && !debug_assertions
}

fn bundle_name(bundle_type: &tauri::utils::config::BundleType) -> &'static str {
    use tauri::utils::config::BundleType;

    match bundle_type {
        BundleType::Deb => "deb",
        BundleType::Rpm => "rpm",
        BundleType::AppImage => "appimage",
        BundleType::Msi => "msi",
        BundleType::Nsis => "nsis",
        BundleType::App => "app",
        BundleType::Dmg => "dmg",
    }
}

fn release_notes_url(version: &str) -> Result<String, String> {
    Version::parse(version).map_err(|error| format!("Invalid release version: {error}"))?;
    Ok(format!(
        "https://github.com/NOALBS/noalbs-cloud/releases/tag/connect-v{version}"
    ))
}

fn emit_update_state(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let _ = app.emit("connect-update-changed", state.updates.snapshot());
}

async fn perform_check(app: &AppHandle) {
    {
        let state = app.state::<crate::AppState>();
        if !state.updates.begin_check() {
            return;
        }
    }
    emit_update_state(app);

    let result = match app.updater() {
        Ok(updater) => updater
            .check()
            .await
            .map(|update| update.map(|update| update.version))
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    {
        let state = app.state::<crate::AppState>();
        state.updates.update(|current| {
            current.checking = false;
            match result {
                Ok(available) => {
                    current.available = available;
                    current.error = None;
                }
                Err(error) => {
                    current.available = None;
                    current.error = Some(error);
                }
            }
        });
    }
    emit_update_state(app);
}

pub(crate) fn start_background_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            perform_check(&app).await;
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
}

#[tauri::command]
pub(crate) async fn check_for_update(app: AppHandle) -> crate::UiState {
    perform_check(&app).await;
    crate::build_ui_state(&app.state::<crate::AppState>())
}

#[tauri::command]
pub(crate) async fn install_update(app: AppHandle) -> crate::UiState {
    {
        let state = app.state::<crate::AppState>();
        if !state.updates.begin_install() {
            return crate::build_ui_state(&state);
        }
    }
    emit_update_state(&app);

    let update = match app.updater() {
        Ok(updater) => match updater.check().await {
            Ok(update) => update,
            Err(error) => {
                set_install_error(&app, error.to_string());
                return crate::build_ui_state(&app.state::<crate::AppState>());
            }
        },
        Err(error) => {
            set_install_error(&app, error.to_string());
            return crate::build_ui_state(&app.state::<crate::AppState>());
        }
    };

    let Some(update) = update else {
        {
            let state = app.state::<crate::AppState>();
            state.updates.update(|current| {
                current.checking = false;
                current.installing = false;
                current.available = None;
                current.error = None;
            });
        }
        emit_update_state(&app);
        return crate::build_ui_state(&app.state::<crate::AppState>());
    };

    let available_version = update.version.clone();
    {
        let state = app.state::<crate::AppState>();
        state.updates.update(|current| {
            current.checking = false;
            current.available = Some(available_version);
        });
    }
    emit_update_state(&app);

    if let Err(error) = update.download_and_install(|_, _| {}, || {}).await {
        set_install_error(&app, error.to_string());
        return crate::build_ui_state(&app.state::<crate::AppState>());
    }

    #[cfg(target_os = "windows")]
    {
        {
            let state = app.state::<crate::AppState>();
            state.updates.update(|current| {
                current.checking = false;
                current.installing = false;
                current.error = None;
            });
        }
        emit_update_state(&app);
        crate::build_ui_state(&app.state::<crate::AppState>())
    }

    #[cfg(not(target_os = "windows"))]
    app.restart()
}

fn set_install_error(app: &AppHandle, error: String) {
    {
        let state = app.state::<crate::AppState>();
        state.updates.update(|current| {
            current.checking = false;
            current.installing = false;
            current.error = Some(error);
        });
    }
    emit_update_state(app);
}

#[tauri::command]
pub(crate) fn open_release_notes(app: AppHandle, version: String) -> crate::UiState {
    let result = release_notes_url(&version).and_then(|url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|error| error.to_string())
    });
    {
        let state = app.state::<crate::AppState>();
        state.updates.update(|current| {
            current.error = result.err();
        });
    }
    emit_update_state(&app);
    crate::build_ui_state(&app.state::<crate::AppState>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::utils::config::BundleType;

    #[test]
    fn updates_require_a_release_bundle_and_non_debug_build() {
        assert!(updates_supported(Some(&BundleType::App), false));
        assert!(!updates_supported(None, false));
        assert!(!updates_supported(Some(&BundleType::App), true));
    }

    #[test]
    fn bundle_names_match_updater_platform_keys() {
        assert_eq!(bundle_name(&BundleType::AppImage), "appimage");
        assert_eq!(bundle_name(&BundleType::Deb), "deb");
        assert_eq!(bundle_name(&BundleType::Rpm), "rpm");
        assert_eq!(bundle_name(&BundleType::Nsis), "nsis");
        assert_eq!(bundle_name(&BundleType::Msi), "msi");
        assert_eq!(bundle_name(&BundleType::App), "app");
        assert_eq!(bundle_name(&BundleType::Dmg), "dmg");
    }

    #[test]
    fn release_notes_url_accepts_only_semver_versions() {
        assert_eq!(
            release_notes_url("1.2.3").unwrap(),
            "https://github.com/NOALBS/noalbs-cloud/releases/tag/connect-v1.2.3"
        );
        assert!(release_notes_url("../latest").is_err());
        assert!(release_notes_url("https://example.com").is_err());
    }
}
