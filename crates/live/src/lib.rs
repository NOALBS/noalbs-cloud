//! The live-data contract between obs-link and the API: NATS subjects, the
//! JetStream resources, and the messages sent over them.
//!
//! | What | Where | Written by |
//! |---|---|---|
//! | [`SwitcherStatus`] | KV bucket [`STATUS_BUCKET`], key = switcher ID | obs-link, on change + every 10 s |
//! | [`StatsSample`] | stream [`STATS_STREAM`], subject [`stats_subject`] | obs-link, every second |
//! | [`SwitcherEvent`] | core subject [`event_subject`] (+ `switcher_events` table) | obs-link and API |
//! | config changed | core subject [`config_subject`] | API, after saving settings |
//! | [`Intent`] | core subject [`intent_subject`] | API, before an OBS command |
//! | [`Takeover`] | core subject [`takeover_subject`] | obs-link, when a browser source identifies |
//! | reconnect | core subject [`reconnect_subject`] | API (staff): close the browser source's connection so it reconnects |
//! | chat links changed | core subject [`chat_reconcile_subject`] | API, after linking or unlinking chat channels and bots |
//! | [`BelaboxStatus`] | KV bucket [`BELABOX_BUCKET`], key = switcher ID | obs-link, while a BELABOX link runs, on change + every 10 s |
//! | [`BelaboxCommand`] → [`BelaboxReply`] | request subject [`belabox_request_subject`] | API and chat service |
//! | (nothing) → [`BelaboxDetailsReply`] | request subject [`belabox_details_subject`] | API (the settings dialog; not in the status: network names, modem and stream settings) |
//! | [`MoblinStatus`] | KV bucket [`MOBLIN_BUCKET`], key = switcher ID | obs-link, while a Moblin is connected, on change + every 10 s |
//! | [`MoblinCommand`] → [`MoblinReply`] | request subject [`moblin_request_subject`] | API, chat service, and obs-link (with the OBS stream) |
//! | [`Takeover`] | core subject [`moblin_takeover_subject`] | obs-link, when a Moblin identifies |
//! | [`AudioUpdate`] | core subject [`audio_subject`] | obs-link: levels while demanded, mute/fader changes always |
//! | levels wanted | core subject [`audio_demand_subject`] | API, every 5 s while a mixer is open |

mod audio;
mod belabox;
mod moblin;

pub use audio::*;
pub use belabox::*;
pub use moblin::*;

use std::time::Duration;

use async_nats::jetstream::{self, kv, stream};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// KV bucket with the latest [`SwitcherStatus`] per switcher.
pub const STATUS_BUCKET: &str = "switcher-status";
/// A status key expires this long after its last write, so a crashed obs-link
/// shows as offline. obs-link rewrites it every [`STATUS_HEARTBEAT`].
pub const STATUS_TTL: Duration = Duration::from_secs(30);
pub const STATUS_HEARTBEAT: Duration = Duration::from_secs(10);

/// JetStream stream holding recent [`StatsSample`]s, for charts that start full.
pub const STATS_STREAM: &str = "SWITCHER_STATS";
pub const STATS_RETENTION: Duration = Duration::from_secs(120);

pub fn status_key(switcher_id: Uuid) -> String {
    switcher_id.to_string()
}

pub fn stats_subject(switcher_id: Uuid) -> String {
    format!("switcher.{switcher_id}.stats")
}

pub fn event_subject(switcher_id: Uuid) -> String {
    format!("switcher.{switcher_id}.event")
}

pub fn config_subject(switcher_id: Uuid) -> String {
    format!("switcher.{switcher_id}.config")
}

pub fn intent_subject(switcher_id: Uuid) -> String {
    format!("switcher.{switcher_id}.intent")
}

pub fn takeover_subject(switcher_id: Uuid) -> String {
    format!("switcher.{switcher_id}.takeover")
}

/// Staff asking obs-link to close a switcher's browser-source connection; the
/// browser source reconnects on its own (unlike after a takeover).
pub fn reconnect_subject(switcher_id: Uuid) -> String {
    format!("switcher.{switcher_id}.reconnect")
}

/// A chat service subject of one environment (`db::chat::environment_id`), so
/// environments sharing a NATS server never handle each other's messages.
pub fn chat_subject(environment: &str, name: &str) -> String {
    format!("chat.{environment}.{name}")
}

/// KV bucket where chat service instances heartbeat: key `<environment>.<instance>`.
pub const CHAT_INSTANCES_BUCKET: &str = "chat-instances";
/// KV bucket of Twitch conduit shard claims: key `<environment>.shard.<n>`, value the
/// holding instance's ID.
pub const CHAT_SHARDS_BUCKET: &str = "chat-shards";
/// KV bucket with how YouTube chat is doing, for Admin → Status: key
/// `<environment>.watcher` ([`YouTubeWatcherSummary`], rewritten every 10 s by the
/// instance following YouTube's live chats) and `<environment>.error`
/// ([`YouTubeLastError`], by any instance). Entries last a day.
pub const CHAT_YOUTUBE_STATUS_BUCKET: &str = "chat-youtube-status";

/// The instance following YouTube's live chats, as it last said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct YouTubeWatcherSummary {
    pub instance_id: String,
    pub updated_at: DateTime<Utc>,
    /// Linked YouTube channels with a switcher streaming.
    pub streaming: u32,
    /// Of those, the ones whose live chat is being followed.
    pub chats_open: u32,
}

/// The last error YouTube answered with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct YouTubeLastError {
    pub at: DateTime<Utc>,
    /// The app's daily quota is used up (it resets at midnight Pacific time).
    pub quota: bool,
    pub message: String,
}

/// A key in the chat service's KV buckets: the parts joined by dots, with anything
/// a NATS KV key can't hold (only `[-/_=.a-zA-Z0-9]`) replaced by `_`.
pub fn chat_key(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| {
            part.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '=') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Tells the chat service to compare its platform subscriptions with what the
/// chat links need (any instance may do it; they share a lease).
pub fn chat_reconcile_subject(environment: &str) -> String {
    chat_subject(environment, "reconcile")
}

/// Where the API sends OBS requests; obs-link relays them to the browser source.
pub fn obs_request_subject(switcher_id: Uuid) -> String {
    format!("obslink.{switcher_id}.request")
}

/// Creates the status bucket if it doesn't exist yet.
pub async fn status_bucket(js: &jetstream::Context) -> Result<kv::Store, async_nats::Error> {
    if let Ok(store) = js.get_key_value(STATUS_BUCKET).await {
        return Ok(store);
    }

    Ok(js
        .create_key_value(kv::Config {
            bucket: STATUS_BUCKET.to_owned(),
            description: "Live status per switcher, written by obs-link".to_owned(),
            history: 1,
            max_age: STATUS_TTL,
            storage: jetstream::stream::StorageType::Memory,
            // Tell watchers when a key expires, so a dead obs-link is noticed.
            limit_markers: Some(STATUS_TTL),
            ..Default::default()
        })
        .await?)
}

/// The JetStream resources the live data lives in, opened (and created when
/// missing) together: every service that reads or writes it starts with [`Stores::open`].
#[derive(Clone)]
pub struct Stores {
    pub status: kv::Store,
    pub belabox: kv::Store,
    pub moblin: kv::Store,
    pub stats: stream::Stream,
}

impl Stores {
    pub async fn open(js: &jetstream::Context) -> Result<Self, async_nats::Error> {
        Ok(Self {
            status: status_bucket(js).await?,
            belabox: belabox_bucket(js).await?,
            moblin: moblin_bucket(js).await?,
            stats: stats_stream(js).await?,
        })
    }
}

/// Creates the stats stream if it doesn't exist yet.
pub async fn stats_stream(js: &jetstream::Context) -> Result<stream::Stream, async_nats::Error> {
    Ok(js
        .get_or_create_stream(stream::Config {
            name: STATS_STREAM.to_owned(),
            description: Some("Recent stream-server stats per switcher".to_owned()),
            subjects: vec!["switcher.*.stats".to_owned()],
            max_age: STATS_RETENTION,
            storage: stream::StorageType::Memory,
            ..Default::default()
        })
        .await?)
}

/// How the feed is doing, and so which scene is (or would be) on air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum FeedState {
    Live,
    Low,
    Offline,
    /// Not streaming, or nothing known yet.
    #[default]
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SceneRef {
    /// `null` on OBS versions that don't report scene UUIDs.
    pub uuid: Option<Uuid>,
    pub name: String,
}

/// The latest known state of a switcher's browser source and OBS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SwitcherStatus {
    pub updated_at: DateTime<Utc>,
    /// The browser source's websocket to obs-link is up (always true while the key exists).
    pub browser_source: bool,
    /// The browser source is connected to OBS's websocket.
    pub obs_connected: bool,
    pub streaming: bool,
    pub stream_started_at: Option<DateTime<Utc>>,
    pub recording: bool,
    pub recording_paused: bool,
    pub recording_started_at: Option<DateTime<Utc>>,
    pub current_scene: Option<SceneRef>,
    /// Automatic switching is on (`bitrate_switcher_enabled`).
    pub auto_switch: bool,
    pub state: FeedState,
    /// The stream server the switcher is using, if any.
    pub active_server_id: Option<Uuid>,
    /// Over the workspace plan's switcher limit: connected, but not switching.
    #[serde(default)]
    pub paused: bool,
    /// OBS's frame counters when the stream started (or when obs-link first saw
    /// it streaming), so OBS stats can count this stream's dropped frames like
    /// NOALBS. `null` while not streaming, and while paused (no switcher runs).
    #[serde(default)]
    pub stream_frames: Option<ObsFrames>,
}

/// OBS's frame counters (`GetStats`), which run since OBS started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ObsFrames {
    pub render_total: u64,
    /// Missed due to rendering lag.
    pub render_skipped: u64,
    pub output_total: u64,
    /// Skipped due to encoding lag.
    pub output_skipped: u64,
}

impl ObsFrames {
    /// What was counted since `start`; everything if the counters went back
    /// (OBS restarted since).
    pub fn since(self, start: Self) -> Self {
        if self.render_total < start.render_total || self.output_total < start.output_total {
            return self;
        }
        Self {
            render_total: self.render_total - start.render_total,
            render_skipped: self.render_skipped.saturating_sub(start.render_skipped),
            output_total: self.output_total - start.output_total,
            output_skipped: self.output_skipped.saturating_sub(start.output_skipped),
        }
    }
}

impl SwitcherStatus {
    pub fn connected(auto_switch: bool) -> Self {
        Self {
            updated_at: Utc::now(),
            browser_source: true,
            obs_connected: false,
            streaming: false,
            stream_started_at: None,
            recording: false,
            recording_paused: false,
            recording_started_at: None,
            current_scene: None,
            auto_switch,
            state: FeedState::Idle,
            active_server_id: None,
            paused: false,
            stream_frames: None,
        }
    }
}

/// One stream server's numbers at one moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ServerSample {
    pub server_id: Uuid,
    /// kbps; 0 when there is no feed or no fresh stats.
    pub bitrate: u32,
    /// ms, for servers that report it (SRT).
    pub rtt: Option<f64>,
    pub state: FeedState,
}

/// All of a switcher's stream servers, sampled once a second.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StatsSample {
    pub at: DateTime<Utc>,
    pub servers: Vec<ServerSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SwitchReason {
    /// The switcher switched because of the feed.
    Auto,
    /// Someone switched from the dashboard.
    Dashboard,
    /// It changed in OBS itself.
    Obs,
}

/// What happened, with the details the activity feed shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum EventDetail {
    SceneSwitched {
        from: Option<String>,
        to: String,
        reason: SwitchReason,
        /// Who did it, for dashboard switches.
        by: Option<String>,
        /// For automatic switches: the server and numbers that caused it.
        server: Option<String>,
        bitrate: Option<u32>,
        rtt: Option<f64>,
    },
    StreamStarted {
        by: Option<String>,
    },
    StreamStopped {
        by: Option<String>,
    },
    /// The feed was gone this long on the BRB scene, so the switcher stopped the
    /// stream (NOALBS's offline timeout).
    OfflineTimeout {
        minutes: u32,
    },
    /// The Ending scene was on air this long, so the switcher stopped the
    /// stream.
    EndingTimeout {
        minutes: u32,
    },
    RecordingStarted {
        by: Option<String>,
    },
    RecordingStopped {
        by: Option<String>,
    },
    /// "Only record on these scenes" paused the recording: `scene` is not one of them.
    RecordingScenePaused {
        scene: String,
    },
    /// ... and resumed it, on `scene`.
    RecordingSceneResumed {
        scene: String,
    },
    /// OBS refused to pause or resume the recording (some formats can't pause).
    RecordingSceneError {
        message: String,
    },
    AutoSwitchChanged {
        enabled: bool,
        by: Option<String>,
    },
    /// "Fix feed" / `!fix` made these media sources reconnect.
    FeedFixed {
        sources: Vec<String>,
        by: Option<String>,
    },
    /// A marker was added to the running stream (dashboard button or `!marker`).
    MarkerCreated {
        marker_id: Uuid,
        description: Option<String>,
        /// Seconds into the stream.
        offset_seconds: i64,
        by: Option<String>,
    },
    /// A source on a scene (or in a group) was shown or hidden.
    SourceToggled {
        source: String,
        /// The scene or group it sits in.
        scene: String,
        enabled: bool,
        by: Option<String>,
    },
    /// An input on OBS's Audio Mixer was muted or unmuted.
    InputMuted {
        input: String,
        muted: bool,
        by: Option<String>,
    },
    ObsConnected,
    ObsDisconnected,
    /// The BELABOX started streaming (`by` is `None` when it was started on
    /// the BELABOX itself).
    BelaboxStreamStarted {
        by: Option<String>,
    },
    BelaboxStreamStopped {
        by: Option<String>,
    },
    /// The BELABOX came online on BELABOX Cloud.
    BelaboxOnline,
    BelaboxOffline,
    BelaboxBitrateChanged {
        max_br: u32,
        by: Option<String>,
    },
    BelaboxInterfaceToggled {
        name: String,
        enabled: bool,
        by: Option<String>,
    },
    /// Part of the BELABOX was reconfigured from here (WiFi, hotspot, modem, stream settings).
    BelaboxConfigured {
        what: BelaboxChange,
        by: Option<String>,
    },
    /// BELABOX reported an error, or a linked start/stop couldn't be sent.
    BelaboxError {
        message: String,
    },
    /// A Moblin connected with the right password.
    MoblinConnected,
    MoblinDisconnected,
    /// A Moblin tried the wrong password (logged at most every 10 minutes).
    MoblinWrongPassword,
    /// Moblin went live (`by` is `None` when started on the phone).
    MoblinStreamStarted {
        by: Option<String>,
    },
    MoblinStreamStopped {
        by: Option<String>,
    },
    MoblinRecordingStarted {
        by: Option<String>,
    },
    MoblinRecordingStopped {
        by: Option<String>,
    },
    MoblinMuteChanged {
        muted: bool,
        by: Option<String>,
    },
    MoblinSceneChanged {
        scene: String,
        by: Option<String>,
    },
    MoblinBitrateChanged {
        kbps: u32,
        by: Option<String>,
    },
    /// A zoom preset was picked (plain zooming isn't logged: it changes a lot).
    MoblinZoomPresetChanged {
        name: String,
        by: Option<String>,
    },
    MoblinTorchChanged {
        on: bool,
        by: Option<String>,
    },
    MoblinStealthModeChanged {
        on: bool,
        by: Option<String>,
    },
    /// Moblin's connection priorities were turned on or off.
    MoblinConnectionPrioritiesChanged {
        enabled: bool,
        by: Option<String>,
    },
    /// A bonding connection was turned on or off, or got another priority.
    MoblinConnectionChanged {
        name: String,
        enabled: bool,
        priority: u8,
        by: Option<String>,
    },
    /// A command to Moblin failed, e.g. starting it with the OBS stream.
    MoblinError {
        message: String,
    },
}

impl EventDetail {
    /// Splits into the `kind` and `data` columns of `switcher_events`.
    pub fn into_parts(self) -> (String, serde_json::Value) {
        let mut value = serde_json::to_value(self).expect("events serialize");
        let kind = value["kind"].as_str().unwrap_or_default().to_owned();
        let data = value
            .get_mut("data")
            .map(serde_json::Value::take)
            .unwrap_or_else(|| serde_json::json!({}));
        (kind, data)
    }

    /// Rebuilds an event from its `kind` and `data` columns.
    pub fn from_parts(kind: &str, data: serde_json::Value) -> Option<Self> {
        let value = if data.as_object().is_some_and(|object| object.is_empty()) {
            serde_json::json!({ "kind": kind })
        } else {
            serde_json::json!({ "kind": kind, "data": data })
        };
        serde_json::from_value(value).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SwitcherEvent {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    #[serde(flatten)]
    pub detail: EventDetail,
}

/// Sent by the API just before it asks OBS to do something, so obs-link can
/// say who did it when OBS reports the change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub action: IntentAction,
    pub by: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IntentAction {
    Scene { uuid: Uuid },
    Stream { active: bool },
    Record { active: bool },
}

/// Sent by a connection when a browser source identifies; older connections for
/// the same switcher close so only one browser source relays at a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Takeover {
    pub connection_id: Uuid,
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{BelaboxChange, EventDetail, ObsFrames, SwitchReason};

    #[test]
    fn frames_count_from_the_stream_start() {
        let start = ObsFrames {
            render_total: 1000,
            render_skipped: 10,
            output_total: 900,
            output_skipped: 5,
        };
        let now = ObsFrames {
            render_total: 1600,
            render_skipped: 13,
            output_total: 1500,
            output_skipped: 5,
        };
        assert_eq!(
            now.since(start),
            ObsFrames {
                render_total: 600,
                render_skipped: 3,
                output_total: 600,
                output_skipped: 0,
            }
        );

        // OBS restarted: its counters start over, so they're all this stream's.
        let restarted = ObsFrames {
            render_total: 300,
            render_skipped: 1,
            output_total: 300,
            output_skipped: 0,
        };
        assert_eq!(restarted.since(start), restarted);
    }

    #[test]
    fn events_round_trip_through_their_columns() {
        let events = [
            EventDetail::SceneSwitched {
                from: Some("LIVE".into()),
                to: "LOW".into(),
                reason: SwitchReason::Auto,
                by: None,
                server: Some("BELABOX Cloud".into()),
                bitrate: Some(640),
                rtt: None,
            },
            EventDetail::StreamStarted {
                by: Some("mod_kate".into()),
            },
            EventDetail::FeedFixed {
                sources: vec!["BELABOX SRT".into()],
                by: Some("mod_kate (Twitch chat)".into()),
            },
            EventDetail::MarkerCreated {
                marker_id: Uuid::nil(),
                description: Some("Clutch play".into()),
                offset_seconds: 3725,
                by: Some("mod_kate (Twitch chat)".into()),
            },
            EventDetail::SourceToggled {
                source: "Camera".into(),
                scene: "LIVE".into(),
                enabled: false,
                by: Some("kate".into()),
            },
            EventDetail::InputMuted {
                input: "Mic/Aux".into(),
                muted: true,
                by: Some("mod_kate (Twitch chat)".into()),
            },
            EventDetail::ObsDisconnected,
            EventDetail::BelaboxStreamStarted {
                by: Some("the OBS stream".into()),
            },
            EventDetail::BelaboxStreamStopped { by: None },
            EventDetail::BelaboxOnline,
            EventDetail::BelaboxBitrateChanged {
                max_br: 6000,
                by: Some("kate".into()),
            },
            EventDetail::BelaboxInterfaceToggled {
                name: "usb0".into(),
                enabled: false,
                by: Some("mod_kate (Twitch chat)".into()),
            },
            EventDetail::BelaboxConfigured {
                what: BelaboxChange::Hotspot,
                by: Some("kate".into()),
            },
            EventDetail::BelaboxError {
                message: "Failed to start, no available network connections".into(),
            },
            EventDetail::MoblinConnected,
            EventDetail::MoblinStreamStarted {
                by: Some("the OBS stream".into()),
            },
            EventDetail::MoblinMuteChanged {
                muted: true,
                by: None,
            },
            EventDetail::MoblinSceneChanged {
                scene: "Back camera".into(),
                by: Some("kate".into()),
            },
            EventDetail::MoblinBitrateChanged {
                kbps: 5000,
                by: Some("mod_kate (Twitch chat)".into()),
            },
            EventDetail::MoblinConnectionChanged {
                name: "Cellular".into(),
                enabled: false,
                priority: 3,
                by: Some("kate".into()),
            },
        ];

        for event in events {
            let (kind, data) = event.clone().into_parts();
            assert_eq!(EventDetail::from_parts(&kind, data), Some(event));
        }
    }

    #[test]
    fn unit_events_store_an_empty_object() {
        let (kind, data) = EventDetail::ObsConnected.into_parts();
        assert_eq!(kind, "obs_connected");
        assert_eq!(data, serde_json::json!({}));
    }
}

/// Records an event in the activity feed and tells live viewers about it.
///
/// The row is the source of truth; if publishing fails, viewers still see the
/// event when they next load the feed.
pub async fn record_event(
    pool: &db::DbPool,
    nats: &async_nats::Client,
    switcher_id: Uuid,
    detail: EventDetail,
) -> Result<SwitcherEvent, db::Error> {
    let (kind, data) = detail.clone().into_parts();
    let row = db::switcher_events::insert(pool, switcher_id, &kind, data).await?;
    let event = SwitcherEvent {
        id: row.id,
        created_at: row.created_at,
        detail,
    };

    match serde_json::to_vec(&event) {
        Ok(payload) => {
            if let Err(error) = nats
                .publish(event_subject(switcher_id), payload.into())
                .await
            {
                tracing::warn!(%error, "Failed to publish switcher event");
            }
        }
        Err(error) => tracing::warn!(%error, "Failed to serialize switcher event"),
    }

    Ok(event)
}
