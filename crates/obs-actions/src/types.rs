use serde::{Deserialize, Serialize, de, ser::SerializeStruct};
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Debug)]
pub enum ObsMessage {
    Hello(Hello),
    Identified(Identified),
    Event(Event),
    RequestResponse(RequestResponse),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub obs_web_socket_version: String,
    pub rpc_version: u32,
    pub authentication: Option<Authentication>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identified {
    pub negotiated_rpc_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Authentication {
    pub challenge: String,
    pub salt: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "eventType", content = "eventData")]
pub enum Event {
    StreamStateChanged(StreamStateChanged),
    CurrentProgramSceneChanged(CurrentProgramSceneChanged),
    RecordStateChanged(RecordStateChanged),
    InputMuteStateChanged(InputMuteStateChanged),
    InputVolumeChanged(InputVolumeChanged),
    /// Every 50 ms, only while subscribed to [`EVENT_SUBSCRIPTION_INPUT_VOLUME_METERS`].
    InputVolumeMeters(InputVolumeMeters),

    #[serde(other, deserialize_with = "deserialize_unknown_event")]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentProgramSceneChanged {
    /// Name of the scene that was switched to
    pub scene_name: String,
    /// UUID of the scene that was switched to (not sent by very old OBS versions)
    #[serde(default)]
    pub scene_uuid: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamStateChanged {
    /// Whether the output is currently active
    pub output_active: bool,
    /// The specific state of the output
    pub output_state: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordStateChanged {
    /// Whether the output is currently active
    pub output_active: bool,
    /// The specific state of the output
    pub output_state: String,
    /// File name for the saved recording, if record stopped. `None` otherwise
    pub output_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputMuteStateChanged {
    pub input_name: String,
    pub input_muted: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputVolumeChanged {
    pub input_name: String,
    pub input_volume_db: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputVolumeMeters {
    /// Every input with audio, active or not.
    pub inputs: Vec<InputVolumeMeter>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputVolumeMeter {
    pub input_name: String,
    /// Per channel: magnitude, peak and input peak (before the volume fader),
    /// as multipliers (1.0 = 0 dBFS). Empty while the input sends no audio.
    #[serde(default)]
    pub input_levels_mul: Vec<Vec<f64>>,
}

/// The events obs-websocket sends unless asked otherwise (`All`: everything but
/// the high-volume ones).
pub const EVENT_SUBSCRIPTION_ALL: u32 = (1 << 11) - 1;
/// Scene events (`CurrentProgramSceneChanged`, …).
pub const EVENT_SUBSCRIPTION_SCENES: u32 = 1 << 2;
/// Input events (`InputMuteStateChanged`, `InputVolumeChanged`, but also
/// `InputSettingsChanged`, …).
pub const EVENT_SUBSCRIPTION_INPUTS: u32 = 1 << 3;
/// Output events (`StreamStateChanged`, `RecordStateChanged`, …).
pub const EVENT_SUBSCRIPTION_OUTPUTS: u32 = 1 << 6;
/// The high-volume `InputVolumeMeters` event.
pub const EVENT_SUBSCRIPTION_INPUT_VOLUME_METERS: u32 = 1 << 16;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestResponse {
    pub request_type: String,
    pub request_id: String,
    pub request_status: RequestStatus,
    pub response_data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestStatus {
    pub code: u32,
    pub result: bool,
    pub comment: Option<String>,
}

/// `GetSourceScreenshot`'s answer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSourceScreenshot {
    /// `data:image/jpg;base64,…`
    pub image_data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSceneList {
    /// Current program scene
    pub current_program_scene_name: String,
    /// UUID of the current program scene (not sent by very old OBS versions)
    #[serde(default)]
    pub current_program_scene_uuid: Option<uuid::Uuid>,
    /// Current preview scene. None if not in studio mode
    pub current_preview_scene_name: Option<String>,
    /// Array of scenes
    pub scenes: Vec<Scene>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub scene_index: usize,
    pub scene_name: String,
    /// Not sent by very old OBS versions.
    #[serde(default)]
    pub scene_uuid: Option<uuid::Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetStreamStatus {
    /// Whether the output is active
    pub output_active: bool,
    /// Whether the output is currently reconnecting
    pub output_reconnecting: bool,
    /// Current formatted timecode string for the output
    pub output_timecode: String,
    /// Current duration in milliseconds for the output
    pub output_duration: u32,
    /// Congestion of the output
    pub output_congestion: f32,
    /// Number of bytes sent by the output
    pub output_bytes: u64,
    /// Number of frames skipped by the output's process
    pub output_skipped_frames: u32,
    /// Total number of frames delivered by the output's process
    pub output_total_frames: u32,
}

/// `GetStats`: OBS's performance numbers, as its Stats dock shows them. The
/// frame counts run since OBS started.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetStats {
    /// Percent of the CPU OBS uses.
    pub cpu_usage: f64,
    /// MB of memory OBS uses.
    pub memory_usage: f64,
    /// MB free on the drive of the recording path.
    pub available_disk_space: f64,
    /// The frame rate OBS renders at right now.
    pub active_fps: f64,
    /// ms per frame.
    pub average_frame_render_time: f64,
    /// Frames missed due to rendering lag.
    pub render_skipped_frames: u64,
    pub render_total_frames: u64,
    /// Frames skipped due to encoding lag.
    pub output_skipped_frames: u64,
    pub output_total_frames: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRecordStatus {
    /// Whether the output is active
    pub output_active: bool,
    /// Whether the output is paused
    pub output_paused: bool,
    /// Current formatted timecode string for the output
    pub output_timecode: String,
    /// Current duration in milliseconds for the output
    pub output_duration: u32,
    /// Number of bytes sent by the output
    pub output_bytes: u64,
}

/// `GetSpecialInputs`' answer: the global audio devices (Settings → Audio), by
/// name, `None` when disabled.
#[derive(Debug, Deserialize)]
pub struct GetSpecialInputs {
    pub desktop1: Option<String>,
    pub desktop2: Option<String>,
    pub mic1: Option<String>,
    pub mic2: Option<String>,
    pub mic3: Option<String>,
    pub mic4: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetInputMute {
    pub input_muted: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetInputVolume {
    /// -100 when the fader is all the way down.
    pub input_volume_db: f64,
}

/// `GetSceneItemList` / `GetGroupSceneItemList`'s answer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSceneItemList {
    pub scene_items: Vec<SceneItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneItem {
    /// Unique within its scene or group.
    pub scene_item_id: i64,
    /// Whether it shows (the eye in OBS's Sources dock).
    pub scene_item_enabled: bool,
    pub source_name: String,
    /// Not sent by OBS versions before 30.
    #[serde(default)]
    pub source_uuid: Option<uuid::Uuid>,
    /// `OBS_SOURCE_TYPE_INPUT`, `OBS_SOURCE_TYPE_SCENE` (groups too), …
    pub source_type: String,
    /// The input's kind (e.g. `ffmpeg_source`); `None` for scenes and groups.
    #[serde(default)]
    pub input_kind: Option<String>,
    /// Whether a scene item is a group; `None` for inputs.
    #[serde(default)]
    pub is_group: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetMediaInputStatus {
    /// `OBS_MEDIA_STATE_PLAYING`, `OBS_MEDIA_STATE_OPENING`, …
    pub media_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetInputSettings {
    /// Only what differs from the kind's defaults.
    pub input_settings: serde_json::Value,
}

impl<'de> Deserialize<'de> for ObsMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize_repr)]
        #[repr(u8)]
        enum OpCode {
            Hello = 0,
            Identified = 2,
            Event = 5,
            RequestResponse = 7,
        }

        #[derive(Deserialize)]
        struct Raw {
            op: OpCode,
            d: serde_json::Value,
        }

        let raw = Raw::deserialize(deserializer)?;

        Ok(match raw.op {
            OpCode::Hello => {
                ObsMessage::Hello(serde_json::from_value(raw.d).map_err(de::Error::custom)?)
            }
            OpCode::Identified => {
                ObsMessage::Identified(serde_json::from_value(raw.d).map_err(de::Error::custom)?)
            }
            OpCode::Event => {
                ObsMessage::Event(serde_json::from_value(raw.d).map_err(de::Error::custom)?)
            }
            OpCode::RequestResponse => ObsMessage::RequestResponse(
                serde_json::from_value(raw.d).map_err(de::Error::custom)?,
            ),
        })
    }
}

// FIXME: This shouldn't be needed but serde is throwing an error when the unknown variant also
// contains data
fn deserialize_unknown_event<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_any(de::IgnoredAny)?;
    Ok(())
}

#[derive(Debug)]
pub enum ObsRequest<'a> {
    Identify(Identify),
    /// Changes the event subscriptions of an identified session.
    Reidentify(Reidentify),
    Request(Request<'a>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reidentify {
    pub event_subscriptions: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identify {
    pub rpc_version: u32,
    pub authentication: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_subscriptions: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request<'a> {
    #[serde(rename = "requestId")]
    pub id: &'a str,
    #[serde(flatten)]
    pub kind: RequestKind<'a>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "requestType", content = "requestData")]
pub enum RequestKind<'a> {
    SetCurrentProgramScene {
        #[serde(rename = "sceneName")]
        scene_name: &'a str,
    },
    #[serde(rename = "SetCurrentProgramScene")]
    SetCurrentProgramSceneByUuid {
        #[serde(rename = "sceneUuid")]
        scene_uuid: uuid::Uuid,
    },
    GetSceneList,
    GetSceneItemList {
        #[serde(rename = "sceneName")]
        scene_name: &'a str,
    },
    GetGroupSceneItemList {
        #[serde(rename = "sceneName")]
        scene_name: &'a str,
    },
    /// `scene_name` may be a group.
    #[serde(rename_all = "camelCase")]
    SetSceneItemEnabled {
        scene_name: &'a str,
        scene_item_id: i64,
        scene_item_enabled: bool,
    },
    GetSpecialInputs,
    GetInputMute {
        #[serde(rename = "inputName")]
        input_name: &'a str,
    },
    #[serde(rename_all = "camelCase")]
    SetInputMute {
        input_name: &'a str,
        input_muted: bool,
    },
    GetInputVolume {
        #[serde(rename = "inputName")]
        input_name: &'a str,
    },
    /// -100 to 26 dB (OBS's own fader stops at 0).
    #[serde(rename_all = "camelCase")]
    SetInputVolume {
        input_name: &'a str,
        input_volume_db: f64,
    },
    GetMediaInputStatus {
        #[serde(rename = "inputName")]
        input_name: &'a str,
    },
    GetInputSettings {
        #[serde(rename = "inputName")]
        input_name: &'a str,
    },
    /// Merged into the input's settings (`overlay` defaults to true).
    #[serde(rename_all = "camelCase")]
    SetInputSettings {
        input_name: &'a str,
        input_settings: serde_json::Value,
    },
    /// `media_action` is e.g. `OBS_WEBSOCKET_MEDIA_INPUT_ACTION_RESTART`.
    #[serde(rename_all = "camelCase")]
    TriggerMediaInputAction {
        input_name: &'a str,
        media_action: &'a str,
    },
    GetStats,
    GetStreamStatus,
    GetRecordStatus,
    StartStream,
    StopStream,
    StartRecord,
    StopRecord,
    PauseRecord,
    ResumeRecord,
    /// A still of a source (a scene is a source), as a `data:` URI.
    #[serde(rename_all = "camelCase")]
    GetSourceScreenshot {
        #[serde(skip_serializing_if = "Option::is_none")]
        source_name: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        source_uuid: Option<uuid::Uuid>,
        /// `jpg`, `png`, … (see `GetVersion`'s `supportedImageFormats`).
        image_format: &'a str,
        /// Scale to this width; the height follows, keeping the aspect ratio.
        #[serde(skip_serializing_if = "Option::is_none")]
        image_width: Option<u32>,
        /// 0 (smallest, blockiest) to 100 (best); for PNG it's the compression
        /// level instead. -1 is OBS's default.
        #[serde(skip_serializing_if = "Option::is_none")]
        image_compression_quality: Option<i32>,
    },
}

impl Serialize for ObsRequest<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize_repr)]
        #[repr(u8)]
        enum OpCode {
            Identify = 1,
            Reidentify = 3,
            Request = 6,
        }

        match self {
            ObsRequest::Identify(i) => {
                let mut state = serializer.serialize_struct("ObsRequest", 2)?;
                state.serialize_field("op", &OpCode::Identify)?;
                state.serialize_field("d", &i)?;
                state.end()
            }
            ObsRequest::Reidentify(r) => {
                let mut state = serializer.serialize_struct("ObsRequest", 2)?;
                state.serialize_field("op", &OpCode::Reidentify)?;
                state.serialize_field("d", &r)?;
                state.end()
            }
            ObsRequest::Request(r) => {
                let mut state = serializer.serialize_struct("ObsRequest", 2)?;
                state.serialize_field("op", &OpCode::Request)?;
                state.serialize_field("d", &r)?;
                state.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ObsRequest, Request, RequestKind};

    #[test]
    fn switching_by_uuid_uses_the_scene_uuid_field() {
        let uuid: uuid::Uuid = "3f1c7b0e-6a51-4c1f-9b53-2c0d1b8a9e10".parse().unwrap();
        let request = ObsRequest::Request(Request {
            id: "1",
            kind: RequestKind::SetCurrentProgramSceneByUuid { scene_uuid: uuid },
        });

        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "op": 6,
                "d": {
                    "requestId": "1",
                    "requestType": "SetCurrentProgramScene",
                    "requestData": { "sceneUuid": "3f1c7b0e-6a51-4c1f-9b53-2c0d1b8a9e10" }
                }
            })
        );
    }

    #[test]
    fn reidentify_asks_for_volume_meters_on_top_of_the_default_events() {
        let request = ObsRequest::Reidentify(super::Reidentify {
            event_subscriptions: super::EVENT_SUBSCRIPTION_ALL
                | super::EVENT_SUBSCRIPTION_INPUT_VOLUME_METERS,
        });
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({ "op": 3, "d": { "eventSubscriptions": 67583 } })
        );
    }

    #[test]
    fn volume_meters_parse() {
        let message: super::ObsMessage = serde_json::from_value(serde_json::json!({
            "op": 5,
            "d": {
                "eventType": "InputVolumeMeters",
                "eventIntent": 65536,
                "eventData": { "inputs": [
                    { "inputName": "Mic", "inputUuid": "3f1c7b0e-6a51-4c1f-9b53-2c0d1b8a9e10",
                      "inputLevelsMul": [[0.1, 0.5, 0.5], [0.2, 0.4, 0.4]] },
                    { "inputName": "Silent", "inputLevelsMul": [] }
                ] }
            }
        }))
        .unwrap();
        let super::ObsMessage::Event(super::Event::InputVolumeMeters(meters)) = message else {
            panic!("not a meter event: {message:?}");
        };
        assert_eq!(meters.inputs[0].input_levels_mul[1], [0.2, 0.4, 0.4]);
        assert!(meters.inputs[1].input_levels_mul.is_empty());
    }

    #[test]
    fn scene_lists_carry_uuids() {
        let list: super::GetSceneList = serde_json::from_value(serde_json::json!({
            "currentProgramSceneName": "LIVE",
            "currentProgramSceneUuid": "3f1c7b0e-6a51-4c1f-9b53-2c0d1b8a9e10",
            "currentPreviewSceneName": null,
            "scenes": [{ "sceneIndex": 0, "sceneName": "LIVE", "sceneUuid": "3f1c7b0e-6a51-4c1f-9b53-2c0d1b8a9e10" }]
        }))
        .unwrap();

        assert!(list.current_program_scene_uuid.is_some());
        assert_eq!(list.scenes[0].scene_uuid, list.current_program_scene_uuid);
    }
}
