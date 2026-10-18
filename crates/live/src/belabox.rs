//! BELABOX Cloud: obs-link keeps a remote connection to a switcher's BELABOX
//! (when it has a remote key), publishes a [`BelaboxStatus`] and answers
//! [`BelaboxCommand`]s from the API and the chat service.
//!
//! The status never holds the remote key or the interfaces' IP addresses: the
//! dashboard is shown on stream.

use std::{collections::BTreeMap, time::Duration};

use async_nats::jetstream::{self, kv};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// KV bucket with the latest [`BelaboxStatus`] per switcher (key = switcher ID).
pub const BELABOX_BUCKET: &str = "belabox-status";
/// Like the switcher status: rewritten at least every [`BELABOX_HEARTBEAT`],
/// so it expires when obs-link goes away.
pub const BELABOX_TTL: Duration = Duration::from_secs(30);
pub const BELABOX_HEARTBEAT: Duration = Duration::from_secs(10);

/// The lowest and highest max bitrate BELABOX accepts, in kbps.
pub const BELABOX_MIN_BITRATE: u32 = 300;
pub const BELABOX_MAX_BITRATE: u32 = 12_000;

/// Where the API and the chat service send [`BelaboxCommand`]s; the reply is a
/// JSON [`BelaboxReply`].
pub fn belabox_request_subject(switcher_id: Uuid) -> String {
    format!("belabox.{switcher_id}.request")
}

/// Where the API asks for the [`BelaboxDetails`] of a switcher's BELABOX; the reply is a
/// JSON [`BelaboxDetailsReply`]. They are fetched on demand instead of living in the
/// status, which every viewer receives and which is rewritten on every change.
pub fn belabox_details_subject(switcher_id: Uuid) -> String {
    format!("belabox.{switcher_id}.details")
}

/// Creates the BELABOX status bucket if it doesn't exist yet.
pub async fn belabox_bucket(js: &jetstream::Context) -> Result<kv::Store, async_nats::Error> {
    if let Ok(store) = js.get_key_value(BELABOX_BUCKET).await {
        return Ok(store);
    }

    Ok(js
        .create_key_value(kv::Config {
            bucket: BELABOX_BUCKET.to_owned(),
            description: "BELABOX Cloud status per switcher, written by obs-link".to_owned(),
            history: 1,
            max_age: BELABOX_TTL,
            storage: jetstream::stream::StorageType::Memory,
            limit_markers: Some(BELABOX_TTL),
            ..Default::default()
        })
        .await?)
}

/// How far obs-link got in reaching the BELABOX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum BelaboxLink {
    /// Connecting to BELABOX Cloud (or waiting to reconnect).
    #[default]
    Connecting,
    /// Connected, and the BELABOX is online.
    Online,
    /// Connected to BELABOX Cloud, but the BELABOX itself isn't.
    EncoderOffline,
    /// BELABOX Cloud refused the remote key; no retries until it changes.
    InvalidKey,
}

/// The latest known state of a switcher's BELABOX. Fields added later need
/// `#[serde(default)]`: obs-link and the API may run different versions.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxStatus {
    pub updated_at: DateTime<Utc>,
    pub link: BelaboxLink,
    pub streaming: bool,
    /// The max bitrate setting, kbps.
    pub max_br: Option<u32>,
    /// What all interfaces send together, kbps.
    pub total_kbps: u32,
    /// Network interfaces, by name.
    pub interfaces: Vec<BelaboxInterface>,
    /// Sensor readings as BELABOX formats them ("45.3 °C").
    pub sensors: Vec<BelaboxSensor>,
    /// BELABOX's current notifications (start errors, network problems, …).
    pub notifications: Vec<BelaboxNotice>,
    /// The encoding pipeline's name.
    pub pipeline: Option<String>,
    /// SRT latency, ms.
    pub srt_latency: Option<u32>,
    /// The BELABOX Cloud relay server's name, when one is used.
    pub relay: Option<String>,
}

impl BelaboxStatus {
    pub fn new(link: BelaboxLink) -> Self {
        Self {
            updated_at: Utc::now(),
            link,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxInterface {
    /// The interface name (`usb0`, `wlan0`, …).
    pub name: String,
    /// What a name the streamer gave it is saved under: the same device keeps it
    /// when the interface name changes (`usb0` → `usb1`). Opaque (a modem's model
    /// and IMEI digits, a WiFi adapter's hardware, else the interface name, hashed).
    #[serde(default)]
    pub key: String,
    /// What it sends, kbps.
    pub kbps: u32,
    pub enabled: bool,
    /// Why it can't be used ("duplicate IPv4 addr", "WiFi hotspot").
    pub error: Option<String>,
    /// For cellular modems: the mobile network.
    pub modem: Option<BelaboxModem>,
}

impl BelaboxInterface {
    /// What to call it: the name the streamer gave it (`labels` by `key`), else
    /// the interface name.
    pub fn display_name<'a>(&'a self, labels: &'a BTreeMap<String, String>) -> &'a str {
        labels.get(&self.key).map_or(&self.name, |label| label)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxModem {
    /// The operator ("Vodafone").
    pub network: Option<String>,
    /// "4G", "5G", …
    pub network_type: Option<String>,
    /// Signal quality, 0–100.
    pub signal: Option<u32>,
    pub roaming: Option<bool>,
    /// ModemManager's state ("connected", "registered", "scanning", …).
    pub connection: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxSensor {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum BelaboxNoticeKind {
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxNotice {
    /// BELABOX's name for it (`start_error`, `netif_disable_all`, …).
    pub name: String,
    pub kind: BelaboxNoticeKind,
    pub message: String,
}

/// Something to do on a switcher's BELABOX.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BelaboxCommand {
    pub action: BelaboxAction,
    /// Who asked, for the activity feed.
    pub by: String,
}

/// Passwords in these actions are never logged: nothing prints a command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BelaboxAction {
    Start,
    Stop,
    /// Applied at once while streaming; otherwise obs-link keeps it for its
    /// next `Start` (BELABOX ignores it when idle).
    Bitrate {
        max_br: u32,
    },
    Interface {
        name: String,
        enabled: bool,
    },
    /// Rescan for WiFi networks.
    WifiScan,
    /// Join a saved network (its connection UUID).
    WifiConnect {
        uuid: String,
    },
    WifiDisconnect {
        uuid: String,
    },
    /// Join and save a new network. `password` is `None` for an open one.
    WifiNew {
        device: String,
        ssid: String,
        password: Option<String>,
    },
    WifiForget {
        uuid: String,
    },
    /// Turn an adapter into a hotspot, or back into a WiFi client.
    Hotspot {
        device: String,
        active: bool,
    },
    /// `password` `None` keeps the current one (obs-link remembers it).
    HotspotConfig {
        device: String,
        name: String,
        password: Option<String>,
        channel: String,
    },
    /// `password` `None` keeps the current one.
    ModemConfig {
        modem: String,
        autoconfig: bool,
        apn: String,
        username: String,
        password: Option<String>,
        roaming: bool,
        /// The operator's ID, or empty for automatic.
        network: String,
        network_type: String,
    },
    /// Search for operators (takes the modem offline for a while).
    ModemScan {
        modem: String,
    },
    /// Stream settings, used from the BELABOX's next start from here: BELABOX
    /// only saves them when it starts.
    Settings {
        settings: BelaboxSettingsUpdate,
    },
}

/// The stream settings as the dialog sends them: all of them, like the other
/// settings PUTs. The server is a relay (`relay_server`) or an address and port;
/// the stream ID a relay account or a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxSettingsUpdate {
    pub pipeline: String,
    /// Needed when the pipeline takes an audio source.
    pub audio_source: Option<String>,
    /// Needed when the pipeline takes an audio codec.
    pub audio_codec: Option<String>,
    /// ms, -2000–2000.
    pub audio_delay: i32,
    /// ms, 100–10000.
    pub srt_latency: u32,
    pub bitrate_overlay: bool,
    pub relay_server: Option<String>,
    /// Needs a `relay_server`.
    pub relay_account: Option<String>,
    /// Without a `relay_server`.
    pub srtla_address: Option<String>,
    pub srtla_port: Option<u16>,
    /// Without a `relay_account`. `None` keeps the one the BELABOX has.
    pub stream_id: Option<String>,
}

/// Why a [`BelaboxCommand`] wasn't carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum BelaboxRefusal {
    #[error("still connecting to BELABOX Cloud")]
    Connecting,
    #[error("BELABOX Cloud refused the secret URL")]
    InvalidKey,
    #[error("the BELABOX is offline")]
    EncoderOffline,
    #[error("the BELABOX has no stream settings yet; start it once from its own page")]
    NoConfig,
    #[error("the BELABOX is already streaming")]
    AlreadyStreaming,
    #[error("the BELABOX isn't streaming")]
    NotStreaming,
    #[error("the bitrate must be between 300 and 12000 kbps")]
    InvalidBitrate,
    #[error("the BELABOX has no such network interface")]
    UnknownInterface,
    #[error("the last enabled network interface can't be turned off")]
    LastInterface,
    #[error("the BELABOX has no such WiFi adapter or network")]
    UnknownWifi,
    #[error("the BELABOX has no such modem, or it has no SIM")]
    UnknownModem,
    #[error("stream settings can't change while the BELABOX streams")]
    Streaming,
    #[error("invalid {0}")]
    Invalid(BelaboxField),
}

/// What a setting we refused was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BelaboxField {
    Pipeline,
    AudioSource,
    AudioCodec,
    AudioDelay,
    SrtLatency,
    Relay,
    RelayAccount,
    Address,
    Port,
    StreamId,
    WifiName,
    WifiPassword,
    HotspotName,
    HotspotPassword,
    HotspotChannel,
    Apn,
    NetworkType,
    Network,
}

impl BelaboxField {
    /// The API's error code for a refused value, so the dashboard can mark the field.
    pub fn code(self) -> &'static str {
        match self {
            Self::Pipeline => "invalid_pipeline",
            Self::AudioSource => "invalid_audio_source",
            Self::AudioCodec => "invalid_audio_codec",
            Self::AudioDelay => "invalid_audio_delay",
            Self::SrtLatency => "invalid_srt_latency",
            Self::Relay => "invalid_relay",
            Self::RelayAccount => "invalid_relay_account",
            Self::Address => "invalid_address",
            Self::Port => "invalid_port",
            Self::StreamId => "invalid_stream_id",
            Self::WifiName => "invalid_wifi_name",
            Self::WifiPassword => "invalid_wifi_password",
            Self::HotspotName => "invalid_hotspot_name",
            Self::HotspotPassword => "invalid_hotspot_password",
            Self::HotspotChannel => "invalid_hotspot_channel",
            Self::Apn => "invalid_apn",
            Self::NetworkType => "invalid_network_type",
            Self::Network => "invalid_network",
        }
    }
}

impl std::fmt::Display for BelaboxField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pipeline => "pipeline",
            Self::AudioSource => "audio source",
            Self::AudioCodec => "audio codec",
            Self::AudioDelay => "audio delay (-2000 to 2000 ms)",
            Self::SrtLatency => "SRT latency (100 to 10000 ms)",
            Self::Relay => "relay server",
            Self::RelayAccount => "relay account",
            Self::Address => "server address",
            Self::Port => "server port",
            Self::StreamId => "stream ID",
            Self::WifiName => "network name (1 to 32 characters)",
            Self::WifiPassword => "WiFi password (8 to 63 characters)",
            Self::HotspotName => "hotspot name (1 to 32 characters)",
            Self::HotspotPassword => "hotspot password (8 to 64 characters)",
            Self::HotspotChannel => "hotspot band",
            Self::Apn => "APN",
            Self::NetworkType => "network type",
            Self::Network => "mobile network",
        })
    }
}

/// obs-link's answer to a [`BelaboxCommand`]: sent to the BELABOX, or refused.
pub type BelaboxReply = Result<(), BelaboxRefusal>;

/// What the BELABOX settings dialog shows, asked for when it is open. Never holds
/// a password (`has_password`); the hotspot's and the modems' stay in obs-link.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxDetails {
    pub streaming: bool,
    pub wifi: Vec<BelaboxWifiDevice>,
    pub modems: Vec<BelaboxModemDetails>,
    /// Audio inputs the BELABOX found.
    pub audio_sources: Vec<String>,
    pub audio_codecs: Vec<BelaboxChoice>,
    pub pipelines: Vec<BelaboxPipeline>,
    pub relay_servers: Vec<BelaboxRelayServer>,
    pub relay_accounts: Vec<BelaboxRelayAccount>,
    /// The saved settings, with what was changed here on top.
    pub settings: BelaboxStreamSettings,
    /// `settings` holds changes the BELABOX takes at its next start from here.
    pub settings_pending: bool,
    /// How the last WiFi or hotspot request ended, once the BELABOX said.
    pub result: Option<BelaboxResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxChoice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxPipeline {
    pub id: String,
    pub name: String,
    /// Takes an audio source.
    pub audio_source: bool,
    /// Takes an audio codec.
    pub audio_codec: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxRelayServer {
    pub id: String,
    /// With the ping BELABOX Cloud measured ("EU West (21 ms)").
    pub name: String,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxRelayAccount {
    pub id: String,
    pub name: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxStreamSettings {
    pub pipeline: Option<String>,
    pub audio_source: Option<String>,
    pub audio_codec: Option<String>,
    pub audio_delay: Option<i32>,
    pub srt_latency: Option<u32>,
    pub bitrate_overlay: Option<bool>,
    pub relay_server: Option<String>,
    pub relay_account: Option<String>,
    pub srtla_address: Option<String>,
    pub srtla_port: Option<u16>,
    /// A stream ID is a key: it's never sent back.
    pub has_stream_id: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxWifiDevice {
    pub id: String,
    pub interface: String,
    /// The adapter's vendor and product.
    pub hardware: Option<String>,
    /// Networks in range and saved ones, connected first, then by signal.
    pub networks: Vec<BelaboxWifiNetwork>,
    /// Set while the adapter is a hotspot (then it has no `networks`).
    pub hotspot: Option<BelaboxHotspot>,
    pub supports_hotspot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxWifiNetwork {
    pub ssid: String,
    /// In range right now.
    pub available: bool,
    /// Connected through this adapter.
    pub connected: bool,
    /// The saved connection's UUID, when saved.
    pub saved: Option<String>,
    /// 0–100.
    pub signal: Option<u32>,
    /// "WPA2", or empty for an open network.
    pub security: Option<String>,
    /// MHz.
    pub frequency: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxHotspot {
    pub name: String,
    pub has_password: bool,
    pub channel: String,
    pub channels: Vec<BelaboxChoice>,
    /// BELABOX's own warnings ("modified": changed outside belaUI).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxModemDetails {
    pub id: String,
    pub interface: Option<String>,
    /// "EM7455 - 12345 | Vodafone".
    pub name: String,
    pub no_sim: bool,
    /// Network types it supports ("4G", "3G4G").
    pub network_types: Vec<String>,
    pub network_type: Option<String>,
    pub config: Option<BelaboxModemConfig>,
    /// From the last operator scan.
    pub networks: Vec<BelaboxModemNetwork>,
    pub status: Option<BelaboxModem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxModemConfig {
    /// The BELABOX picks the APN itself; `None` when it can't.
    pub autoconfig: Option<bool>,
    pub apn: String,
    pub username: String,
    pub has_password: bool,
    pub roaming: bool,
    /// The operator's ID; empty is automatic.
    pub network: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxModemNetwork {
    /// The operator ID.
    pub id: String,
    pub name: String,
    /// "available", "forbidden", …; `None` when unknown.
    pub availability: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum BelaboxResultKind {
    WifiConnect,
    WifiNew,
    HotspotConfig,
}

/// How a WiFi request ended; the BELABOX answers a while after (up to 15 s).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BelaboxResult {
    /// Counts up, so the dialog tells a new result from one it showed.
    pub id: u64,
    pub kind: BelaboxResultKind,
    pub device: String,
    /// `None` = it worked. `auth` (wrong password), `generic`, or for a hotspot
    /// the field BELABOX refused (`name`, `password`, `channel`, `saving`,
    /// `activating`).
    pub error: Option<String>,
}

/// obs-link's answer to a details request.
pub type BelaboxDetailsReply = Result<BelaboxDetails, BelaboxRefusal>;

/// What part of the BELABOX was changed from here (for the activity feed; the
/// details stay out of it: network names say where the streamer is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum BelaboxChange {
    Wifi,
    Hotspot,
    Modem,
    StreamSettings,
}
