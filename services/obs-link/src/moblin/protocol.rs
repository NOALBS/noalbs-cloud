//! Moblin's remote control protocol (`Moblin/RemoteControl/RemoteControl.swift`),
//! seen from the assistant's side: what we send is built here, and what arrives
//! is folded into a [`State`]. Pure, so it's tested without a socket.
//!
//! Moblin encodes its messages with Swift's synthesized `Codable`: every enum
//! case is an object with one key, the case name, holding its labelled fields
//! (`{"request":{"id":1,"data":{"getStatus":{}}}}`; a case without fields is
//! `{"x":{}}`, a missing optional field is left out). UUIDs are uppercase
//! strings, which we keep as they come and send back as they were.
//!
//! Moblin pushes `state` events when something changes, often with only the
//! field that changed, and answers `getStatus` (battery, heat, bitrate text) and
//! `getSettings` (scenes, bitrate presets), which we ask for regularly.

use base64::{Engine, engine::general_purpose::STANDARD};
use live::{
    MOBLIN_MAX_PRIORITY, MOBLIN_MAX_ZOOM, MOBLIN_MIN_PRIORITY, MOBLIN_MIN_ZOOM, MoblinAction,
    MoblinBitratePreset, MoblinConnection, MoblinRefusal, MoblinScene, MoblinStatus,
    MoblinStatusText, MoblinThermal, MoblinZoomPreset,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// The remote control API version Moblin speaks.
const API_VERSION: &str = "0.1";

/// What Moblin should answer to prove it knows the password:
/// `base64(sha256(base64(sha256(password + salt)) + challenge))`.
pub fn hash_password(challenge: &str, salt: &str, password: &str) -> String {
    let inner = STANDARD.encode(Sha256::digest(format!("{password}{salt}")));
    STANDARD.encode(Sha256::digest(format!("{inner}{challenge}")))
}

/// The first message, with the challenge Moblin answers by identifying.
pub fn hello(challenge: &str, salt: &str) -> String {
    json!({
        "hello": {
            "apiVersion": API_VERSION,
            "authentication": { "challenge": challenge, "salt": salt },
        }
    })
    .to_string()
}

/// The answer to `identify`.
pub fn identified(ok: bool) -> String {
    let result = if ok { "ok" } else { "wrongPassword" };
    let mut results = Map::new();
    results.insert(result.to_owned(), json!({}));
    json!({ "identified": { "result": results } }).to_string()
}

pub fn pong() -> String {
    json!({ "pong": {} }).to_string()
}

/// A request to Moblin.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    GetStatus,
    GetSettings,
    SetLive(bool),
    SetRecord(bool),
    SetMute(bool),
    SetBitratePreset(String),
    SetScene(String),
    SetZoom(f64),
    SetZoomPreset(String),
    SetTorch(bool),
    SetStealthMode(bool),
    SetSrtConnectionPrioritiesEnabled(bool),
    SetSrtConnectionPriority {
        id: String,
        priority: u8,
        enabled: bool,
    },
}

impl Request {
    pub fn to_message(&self, id: u64) -> String {
        let data = match self {
            Self::GetStatus => json!({ "getStatus": {} }),
            Self::GetSettings => json!({ "getSettings": {} }),
            Self::SetLive(on) => json!({ "setLive": { "on": on } }),
            Self::SetRecord(on) => json!({ "setRecord": { "on": on } }),
            Self::SetMute(on) => json!({ "setMute": { "on": on } }),
            Self::SetBitratePreset(id) => json!({ "setBitratePreset": { "id": id } }),
            Self::SetScene(id) => json!({ "setScene": { "id": id } }),
            Self::SetZoom(x) => json!({ "setZoom": { "x": x } }),
            Self::SetZoomPreset(id) => json!({ "setZoomPreset": { "id": id } }),
            Self::SetTorch(on) => json!({ "setTorch": { "on": on } }),
            Self::SetStealthMode(on) => json!({ "setStealthMode": { "on": on } }),
            Self::SetSrtConnectionPrioritiesEnabled(enabled) => {
                json!({ "setSrtConnectionPrioritiesEnabled": { "enabled": enabled } })
            }
            Self::SetSrtConnectionPriority {
                id,
                priority,
                enabled,
            } => json!({
                "setSrtConnectionPriority": { "id": id, "priority": priority, "enabled": enabled }
            }),
        };
        json!({ "request": { "id": id, "data": data } }).to_string()
    }
}

/// A message from Moblin.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Identify {
        authentication: String,
    },
    /// The answer to a request: `ok` and its data, if any.
    Response {
        id: u64,
        ok: bool,
        data: Option<Value>,
    },
    /// An event's `data` (`{"state":{…}}`, `{"log":{…}}`, …).
    Event(Value),
    Ping,
    /// Anything else (`preview`, `twitchStart`, …), which we ignore. The Twitch
    /// token in `twitchStart` is never read.
    Other(String),
}

/// Reads a message from Moblin; `None` if it isn't one.
pub fn parse(text: &str) -> Option<Incoming> {
    let (kind, body) = single(serde_json::from_str(text).ok()?)?;
    Some(match kind.as_str() {
        "identify" => Incoming::Identify {
            authentication: body.get("authentication")?.as_str()?.to_owned(),
        },
        "response" => {
            let (result, _) = single(body.get("result")?.clone())?;
            Incoming::Response {
                id: body.get("id")?.as_u64()?,
                ok: result == "ok",
                data: body.get("data").cloned(),
            }
        }
        "event" => Incoming::Event(body.get("data")?.clone()),
        "ping" => Incoming::Ping,
        _ => Incoming::Other(kind),
    })
}

/// The case name and fields of a Swift enum value.
fn single(value: Value) -> Option<(String, Value)> {
    let Value::Object(map) = value else {
        return None;
    };
    let mut entries = map.into_iter();
    let (key, value) = entries.next()?;
    entries.next().is_none().then_some((key, value))
}

/// Sets `field` to `value`, if Moblin said; a change from a known value is
/// news, the first reading isn't.
fn set_flag(
    field: &mut Option<bool>,
    value: Option<bool>,
    change: fn(bool) -> Change,
    changes: &mut Vec<Change>,
) {
    if let Some(value) = value
        && field.replace(value).is_some_and(|old| old != value)
    {
        changes.push(change(value));
    }
}

/// Something the activity feed logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Streaming(bool),
    Recording(bool),
    Muted(bool),
    /// The scene on air, by name.
    Scene(String),
    /// The bitrate preset in use, kbps.
    BitratePreset(u32),
    /// A zoom preset picked, by name.
    ZoomPreset(String),
    Torch(bool),
    StealthMode(bool),
    ConnectionPriorities(bool),
    /// A connection turned on or off or got another priority.
    Connection {
        name: String,
        enabled: bool,
        priority: u8,
    },
}

/// What we know about the connected Moblin.
#[derive(Debug, Clone, Default)]
pub struct State {
    streaming: Option<bool>,
    recording: Option<bool>,
    muted: Option<bool>,
    scene_id: Option<String>,
    bitrate_preset_id: Option<String>,
    battery_level: Option<u8>,
    battery_charging: Option<bool>,
    thermal: Option<MoblinThermal>,
    bitrate: Option<String>,
    bonding: Option<String>,
    bonding_rtts: Option<String>,
    uptime: Option<String>,
    scenes: Vec<MoblinScene>,
    bitrate_presets: Vec<MoblinBitratePreset>,
    torch: Option<bool>,
    stealth_mode: Option<bool>,
    system_monitor: Option<String>,
    moblink: Option<MoblinStatusText>,
    dji_devices: Option<MoblinStatusText>,
    /// `None` until the first `getSettings` answer.
    connection_priorities_enabled: Option<bool>,
    connections: Vec<MoblinConnection>,
    zoom: Option<f64>,
    /// `Some(false)` when Moblin's status has no zoom (the camera can't).
    zoom_available: Option<bool>,
    zoom_presets: Vec<MoblinZoomPreset>,
    zoom_preset_id: Option<String>,
}

/// `RemoteControlAssistantStreamerState`, of which Moblin often sends only the
/// field that changed.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StreamerState {
    scene: Option<String>,
    bitrate: Option<String>,
    streaming: Option<bool>,
    recording: Option<bool>,
    muted: Option<bool>,
    torch_on: Option<bool>,
    stealth_mode: Option<bool>,
    zoom: Option<f64>,
    zoom_presets: Option<Vec<Named>>,
    zoom_preset: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StatusGeneral {
    battery_charging: Option<bool>,
    battery_level: Option<i64>,
    flame: Option<String>,
    is_live: Option<bool>,
    is_recording: Option<bool>,
    is_muted: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct StatusItem {
    message: String,
    #[serde(default = "yes")]
    ok: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StatusTopRight {
    bitrate: Option<StatusItem>,
    uptime: Option<StatusItem>,
    srtla: Option<StatusItem>,
    srtla_rtts: Option<StatusItem>,
    system_monitor: Option<StatusItem>,
    moblink: Option<StatusItem>,
    dji_devices: Option<StatusItem>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct StatusTopLeft {
    zoom: Option<StatusItem>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Status {
    general: Option<StatusGeneral>,
    top_left: Option<StatusTopLeft>,
    top_right: Option<StatusTopRight>,
}

#[derive(Debug, Deserialize)]
struct Named {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct Preset {
    id: String,
    /// bits per second
    bitrate: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    scenes: Vec<Named>,
    bitrate_presets: Vec<Preset>,
    srt: Option<Srt>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Srt {
    connection_priorities_enabled: bool,
    connection_priorities: Vec<Priority>,
}

#[derive(Debug, Deserialize)]
struct Priority {
    id: String,
    name: String,
    priority: i64,
    enabled: bool,
}

impl State {
    /// Folds an event's data in.
    pub fn apply_event(&mut self, data: &Value) -> Vec<Change> {
        let Some((kind, body)) = single(data.clone()) else {
            return Vec::new();
        };
        if kind != "state" {
            return Vec::new();
        }
        let Some(state) = body
            .get("data")
            .and_then(|state| StreamerState::deserialize(state).ok())
        else {
            return Vec::new();
        };

        let mut changes = Vec::new();
        self.set_flags(state.streaming, state.recording, state.muted, &mut changes);
        set_flag(&mut self.torch, state.torch_on, Change::Torch, &mut changes);
        set_flag(
            &mut self.stealth_mode,
            state.stealth_mode,
            Change::StealthMode,
            &mut changes,
        );
        if let Some(x) = state.zoom {
            self.zoom = Some(x);
        }
        if let Some(presets) = state.zoom_presets {
            self.zoom_presets = presets
                .into_iter()
                .map(|preset| MoblinZoomPreset {
                    id: preset.id,
                    name: preset.name,
                })
                .collect();
        }
        // Moblin reports an ID outside the list when the zoom is no preset.
        if let Some(preset) = state.zoom_preset
            && self
                .zoom_preset_id
                .replace(preset.clone())
                .is_some_and(|old| old != preset)
            && let Some(name) = self.zoom_preset_name(&preset)
        {
            changes.push(Change::ZoomPreset(name.to_owned()));
        }
        if let Some(scene) = state.scene
            && self
                .scene_id
                .replace(scene.clone())
                .is_some_and(|old| old != scene)
            && let Some(name) = self.scene_name(&scene)
        {
            changes.push(Change::Scene(name.to_owned()));
        }
        if let Some(preset) = state.bitrate
            && self
                .bitrate_preset_id
                .replace(preset.clone())
                .is_some_and(|old| old != preset)
            && let Some(kbps) = self.preset_kbps(&preset)
        {
            changes.push(Change::BitratePreset(kbps));
        }
        changes
    }

    /// Folds in the answer to `getStatus` (`{"getStatus":{…}}`).
    pub fn apply_status(&mut self, data: &Value) -> Vec<Change> {
        let Some(status) = single(data.clone())
            .filter(|(kind, _)| kind == "getStatus")
            .and_then(|(_, body)| Status::deserialize(body).ok())
        else {
            return Vec::new();
        };

        let mut changes = Vec::new();
        if let Some(general) = status.general {
            self.battery_level = general
                .battery_level
                .and_then(|level| u8::try_from(level.clamp(0, 100)).ok());
            self.battery_charging = general.battery_charging;
            self.thermal = general.flame.as_deref().map(|flame| match flame {
                "Red" => MoblinThermal::Critical,
                "Yellow" => MoblinThermal::Serious,
                _ => MoblinThermal::Normal,
            });
            self.set_flags(
                general.is_live,
                general.is_recording,
                general.is_muted,
                &mut changes,
            );
        }
        if let Some(top_left) = status.top_left {
            self.zoom_available = Some(top_left.zoom.is_some());
        }
        if let Some(top_right) = status.top_right {
            let message = |item: Option<StatusItem>| item.map(|item| item.message);
            let text = |item: Option<StatusItem>| {
                item.map(|item| MoblinStatusText {
                    text: item.message,
                    ok: item.ok,
                })
            };
            self.bitrate = message(top_right.bitrate);
            self.uptime = message(top_right.uptime);
            self.bonding = message(top_right.srtla);
            self.bonding_rtts = message(top_right.srtla_rtts);
            self.system_monitor = message(top_right.system_monitor);
            self.moblink = text(top_right.moblink);
            self.dji_devices = text(top_right.dji_devices);
        }
        changes
    }

    /// Folds in the answer to `getSettings` (`{"getSettings":{"data":{…}}}`);
    /// connection changes since the last answer are news.
    pub fn apply_settings(&mut self, data: &Value) -> Vec<Change> {
        let Some(settings) = single(data.clone())
            .filter(|(kind, _)| kind == "getSettings")
            .and_then(|(_, body)| body.get("data").cloned())
            .and_then(|settings| Settings::deserialize(settings).ok())
        else {
            return Vec::new();
        };

        let mut changes = Vec::new();
        if let Some(srt) = settings.srt {
            let connections: Vec<MoblinConnection> = srt
                .connection_priorities
                .into_iter()
                .map(|priority| MoblinConnection {
                    id: priority.id,
                    name: priority.name,
                    priority: u8::try_from(priority.priority.clamp(
                        i64::from(MOBLIN_MIN_PRIORITY),
                        i64::from(MOBLIN_MAX_PRIORITY),
                    ))
                    .unwrap_or(MOBLIN_MIN_PRIORITY),
                    enabled: priority.enabled,
                })
                .collect();
            if self.connection_priorities_enabled.is_some() {
                for connection in &connections {
                    let before = self.connections.iter().find(|old| old.id == connection.id);
                    if before.is_some_and(|old| {
                        old.enabled != connection.enabled || old.priority != connection.priority
                    }) {
                        changes.push(Change::Connection {
                            name: connection.name.clone(),
                            enabled: connection.enabled,
                            priority: connection.priority,
                        });
                    }
                }
            }
            self.connections = connections;
            set_flag(
                &mut self.connection_priorities_enabled,
                Some(srt.connection_priorities_enabled),
                Change::ConnectionPriorities,
                &mut changes,
            );
        }
        self.scenes = settings
            .scenes
            .into_iter()
            .map(|scene| MoblinScene {
                id: scene.id,
                name: scene.name,
            })
            .collect();
        self.bitrate_presets = settings
            .bitrate_presets
            .into_iter()
            .map(|preset| MoblinBitratePreset {
                id: preset.id,
                kbps: u32::try_from(preset.bitrate / 1000).unwrap_or(u32::MAX),
            })
            .collect();
        changes
    }

    fn set_flags(
        &mut self,
        streaming: Option<bool>,
        recording: Option<bool>,
        muted: Option<bool>,
        changes: &mut Vec<Change>,
    ) {
        set_flag(&mut self.streaming, streaming, Change::Streaming, changes);
        set_flag(&mut self.recording, recording, Change::Recording, changes);
        set_flag(&mut self.muted, muted, Change::Muted, changes);
    }

    fn scene_name(&self, id: &str) -> Option<&str> {
        self.scenes
            .iter()
            .find(|scene| scene.id == id)
            .map(|scene| scene.name.as_str())
    }

    fn zoom_preset_name(&self, id: &str) -> Option<&str> {
        self.zoom_presets
            .iter()
            .find(|preset| preset.id.eq_ignore_ascii_case(id))
            .map(|preset| preset.name.as_str())
    }

    fn preset_kbps(&self, id: &str) -> Option<u32> {
        self.bitrate_presets
            .iter()
            .find(|preset| preset.id == id)
            .map(|preset| preset.kbps)
    }

    pub fn streaming(&self) -> bool {
        self.streaming.unwrap_or(false)
    }

    /// What viewers see (`updated_at` and `paused` are the connection's).
    pub fn status(&self) -> MoblinStatus {
        MoblinStatus {
            streaming: self.streaming(),
            recording: self.recording.unwrap_or(false),
            muted: self.muted.unwrap_or(false),
            battery_level: self.battery_level,
            battery_charging: self.battery_charging,
            thermal: self.thermal,
            bitrate: self.bitrate.clone(),
            bonding: self.bonding.clone(),
            bonding_rtts: self.bonding_rtts.clone(),
            uptime: self.uptime.clone(),
            scene_id: self.scene_id.clone(),
            scenes: self.scenes.clone(),
            bitrate_preset_id: self.bitrate_preset_id.clone(),
            bitrate_presets: self.bitrate_presets.clone(),
            torch: self.torch.unwrap_or(false),
            stealth_mode: self.stealth_mode.unwrap_or(false),
            system_monitor: self.system_monitor.clone(),
            moblink: self.moblink.clone(),
            dji_devices: self.dji_devices.clone(),
            connection_priorities_enabled: self.connection_priorities_enabled.unwrap_or(false),
            connections: self.connections.clone(),
            zoom: self.zoom.filter(|_| self.zoom_available != Some(false)),
            zoom_presets: self.zoom_presets.clone(),
            zoom_preset_id: self
                .zoom_preset_id
                .clone()
                .filter(|id| self.zoom_preset_name(id).is_some()),
            ..MoblinStatus::default()
        }
    }

    /// The request that carries out `action`, or why it can't be done.
    pub fn prepare(&self, action: &MoblinAction) -> Result<Request, MoblinRefusal> {
        Ok(match action {
            MoblinAction::Stream { active } => {
                match (self.streaming, *active) {
                    (Some(true), true) => return Err(MoblinRefusal::AlreadyStreaming),
                    (Some(false), false) => return Err(MoblinRefusal::NotStreaming),
                    _ => {}
                }
                Request::SetLive(*active)
            }
            MoblinAction::Record { active } => Request::SetRecord(*active),
            MoblinAction::Mute { muted } => Request::SetMute(*muted),
            MoblinAction::BitratePreset { id } => {
                let preset = self.known(id, self.bitrate_presets.iter().map(|p| p.id.as_str()));
                Request::SetBitratePreset(preset.ok_or(MoblinRefusal::UnknownPreset)?)
            }
            MoblinAction::Scene { id } => {
                let scene = self.known(id, self.scenes.iter().map(|s| s.id.as_str()));
                Request::SetScene(scene.ok_or(MoblinRefusal::UnknownScene)?)
            }
            MoblinAction::Zoom { x } => {
                if !(MOBLIN_MIN_ZOOM..=MOBLIN_MAX_ZOOM).contains(x) {
                    return Err(MoblinRefusal::InvalidZoom);
                }
                Request::SetZoom(*x)
            }
            MoblinAction::ZoomPreset { id } => {
                let preset = self.known(id, self.zoom_presets.iter().map(|p| p.id.as_str()));
                Request::SetZoomPreset(preset.ok_or(MoblinRefusal::UnknownZoomPreset)?)
            }
            MoblinAction::Torch { on } => Request::SetTorch(*on),
            MoblinAction::StealthMode { on } => Request::SetStealthMode(*on),
            MoblinAction::ConnectionPriorities { enabled } => {
                Request::SetSrtConnectionPrioritiesEnabled(*enabled)
            }
            MoblinAction::Connection {
                id,
                enabled,
                priority,
            } => {
                let connection = self
                    .connections
                    .iter()
                    .find(|connection| connection.id.eq_ignore_ascii_case(id))
                    .ok_or(MoblinRefusal::UnknownConnection)?;
                let priority = priority.unwrap_or(connection.priority);
                if !(MOBLIN_MIN_PRIORITY..=MOBLIN_MAX_PRIORITY).contains(&priority) {
                    return Err(MoblinRefusal::InvalidPriority);
                }
                // With none left on, Moblin would use them all again.
                let others_on = self
                    .connections
                    .iter()
                    .any(|other| other.id != connection.id && other.enabled);
                if !enabled && !others_on {
                    return Err(MoblinRefusal::LastConnection);
                }
                Request::SetSrtConnectionPriority {
                    id: connection.id.clone(),
                    priority,
                    enabled: *enabled,
                }
            }
        })
    }

    /// What Moblin should then report for `action`, so the change is logged
    /// "by" whoever asked.
    pub fn expected_change(&self, action: &MoblinAction) -> Option<Change> {
        match action {
            MoblinAction::Stream { active } => Some(Change::Streaming(*active)),
            MoblinAction::Record { active } => Some(Change::Recording(*active)),
            MoblinAction::Mute { muted } => Some(Change::Muted(*muted)),
            MoblinAction::Zoom { .. } => None,
            MoblinAction::ZoomPreset { id } => self
                .zoom_preset_name(id)
                .map(|name| Change::ZoomPreset(name.to_owned())),
            MoblinAction::Torch { on } => Some(Change::Torch(*on)),
            MoblinAction::StealthMode { on } => Some(Change::StealthMode(*on)),
            MoblinAction::ConnectionPriorities { enabled } => {
                Some(Change::ConnectionPriorities(*enabled))
            }
            MoblinAction::Scene { id } => self
                .scenes
                .iter()
                .find(|scene| scene.id.eq_ignore_ascii_case(id))
                .map(|scene| Change::Scene(scene.name.clone())),
            MoblinAction::BitratePreset { id } => self
                .bitrate_presets
                .iter()
                .find(|preset| preset.id.eq_ignore_ascii_case(id))
                .map(|preset| Change::BitratePreset(preset.kbps)),
            MoblinAction::Connection {
                id,
                enabled,
                priority,
            } => self
                .connections
                .iter()
                .find(|connection| connection.id.eq_ignore_ascii_case(id))
                .map(|connection| Change::Connection {
                    name: connection.name.clone(),
                    enabled: *enabled,
                    priority: priority.unwrap_or(connection.priority),
                }),
        }
    }

    /// Moblin's spelling of `id`, if it's one of `ids` (the dashboard may send
    /// UUIDs in lower case).
    fn known<'a>(&self, id: &str, mut ids: impl Iterator<Item = &'a str>) -> Option<String> {
        ids.find(|known| known.eq_ignore_ascii_case(id))
            .map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE_A: &str = "5B3F1D8E-0C2B-4E0B-9C1A-8D7E6F5A4B3C";
    const SCENE_B: &str = "0F9E8D7C-6B5A-4F3E-2D1C-0B9A8F7E6D5C";
    const PRESET_LOW: &str = "11111111-2222-4333-8444-555555555555";
    const PRESET_HIGH: &str = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";

    fn settings() -> Value {
        json!({ "getSettings": { "data": {
            "streams": [{ "id": "C0FFEE00-0000-4000-8000-000000000000", "name": "Main" }],
            "scenes": [
                { "id": SCENE_A, "name": "Back camera" },
                { "id": SCENE_B, "name": "Front camera" },
            ],
            "autoSceneSwitchers": [],
            "bitratePresets": [
                { "id": PRESET_LOW, "bitrate": 1_000_000 },
                { "id": PRESET_HIGH, "bitrate": 5_000_000 },
            ],
            "mics": [{ "id": "builtin", "name": "Bottom" }],
            "srt": { "connectionPrioritiesEnabled": false, "connectionPriorities": [] },
        } } })
    }

    fn state_event(state: Value) -> Value {
        json!({ "state": { "data": state } })
    }

    #[test]
    fn password_hash_matches_moblins_formula() {
        // Computed independently with
        // `inner=$(printf 'hunter2salt123' | openssl dgst -sha256 -binary | base64)`
        // `printf '%schallenge456' "$inner" | openssl dgst -sha256 -binary | base64`.
        assert_eq!(
            hash_password("challenge456", "salt123", "hunter2"),
            "aI0prElVWyeMN25ZsAkh8ejfMb3X6Y7ijCfK2/uLqOk="
        );
        assert_eq!(
            hash_password("c", "s", "p"),
            "LEfh2WVBWpa8M06P7MehLXlToA1PtH2lNSNPjUZVYls="
        );
        assert_ne!(hash_password("c", "s", "p"), hash_password("c", "s", "q"));
    }

    #[test]
    fn messages_are_spelled_like_swift_codable() {
        assert_eq!(
            hello("C", "S"),
            r#"{"hello":{"apiVersion":"0.1","authentication":{"challenge":"C","salt":"S"}}}"#
        );
        assert_eq!(identified(true), r#"{"identified":{"result":{"ok":{}}}}"#);
        assert_eq!(
            identified(false),
            r#"{"identified":{"result":{"wrongPassword":{}}}}"#
        );
        assert_eq!(pong(), r#"{"pong":{}}"#);
        let message =
            |request: Request, id| serde_json::from_str::<Value>(&request.to_message(id)).unwrap();
        assert_eq!(
            message(Request::GetStatus, 1),
            json!({ "request": { "id": 1, "data": { "getStatus": {} } } })
        );
        assert_eq!(
            message(Request::SetLive(true), 7),
            json!({ "request": { "id": 7, "data": { "setLive": { "on": true } } } })
        );
        assert_eq!(
            message(Request::SetScene(SCENE_A.into()), 2),
            json!({ "request": { "id": 2, "data": { "setScene": { "id": SCENE_A } } } })
        );
    }

    #[test]
    fn reads_moblins_messages() {
        assert_eq!(
            parse(r#"{"identify":{"streamerId":"X","authentication":"abc="}}"#),
            Some(Incoming::Identify {
                authentication: "abc=".into()
            })
        );
        assert_eq!(parse(r#"{"ping":{}}"#), Some(Incoming::Ping));
        assert_eq!(
            parse(r#"{"response":{"id":3,"result":{"ok":{}}}}"#),
            Some(Incoming::Response {
                id: 3,
                ok: true,
                data: None
            })
        );
        assert_eq!(
            parse(r#"{"response":{"id":4,"result":{"error":{}}}}"#),
            Some(Incoming::Response {
                id: 4,
                ok: false,
                data: None
            })
        );
        assert_eq!(
            parse(r#"{"twitchStart":{"channelId":"1","accessToken":"secret"}}"#),
            Some(Incoming::Other("twitchStart".into()))
        );
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(r#"{"a":{},"b":{}}"#), None);
    }

    #[test]
    fn folds_status_settings_and_state() {
        let mut state = State::default();
        state.apply_settings(&settings());
        let status = json!({ "getStatus": {
            "general": {
                "batteryCharging": false, "batteryLevel": 87, "flame": "Yellow",
                "wiFiSsid": "Home", "isLive": true, "isRecording": false, "isMuted": false,
            },
            "topLeft": { "mic": { "message": "Bottom", "ok": true } },
            "topRight": {
                "audioInfo": { "audioLevel": { "value": { "_0": -20.5 } }, "numberOfAudioChannels": 1 },
                "bitrate": { "message": "5.1 Mbps, 1.2 GB", "ok": true },
                "uptime": { "message": "0:12:03", "ok": true },
                "srtla": { "message": "60% Cellular, 40% WiFi", "ok": true },
            },
        } });
        assert_eq!(state.apply_status(&status), Vec::new());

        // Moblin's first full state; `filters` is an array (enum-keyed dictionary).
        let full = state_event(json!({
            "scene": SCENE_A, "bitrate": PRESET_HIGH, "streaming": true, "recording": false,
            "muted": false, "zoom": 1.0, "filters": [{ "pixellate": {} }, false],
        }));
        assert_eq!(state.apply_event(&full), Vec::new());

        let status = state.status();
        assert!(status.streaming);
        assert_eq!(status.battery_level, Some(87));
        assert_eq!(status.thermal, Some(MoblinThermal::Serious));
        assert_eq!(status.bitrate.as_deref(), Some("5.1 Mbps, 1.2 GB"));
        assert_eq!(status.bonding.as_deref(), Some("60% Cellular, 40% WiFi"));
        assert_eq!(status.scene_name(), Some("Back camera"));
        assert_eq!(status.bitrate_preset().map(|p| p.kbps), Some(5000));

        // Partial states: only what changed.
        let changes = state.apply_event(&state_event(json!({ "scene": SCENE_B })));
        assert_eq!(changes, vec![Change::Scene("Front camera".into())]);
        let changes = state.apply_event(&state_event(json!({ "bitrate": PRESET_LOW })));
        assert_eq!(changes, vec![Change::BitratePreset(1000)]);
        let changes = state.apply_event(&state_event(json!({ "muted": true })));
        assert_eq!(changes, vec![Change::Muted(true)]);
        let changes = state.apply_event(&state_event(json!({ "streaming": false })));
        assert_eq!(changes, vec![Change::Streaming(false)]);
        // getStatus agreeing with what we know isn't news.
        let stopped = json!({ "getStatus": { "general": { "isLive": false, "isMuted": true }, "topLeft": {}, "topRight": {} } });
        assert_eq!(state.apply_status(&stopped), Vec::new());
        assert_eq!(state.status().bitrate, None);
        assert_eq!(
            state.apply_event(&json!({ "log": { "entry": "x" } })),
            Vec::new()
        );
    }

    #[test]
    fn prepares_commands() {
        let mut state = State::default();
        state.apply_settings(&settings());
        state.apply_event(&state_event(json!({ "streaming": true })));

        assert_eq!(
            state.prepare(&MoblinAction::Stream { active: true }),
            Err(MoblinRefusal::AlreadyStreaming)
        );
        assert_eq!(
            state.prepare(&MoblinAction::Stream { active: false }),
            Ok(Request::SetLive(false))
        );
        assert_eq!(
            state.prepare(&MoblinAction::Scene {
                id: SCENE_B.to_lowercase()
            }),
            Ok(Request::SetScene(SCENE_B.into()))
        );
        assert_eq!(
            state.prepare(&MoblinAction::Scene { id: "nope".into() }),
            Err(MoblinRefusal::UnknownScene)
        );
        assert_eq!(
            state.prepare(&MoblinAction::BitratePreset {
                id: PRESET_LOW.into()
            }),
            Ok(Request::SetBitratePreset(PRESET_LOW.into()))
        );
        assert_eq!(
            state.prepare(&MoblinAction::BitratePreset { id: "x".into() }),
            Err(MoblinRefusal::UnknownPreset)
        );
        // Unknown state: let Moblin decide.
        assert_eq!(
            State::default().prepare(&MoblinAction::Stream { active: true }),
            Ok(Request::SetLive(true))
        );
    }

    const CELLULAR: &str = "C0000000-0000-4000-8000-000000000001";
    const WIFI: &str = "C0000000-0000-4000-8000-000000000002";

    fn srt_settings(enabled: bool, cellular_on: bool, wifi_priority: i64) -> Value {
        json!({ "getSettings": { "data": {
            "streams": [], "scenes": [], "bitratePresets": [], "mics": [],
            "srt": {
                "connectionPrioritiesEnabled": enabled,
                "connectionPriorities": [
                    { "id": CELLULAR, "name": "Cellular", "priority": 1, "enabled": cellular_on },
                    { "id": WIFI, "name": "WiFi", "priority": wifi_priority, "enabled": true },
                ],
            },
        } } })
    }

    #[test]
    fn follows_connections_and_logs_their_changes() {
        let mut state = State::default();
        assert_eq!(
            state.apply_settings(&srt_settings(false, true, 1)),
            Vec::new()
        );
        let status = state.status();
        assert!(!status.connection_priorities_enabled);
        assert_eq!(status.connections.len(), 2);
        assert_eq!(status.connections[1].name, "WiFi");

        assert_eq!(
            state.apply_settings(&srt_settings(true, false, 12)),
            vec![
                Change::Connection {
                    name: "Cellular".into(),
                    enabled: false,
                    priority: 1
                },
                // Clamped to Moblin's 1–10.
                Change::Connection {
                    name: "WiFi".into(),
                    enabled: true,
                    priority: 10
                },
                Change::ConnectionPriorities(true),
            ]
        );
    }

    #[test]
    fn prepares_connection_commands() {
        let mut state = State::default();
        state.apply_settings(&srt_settings(true, false, 5));
        let wifi_off = MoblinAction::Connection {
            id: WIFI.to_lowercase(),
            enabled: false,
            priority: None,
        };
        // Cellular is off already: WiFi is the last one in use.
        assert_eq!(state.prepare(&wifi_off), Err(MoblinRefusal::LastConnection));
        assert_eq!(
            state.prepare(&MoblinAction::Connection {
                id: CELLULAR.into(),
                enabled: true,
                priority: Some(3),
            }),
            Ok(Request::SetSrtConnectionPriority {
                id: CELLULAR.into(),
                priority: 3,
                enabled: true
            })
        );
        assert_eq!(
            state.prepare(&MoblinAction::Connection {
                id: WIFI.into(),
                enabled: true,
                priority: Some(11),
            }),
            Err(MoblinRefusal::InvalidPriority)
        );
        assert_eq!(
            state.prepare(&MoblinAction::Connection {
                id: "nope".into(),
                enabled: true,
                priority: None,
            }),
            Err(MoblinRefusal::UnknownConnection)
        );
        let message = serde_json::from_str::<Value>(
            &Request::SetSrtConnectionPriority {
                id: WIFI.into(),
                priority: 5,
                enabled: false,
            }
            .to_message(9),
        )
        .unwrap();
        assert_eq!(
            message,
            json!({ "request": { "id": 9, "data": { "setSrtConnectionPriority": {
                "id": WIFI, "priority": 5, "enabled": false
            } } } })
        );
    }

    #[test]
    fn follows_zoom_torch_and_status_lines() {
        const WIDE: &str = "20000000-0000-4000-8000-000000000001";
        const TELE: &str = "20000000-0000-4000-8000-000000000002";
        let mut state = State::default();
        let full = state_event(json!({
            "zoom": 1.0, "zoomPreset": WIDE, "torchOn": false, "stealthMode": false,
            "zoomPresets": [{ "id": WIDE, "name": "1x" }, { "id": TELE, "name": "3x" }],
        }));
        assert_eq!(state.apply_event(&full), Vec::new());
        assert_eq!(
            state.apply_event(&state_event(json!({ "zoomPreset": TELE, "zoom": 3.0 }))),
            vec![Change::ZoomPreset("3x".into())]
        );
        // Zooming by hand: Moblin reports an ID that's no preset.
        let other = "99999999-0000-4000-8000-000000000000";
        assert_eq!(
            state.apply_event(&state_event(json!({ "zoomPreset": other, "zoom": 2.2 }))),
            Vec::new()
        );
        assert_eq!(
            state.apply_event(&state_event(json!({ "torchOn": true }))),
            vec![Change::Torch(true)]
        );

        let status = json!({ "getStatus": {
            "general": {},
            "topLeft": { "zoom": { "message": "2.2", "ok": true } },
            "topRight": {
                "systemMonitor": { "message": "12%/45% 310 MB", "ok": true },
                "moblink": { "message": "1 relay, Connected", "ok": false },
            },
        } });
        state.apply_status(&status);
        let view = state.status();
        assert_eq!(view.zoom, Some(2.2));
        assert_eq!(view.zoom_preset_id, None);
        assert!(view.torch);
        assert_eq!(view.system_monitor.as_deref(), Some("12%/45% 310 MB"));
        assert_eq!(
            view.moblink,
            Some(MoblinStatusText {
                text: "1 relay, Connected".into(),
                ok: false
            })
        );
        assert_eq!(view.dji_devices, None);

        // A camera without zoom: Moblin's status leaves it out.
        state.apply_status(&json!({ "getStatus": { "topLeft": {}, "topRight": {} } }));
        assert_eq!(state.status().zoom, None);

        assert_eq!(
            state.prepare(&MoblinAction::Zoom { x: 0.1 }),
            Err(MoblinRefusal::InvalidZoom)
        );
        assert_eq!(
            state.prepare(&MoblinAction::ZoomPreset {
                id: TELE.to_lowercase()
            }),
            Ok(Request::SetZoomPreset(TELE.into()))
        );
        assert_eq!(
            state.expected_change(&MoblinAction::ZoomPreset { id: TELE.into() }),
            Some(Change::ZoomPreset("3x".into()))
        );
    }
}
