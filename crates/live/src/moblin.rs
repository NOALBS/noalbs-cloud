//! Moblin remote control: a switcher's Moblin (in its "streamer" role) connects
//! to obs-link at `/moblin/<switcher ID>` with the switcher's Moblin password.
//! While it's connected, obs-link publishes a [`MoblinStatus`] and answers
//! [`MoblinCommand`]s from the API, the chat service and the switcher.
//!
//! The status never holds the phone's Wi-Fi name, location or logs: the
//! dashboard is shown on stream.

use std::time::Duration;

use async_nats::jetstream::{self, kv};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// KV bucket with the latest [`MoblinStatus`] per switcher (key = switcher ID);
/// no key = no Moblin connected.
pub const MOBLIN_BUCKET: &str = "moblin-status";
/// Like the switcher status: rewritten at least every [`MOBLIN_HEARTBEAT`], so
/// it expires when obs-link goes away.
pub const MOBLIN_TTL: Duration = Duration::from_secs(30);
pub const MOBLIN_HEARTBEAT: Duration = Duration::from_secs(10);

/// Where the API, the chat service and the switcher send [`MoblinCommand`]s;
/// the reply is a JSON [`MoblinReply`].
pub fn moblin_request_subject(switcher_id: Uuid) -> String {
    format!("moblin.{switcher_id}.request")
}

/// Sent when a Moblin identifies; older Moblin connections for the same
/// switcher close (a [`crate::Takeover`]).
pub fn moblin_takeover_subject(switcher_id: Uuid) -> String {
    format!("moblin.{switcher_id}.takeover")
}

/// Creates the Moblin status bucket if it doesn't exist yet.
pub async fn moblin_bucket(js: &jetstream::Context) -> Result<kv::Store, async_nats::Error> {
    if let Ok(store) = js.get_key_value(MOBLIN_BUCKET).await {
        return Ok(store);
    }

    Ok(js
        .create_key_value(kv::Config {
            bucket: MOBLIN_BUCKET.to_owned(),
            description: "Moblin status per switcher, written by obs-link".to_owned(),
            history: 1,
            max_age: MOBLIN_TTL,
            storage: jetstream::stream::StorageType::Memory,
            limit_markers: Some(MOBLIN_TTL),
            ..Default::default()
        })
        .await?)
}

/// How hot the phone is (Moblin's flame: white, yellow, red).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum MoblinThermal {
    Normal,
    Serious,
    Critical,
}

/// The latest known state of a switcher's connected Moblin. Every field has a
/// default: obs-link and the API may run different versions.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(default)]
pub struct MoblinStatus {
    pub updated_at: DateTime<Utc>,
    /// Over the plan's switcher limit: connected, but commands are refused.
    pub paused: bool,
    pub streaming: bool,
    pub recording: bool,
    pub muted: bool,
    /// Battery charge, 0–100.
    pub battery_level: Option<u8>,
    pub battery_charging: Option<bool>,
    pub thermal: Option<MoblinThermal>,
    /// What Moblin sends, as it formats it ("5.2 Mbps, 1.3 GB"), while live.
    pub bitrate: Option<String>,
    /// SRTLA bonding per connection, as Moblin formats it.
    pub bonding: Option<String>,
    /// SRTLA round-trip times per connection, as Moblin formats it.
    pub bonding_rtts: Option<String>,
    /// How long it has been live ("1:02:03").
    pub uptime: Option<String>,
    /// The scene on air (one of `scenes`).
    pub scene_id: Option<String>,
    pub scenes: Vec<MoblinScene>,
    /// The bitrate preset in use (one of `bitrate_presets`), if the bitrate
    /// matches one.
    pub bitrate_preset_id: Option<String>,
    pub bitrate_presets: Vec<MoblinBitratePreset>,
    /// The phone's light.
    pub torch: bool,
    /// Moblin's screen shows a black (or custom) image instead of the camera.
    pub stealth_mode: bool,
    /// CPU and memory as Moblin formats it ("12%/45% 310 MB": Moblin's CPU, the
    /// phone's, memory), when its system monitor is on.
    pub system_monitor: Option<String>,
    /// Moblink relays (other phones bonding for this one), as Moblin reports them.
    pub moblink: Option<MoblinStatusText>,
    /// DJI cameras connected to Moblin.
    pub dji_devices: Option<MoblinStatusText>,
    /// Whether the connections' priorities and on/off apply; off, Moblin uses
    /// every connection alike.
    pub connection_priorities_enabled: bool,
    /// The SRTLA bonding connections (Cellular, WiFi, Moblink relays, …).
    pub connections: Vec<MoblinConnection>,
    /// The camera's zoom factor (`None` when the camera can't zoom).
    pub zoom: Option<f64>,
    /// The current camera's zoom presets.
    pub zoom_presets: Vec<MoblinZoomPreset>,
    /// The zoom preset in use, if the zoom is one (one of `zoom_presets`).
    pub zoom_preset_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MoblinZoomPreset {
    pub id: String,
    pub name: String,
}

/// Zoom factors we pass on; Moblin clamps them to what the camera can do.
pub const MOBLIN_MIN_ZOOM: f64 = 0.5;
pub const MOBLIN_MAX_ZOOM: f64 = 100.0;

/// A status line from Moblin, and whether it says something's wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MoblinStatusText {
    pub text: String,
    pub ok: bool,
}

/// One of Moblin's SRTLA bonding connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MoblinConnection {
    pub id: String,
    pub name: String,
    /// 1–10: a stable connection with a higher priority is used more.
    pub priority: u8,
    /// Off = not used (while priorities are on).
    pub enabled: bool,
}

/// Moblin's connection priorities range.
pub const MOBLIN_MIN_PRIORITY: u8 = 1;
pub const MOBLIN_MAX_PRIORITY: u8 = 10;

impl MoblinStatus {
    pub fn scene_name(&self) -> Option<&str> {
        let id = self.scene_id.as_deref()?;
        self.scenes
            .iter()
            .find(|scene| scene.id == id)
            .map(|scene| scene.name.as_str())
    }

    pub fn bitrate_preset(&self) -> Option<&MoblinBitratePreset> {
        let id = self.bitrate_preset_id.as_deref()?;
        self.bitrate_presets.iter().find(|preset| preset.id == id)
    }
}

/// One of Moblin's scenes. IDs are Moblin's UUIDs, kept as it spells them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MoblinScene {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MoblinBitratePreset {
    pub id: String,
    pub kbps: u32,
}

/// Something to do on a switcher's Moblin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoblinCommand {
    pub action: MoblinAction,
    /// Who asked, for the activity feed.
    pub by: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MoblinAction {
    Stream {
        active: bool,
    },
    Record {
        active: bool,
    },
    Mute {
        muted: bool,
    },
    BitratePreset {
        id: String,
    },
    Scene {
        id: String,
    },
    /// A zoom factor (Moblin clamps it to the camera's range).
    Zoom {
        x: f64,
    },
    ZoomPreset {
        id: String,
    },
    Torch {
        on: bool,
    },
    StealthMode {
        on: bool,
    },
    /// Whether connection priorities (and turning connections off) apply.
    ConnectionPriorities {
        enabled: bool,
    },
    /// Turn a bonding connection on or off, and/or set its priority (1–10;
    /// `None` keeps it).
    Connection {
        id: String,
        enabled: bool,
        priority: Option<u8>,
    },
}

/// Why a [`MoblinCommand`] wasn't carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum MoblinRefusal {
    #[error("Moblin isn't connected")]
    NotConnected,
    #[error("the switcher is paused")]
    Paused,
    #[error("Moblin is already live")]
    AlreadyStreaming,
    #[error("Moblin isn't live")]
    NotStreaming,
    #[error("Moblin has no such scene")]
    UnknownScene,
    #[error("Moblin has no such zoom preset")]
    UnknownZoomPreset,
    #[error("the zoom must be between 0.5x and 100x")]
    InvalidZoom,
    #[error("Moblin has no such connection")]
    UnknownConnection,
    #[error("the last connection in use can't be turned off")]
    LastConnection,
    #[error("the priority must be between 1 and 10")]
    InvalidPriority,
    #[error("Moblin has no such bitrate preset")]
    UnknownPreset,
    #[error("Moblin refused")]
    Failed,
    #[error("Moblin didn't answer")]
    Timeout,
}

/// obs-link's answer to a [`MoblinCommand`]: done, or refused.
pub type MoblinReply = Result<(), MoblinRefusal>;
