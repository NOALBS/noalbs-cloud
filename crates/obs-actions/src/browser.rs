//! Reloading browser sources on the scene on air.

use std::collections::HashSet;

use crate::{
    ClientTransport, Error, ObsClient,
    error::Result,
    types::{GetInputSettings, RequestKind},
};

pub const BROWSER_KIND: &str = "browser_source";
const RELOAD_PROPERTY: &str = "refreshnocache";

/// Whether `url` belongs to the page used by NOALBS Cloud's OBS link.
pub fn is_link_page(url: &str, link_page: &str) -> bool {
    !link_page.is_empty() && url.starts_with(link_page)
}

/// What [`ObsClient::reload_browser_sources`] did.
#[derive(Debug, Default)]
pub struct ReloadOutcome {
    /// The scene that was on air.
    pub scene: String,
    /// The browser sources told to reload.
    pub reloaded: Vec<String>,
    /// The sources OBS refused to reload (or didn't answer for).
    pub failed: Vec<(String, Error)>,
}

impl<CT: ClientTransport> ObsClient<CT> {
    /// Reloads a browser source's current page without using its cache.
    pub async fn reload_browser_source(&self, name: &str) -> Result<()> {
        self.command(RequestKind::PressInputPropertiesButton {
            input_name: name,
            property_name: RELOAD_PROPERTY,
        })
        .await
    }

    /// Reloads browser sources on the scene on air, including nested scenes
    /// and groups. NOALBS Cloud's own link source is skipped.
    pub async fn reload_browser_sources(&self, link_page: &str) -> Result<ReloadOutcome> {
        let scene = self.scene_list().await?.current_program_scene_name;
        let items = self.scene_tree(&scene).await?;
        let mut outcome = ReloadOutcome {
            scene,
            ..ReloadOutcome::default()
        };
        let mut seen = HashSet::new();

        for item in items {
            if item.input_kind.as_deref() != Some(BROWSER_KIND) || !seen.insert(item.name.clone()) {
                continue;
            }

            let settings = self
                .send_request::<GetInputSettings>(RequestKind::GetInputSettings {
                    input_name: &item.name,
                })
                .await;
            let settings = match settings {
                Ok(settings) => settings,
                Err(error) => {
                    outcome.failed.push((item.name, error));
                    continue;
                }
            };
            if settings.input_kind != BROWSER_KIND {
                continue;
            }
            if settings.input_settings["url"]
                .as_str()
                .is_some_and(|url| is_link_page(url, link_page))
            {
                continue;
            }

            match self.reload_browser_source(&item.name).await {
                Ok(()) => outcome.reloaded.push(item.name),
                Err(error) => outcome.failed.push((item.name, error)),
            }
        }

        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{BROWSER_KIND, is_link_page};
    use crate::{
        Error,
        fake::{client, failure, input, scene},
    };

    #[test]
    fn identifies_only_urls_with_a_non_empty_link_page_prefix() {
        assert!(is_link_page(
            "https://app.example/source/token#obs",
            "https://app.example/source/"
        ));
        assert!(!is_link_page(
            "https://other.example/source/token",
            "https://app.example/source/"
        ));
        assert!(!is_link_page("https://app.example/source/", ""));
    }

    fn scenes(kind: &str, data: &Value) -> Value {
        match (kind, data["sceneName"].as_str().unwrap_or_default()) {
            ("GetSceneList", _) => json!({
                "currentProgramSceneName": "LIVE",
                "currentPreviewSceneName": null,
                "scenes": [],
            }),
            ("GetSceneItemList", "LIVE") => json!({
                "sceneItems": [
                    input(1, "Top", BROWSER_KIND, true),
                    input(2, "Audio", "ffmpeg_source", true),
                    scene(3, "Nested", false, true),
                    scene(4, "Group", true, true),
                ]
            }),
            ("GetSceneItemList", "Nested") => json!({
                "sceneItems": [
                    input(1, "Shared", BROWSER_KIND, true),
                    input(2, "Link", BROWSER_KIND, true),
                ]
            }),
            ("GetGroupSceneItemList", "Group") => json!({
                "sceneItems": [
                    input(1, "Shared", BROWSER_KIND, true),
                    input(2, "In group", BROWSER_KIND, true),
                ]
            }),
            ("GetInputSettings", _) => {
                let name = data["inputName"].as_str().unwrap_or_default();
                let url = if name == "Link" {
                    "https://app.example/source/token#obs"
                } else {
                    "https://example.com/page"
                };
                json!({
                    "inputKind": BROWSER_KIND,
                    "inputSettings": { "url": url }
                })
            }
            _ => Value::Null,
        }
    }

    #[tokio::test]
    async fn reloads_nested_and_group_browser_sources_once_and_skips_the_link() {
        let obs = client(scenes);
        let outcome = obs
            .reload_browser_sources("https://app.example/source/")
            .await
            .unwrap();

        assert_eq!(outcome.scene, "LIVE");
        assert_eq!(outcome.reloaded, ["In group", "Shared", "Top"]);
        assert!(outcome.failed.is_empty());
        assert_eq!(
            obs.asked(&["PressInputPropertiesButton"])
                .into_iter()
                .map(|(_, data)| data)
                .collect::<Vec<_>>(),
            [
                json!({ "inputName": "In group", "propertyName": "refreshnocache" }),
                json!({ "inputName": "Shared", "propertyName": "refreshnocache" }),
                json!({ "inputName": "Top", "propertyName": "refreshnocache" }),
            ]
        );
        assert_eq!(
            obs.asked(&["GetInputSettings"])
                .into_iter()
                .map(|(_, data)| data["inputName"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>(),
            ["In group", "Shared", "Link", "Top"]
        );
    }

    #[tokio::test]
    async fn a_failed_reload_does_not_stop_the_other_sources() {
        fn with_failure(kind: &str, data: &Value) -> Value {
            if kind == "GetSceneList"
                || kind == "GetSceneItemList"
                || kind == "GetGroupSceneItemList"
            {
                return scenes(kind, data);
            }
            if kind == "GetInputSettings" {
                return scenes(kind, data);
            }
            if kind == "PressInputPropertiesButton" && data["inputName"] == "Shared" {
                return failure(600);
            }
            json!({})
        }

        let obs = client(with_failure);
        let outcome = obs
            .reload_browser_sources("https://app.example/source/")
            .await
            .unwrap();

        assert_eq!(outcome.reloaded, ["In group", "Top"]);
        assert_eq!(outcome.failed.len(), 1);
        assert_eq!(outcome.failed[0].0, "Shared");
        assert!(matches!(
            outcome.failed[0].1,
            Error::ObsResponseError { code: 600, .. }
        ));
        assert_eq!(obs.asked(&["PressInputPropertiesButton"]).len(), 3);
    }
}
