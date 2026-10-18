//! OBS's Audio Mixer: the inputs with audio that are on air, and muting them.

use std::collections::HashSet;

use futures_util::future::join_all;

use crate::{
    ClientTransport, ObsClient,
    error::{Error, Result},
    sources::SourceItem,
    types::{GetInputMute, GetInputVolume, GetSpecialInputs, RequestKind},
};

/// One strip of the mixer.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInput {
    pub name: String,
    /// Not known for the global audio devices, nor on OBS versions before 30.
    pub uuid: Option<uuid::Uuid>,
    /// The input's kind (e.g. `wasapi_output_capture`); `None` for the global devices.
    pub input_kind: Option<String>,
    /// A global audio device (Settings → Audio), on air on every scene.
    pub global: bool,
    pub muted: bool,
    /// The fader, in dB (-100 all the way down).
    pub volume_db: f64,
}

impl<CT: ClientTransport> ObsClient<CT> {
    /// What OBS's Audio Mixer shows for the scene on air: the global audio
    /// devices, then the inputs with audio that are shown on the scene (nested
    /// scenes and groups included, top first), each once.
    pub async fn audio_mixer(&self, tree: &[SourceItem]) -> Result<Vec<AudioInput>> {
        let special: GetSpecialInputs = self.send_request(RequestKind::GetSpecialInputs).await?;
        let globals = [
            special.desktop1,
            special.desktop2,
            special.mic1,
            special.mic2,
            special.mic3,
            special.mic4,
        ];
        let mut seen = HashSet::new();
        let candidates: Vec<_> = globals
            .into_iter()
            .flatten()
            .map(|name| (name, None, None, true))
            .chain(
                on_air(tree)
                    .map(|item| (item.name.clone(), item.uuid, item.input_kind.clone(), false)),
            )
            .filter(|(name, ..)| seen.insert(name.clone()))
            .collect();

        let levels = join_all(candidates.iter().map(|(name, ..)| async move {
            let muted: Result<GetInputMute> = self
                .send_request(RequestKind::GetInputMute { input_name: name })
                .await;
            let volume: Result<GetInputVolume> = self
                .send_request(RequestKind::GetInputVolume { input_name: name })
                .await;
            Ok::<_, Error>((muted?.input_muted, volume?.input_volume_db))
        }))
        .await;

        let mut inputs = Vec::new();
        for ((name, uuid, input_kind, global), result) in candidates.into_iter().zip(levels) {
            match result {
                Ok((muted, volume_db)) => inputs.push(AudioInput {
                    name,
                    uuid,
                    input_kind,
                    global,
                    muted,
                    volume_db,
                }),
                // No audio (a camera without a microphone, an image, …).
                Err(Error::ObsResponseError { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(inputs)
    }

    pub async fn input_muted(&self, input: &str) -> Result<bool> {
        let mute: GetInputMute = self
            .send_request(RequestKind::GetInputMute { input_name: input })
            .await?;
        Ok(mute.input_muted)
    }

    /// Mutes (`true`) or unmutes an input.
    pub async fn set_input_muted(&self, input: &str, muted: bool) -> Result<()> {
        self.command(RequestKind::SetInputMute {
            input_name: input,
            input_muted: muted,
        })
        .await
    }

    /// Moves an input's fader, in dB (-100 is all the way down).
    pub async fn set_input_volume(&self, input: &str, volume_db: f64) -> Result<()> {
        self.command(RequestKind::SetInputVolume {
            input_name: input,
            input_volume_db: volume_db,
        })
        .await
    }
}

/// The inputs of a scene tree that are on air: shown, in a scene or group that
/// is shown too.
fn on_air(tree: &[SourceItem]) -> impl Iterator<Item = &SourceItem> {
    // Whether the last item seen at each depth is on air.
    let mut shown: Vec<bool> = Vec::new();
    tree.iter().filter(move |item| {
        let depth = item.depth as usize;
        let parent_shown = depth == 0 || shown.get(depth - 1).copied().unwrap_or(false);
        let on_air = parent_shown && item.enabled;
        shown.truncate(depth);
        shown.push(on_air);
        on_air && item.input_kind.is_some()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::fake::{client, failure, input, scene};

    fn obs(kind: &str, data: &Value) -> Value {
        let name = data["inputName"].as_str().unwrap_or_default();
        match kind {
            "GetSpecialInputs" => json!({
                "desktop1": "Desktop Audio", "desktop2": null,
                "mic1": "Mic/Aux", "mic2": null, "mic3": null, "mic4": null,
            }),
            "GetSceneItemList" => match data["sceneName"].as_str().unwrap_or_default() {
                "LIVE" => json!({ "sceneItems": [
                    input(1, "Camera", "dshow_input", true),
                    input(2, "Hidden music", "ffmpeg_source", false),
                    scene(3, "Feed", false, true),
                ] }),
                "Feed" => json!({ "sceneItems": [
                    input(1, "Overlay", "image_source", true),
                    input(2, "BELABOX", "ffmpeg_source", true),
                    // Also a global device: listed once.
                    input(3, "Mic/Aux", "wasapi_input_capture", true),
                ] }),
                _ => json!({ "sceneItems": [] }),
            },
            "GetInputMute" | "GetInputVolume" if name == "Overlay" => failure(604),
            "GetInputMute" => json!({ "inputMuted": name == "Desktop Audio" }),
            "GetInputVolume" => json!({ "inputVolumeMul": 1.0, "inputVolumeDb": -3.5 }),
            _ => json!({}),
        }
    }

    #[tokio::test]
    async fn the_mixer_lists_global_devices_then_audio_on_air() {
        let obs = client(obs);
        let tree = obs.scene_tree("LIVE").await.unwrap();
        let mixer = obs.audio_mixer(&tree).await.unwrap();
        let summary: Vec<_> = mixer
            .iter()
            .map(|input| (input.name.as_str(), input.global, input.muted))
            .collect();
        assert_eq!(
            summary,
            [
                ("Desktop Audio", true, true),
                ("Mic/Aux", true, false),
                ("BELABOX", false, false),
                ("Camera", false, false),
            ]
        );
        assert_eq!(mixer[2].input_kind.as_deref(), Some("ffmpeg_source"));
        assert_eq!(mixer[0].volume_db, -3.5);
    }

    #[tokio::test]
    async fn inputs_are_muted_by_name() {
        let obs = client(obs);
        obs.set_input_muted("Mic/Aux", true).await.unwrap();
        assert_eq!(
            obs.asked(&["SetInputMute"]),
            [(
                "SetInputMute".to_owned(),
                json!({ "inputName": "Mic/Aux", "inputMuted": true })
            )]
        );
    }

    #[tokio::test]
    async fn faders_are_set_in_decibels() {
        let obs = client(obs);
        obs.set_input_volume("Mic/Aux", -6.5).await.unwrap();
        assert_eq!(
            obs.asked(&["SetInputVolume"]),
            [(
                "SetInputVolume".to_owned(),
                json!({ "inputName": "Mic/Aux", "inputVolumeDb": -6.5 })
            )]
        );
    }
}
