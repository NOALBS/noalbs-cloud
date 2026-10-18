//! Fixing a frozen feed, as NOALBS's `!fix` does (`fix()` in its `obs_v5.rs`):
//! every media source playing on the scene on air (nested scenes and groups
//! included) that reads a network stream is made to reconnect.

use serde_json::Value;

use crate::{
    ClientTransport, Error, ObsClient,
    error::Result,
    types::{GetInputSettings, GetMediaInputStatus, RequestKind},
};

const MEDIA_KINDS: [&str; 3] = ["ffmpeg_source", "vlc_source", "irl_source"];
const PLAYING_STATES: [&str; 3] = [
    "OBS_MEDIA_STATE_PLAYING",
    "OBS_MEDIA_STATE_BUFFERING",
    "OBS_MEDIA_STATE_OPENING",
];
const NETWORK_SCHEMES: [&str; 5] = ["rtmp", "srt", "udp", "rist", "rtsp"];
const MEDIA_RESTART: &str = "OBS_WEBSOCKET_MEDIA_INPUT_ACTION_RESTART";

/// How a source is made to reconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixMethod {
    /// Write empty settings back: `ffmpeg_source` and `vlc_source` reopen the
    /// stream from their update callback.
    RewriteSettings,
    /// `irl_source` only applies settings that changed, so it needs a media
    /// restart instead.
    MediaRestart,
}

/// How to make an input of `kind` with these `settings` reconnect, or `None`
/// when it doesn't read a network stream (a local file, say).
pub fn fix_method(kind: &str, settings: &Value) -> Option<FixMethod> {
    let (urls, method): (Vec<&str>, _) = match kind {
        "ffmpeg_source" => (
            settings["input"].as_str().into_iter().collect(),
            FixMethod::RewriteSettings,
        ),
        "vlc_source" => (
            settings["playlist"]
                .as_array()
                .map(|files| {
                    files
                        .iter()
                        .filter_map(|file| file["value"].as_str())
                        .collect()
                })
                .unwrap_or_default(),
            FixMethod::RewriteSettings,
        ),
        "irl_source" => (
            settings["url"].as_str().into_iter().collect(),
            FixMethod::MediaRestart,
        ),
        _ => return None,
    };
    urls.iter()
        .any(|url| is_network_stream(url))
        .then_some(method)
}

fn is_network_stream(url: &str) -> bool {
    let url = url.to_lowercase();
    NETWORK_SCHEMES.iter().any(|scheme| url.starts_with(scheme))
}

/// What [`ObsClient::fix_media_sources`] did.
#[derive(Debug, Default)]
pub struct FixOutcome {
    /// The scene that was on air.
    pub scene: String,
    /// The sources told to reconnect.
    pub fixed: Vec<String>,
    /// The sources OBS refused to fix (or didn't answer for).
    pub failed: Vec<(String, Error)>,
}

impl<CT: ClientTransport> ObsClient<CT> {
    /// Makes the network media sources playing on the scene on air reconnect.
    /// One source failing doesn't stop the others.
    pub async fn fix_media_sources(&self) -> Result<FixOutcome> {
        let scene = self.scene_list().await?.current_program_scene_name;
        let playing = self.playing_media_inputs(&scene).await?;
        let mut outcome = FixOutcome {
            scene,
            ..FixOutcome::default()
        };
        for (name, kind) in playing {
            match self.fix_input(&name, &kind).await {
                Ok(true) => outcome.fixed.push(name),
                Ok(false) => {}
                Err(error) => outcome.failed.push((name, error)),
            }
        }
        Ok(outcome)
    }

    /// The media inputs (name, kind) playing in `scene`, its nested scenes and
    /// its groups, each once.
    async fn playing_media_inputs(&self, scene: &str) -> Result<Vec<(String, String)>> {
        let mut found: Vec<(String, String)> = Vec::new();
        for item in self.scene_tree(scene).await? {
            let Some(kind) = item
                .input_kind
                .filter(|kind| MEDIA_KINDS.contains(&kind.as_str()))
            else {
                continue;
            };
            if found.iter().any(|(name, _)| *name == item.name) {
                continue;
            }
            let status = self
                .send_request::<GetMediaInputStatus>(RequestKind::GetMediaInputStatus {
                    input_name: &item.name,
                })
                .await;
            if status.is_ok_and(|status| PLAYING_STATES.contains(&status.media_state.as_str())) {
                found.push((item.name, kind));
            }
        }
        Ok(found)
    }

    /// Makes one input reconnect; `false` if it doesn't read a network stream.
    async fn fix_input(&self, name: &str, kind: &str) -> Result<bool> {
        let settings: GetInputSettings = self
            .send_request(RequestKind::GetInputSettings { input_name: name })
            .await?;
        let Some(method) = fix_method(kind, &settings.input_settings) else {
            return Ok(false);
        };
        match method {
            FixMethod::RewriteSettings => {
                self.command(RequestKind::SetInputSettings {
                    input_name: name,
                    input_settings: serde_json::json!({}),
                })
                .await?
            }
            FixMethod::MediaRestart => {
                self.command(RequestKind::TriggerMediaInputAction {
                    input_name: name,
                    media_action: MEDIA_RESTART,
                })
                .await?
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{FixMethod, fix_method};
    use crate::fake::{client, input, scene};

    #[test]
    fn only_network_streams_are_fixed() {
        let ffmpeg = |input: &str| fix_method("ffmpeg_source", &json!({ "input": input }));
        assert_eq!(
            ffmpeg("srt://127.0.0.1:4000"),
            Some(FixMethod::RewriteSettings)
        );
        assert_eq!(
            ffmpeg("RTMPS://ingest/live"),
            Some(FixMethod::RewriteSettings)
        );
        assert_eq!(ffmpeg("https://example.com/video.mp4"), None);
        assert_eq!(
            fix_method("ffmpeg_source", &json!({ "local_file": "a.mp4" })),
            None
        );

        let vlc =
            json!({ "playlist": [{ "value": "C:/intro.mp4" }, { "value": "rist://1.2.3.4" }] });
        assert_eq!(
            fix_method("vlc_source", &vlc),
            Some(FixMethod::RewriteSettings)
        );
        assert_eq!(
            fix_method("irl_source", &json!({ "url": "udp://0.0.0.0:9000" })),
            Some(FixMethod::MediaRestart)
        );
        assert_eq!(
            fix_method("browser_source", &json!({ "url": "rtmp://x" })),
            None
        );
    }

    fn obs(kind: &str, data: &Value) -> Value {
        let name = |key: &str| data[key].as_str().unwrap_or_default().to_owned();
        match kind {
            "GetSceneList" => json!({
                "currentProgramSceneName": "LIVE",
                "currentPreviewSceneName": null,
                "scenes": [],
            }),
            "GetSceneItemList" | "GetGroupSceneItemList" => {
                let items = match (kind, name("sceneName").as_str()) {
                    ("GetSceneItemList", "LIVE") => vec![
                        input(1, "SRT", "ffmpeg_source", true),
                        input(2, "Intro", "ffmpeg_source", true),
                        input(3, "Chat", "browser_source", true),
                        scene(4, "Cams", false, true),
                        scene(5, "Overlay", true, true),
                    ],
                    ("GetSceneItemList", "Cams") => vec![
                        input(1, "IRL", "irl_source", true),
                        input(2, "Ended", "ffmpeg_source", true),
                    ],
                    ("GetGroupSceneItemList", "Overlay") => vec![
                        input(1, "Playlist", "vlc_source", true),
                        input(2, "SRT", "ffmpeg_source", true),
                    ],
                    _ => vec![],
                };
                json!({ "sceneItems": items })
            }
            "GetMediaInputStatus" => {
                let state = if name("inputName") == "Ended" {
                    "OBS_MEDIA_STATE_ENDED"
                } else {
                    "OBS_MEDIA_STATE_PLAYING"
                };
                json!({ "mediaState": state })
            }
            "GetInputSettings" => {
                let settings = match name("inputName").as_str() {
                    "SRT" | "Ended" => json!({ "input": "srt://ingest:4000" }),
                    "Intro" => json!({ "local_file": "intro.mp4" }),
                    "IRL" => json!({ "url": "srt://ingest:5000" }),
                    "Playlist" => json!({ "playlist": [{ "value": "rtmp://ingest/live" }] }),
                    _ => json!({}),
                };
                json!({ "inputSettings": settings, "inputKind": "" })
            }
            _ => Value::Null,
        }
    }

    #[tokio::test]
    async fn fixes_the_network_sources_playing_anywhere_on_the_scene_on_air() {
        let obs = client(obs);
        let outcome = obs.fix_media_sources().await.unwrap();

        assert_eq!(outcome.scene, "LIVE");
        let mut fixed = outcome.fixed.clone();
        fixed.sort();
        assert_eq!(fixed, ["IRL", "Playlist", "SRT"]);
        assert!(outcome.failed.is_empty());

        let mut changes = obs.asked(&["SetInputSettings", "TriggerMediaInputAction"]);
        changes.sort_by_key(|(_, data)| data["inputName"].as_str().unwrap().to_owned());
        assert_eq!(
            changes,
            [
                (
                    "TriggerMediaInputAction".to_owned(),
                    json!({ "inputName": "IRL", "mediaAction": "OBS_WEBSOCKET_MEDIA_INPUT_ACTION_RESTART" })
                ),
                (
                    "SetInputSettings".to_owned(),
                    json!({ "inputName": "Playlist", "inputSettings": {} })
                ),
                (
                    "SetInputSettings".to_owned(),
                    json!({ "inputName": "SRT", "inputSettings": {} })
                ),
            ]
        );
    }
}
