//! What the bot says. Keyed by the switcher's `language`; only English is
//! written so far, so every language falls back to it.

use std::collections::BTreeMap;

use live::{BelaboxInterface, BelaboxRefusal, MoblinRefusal, MoblinStatus, MoblinThermal};

/// A reply, with what it needs to be worded.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    ObsOffline,
    Paused,
    ObsRefused(String),
    ObsTimeout,
    Failed,
    /// No scene of this kind (e.g. "Privacy") is picked for the switcher.
    NoScene(&'static str),
    NoSceneMatch(String),
    SwitchedTo(String),
    StreamStarting,
    StreamStopping,
    AlreadyStreaming,
    NotStreaming,
    RecordingStarting,
    RecordingStopping,
    Refreshing,
    AlreadyRefreshing,
    /// `!fix` made these media sources reconnect.
    Fixed(Vec<String>),
    /// `!fix` found no network media source playing on this scene.
    NothingToFix(String),
    /// `!refresh` without a Refresh scene fixed instead; what the fix said.
    FixInstead(Box<Msg>),
    /// `!source` showed (`enabled`) or hid a source.
    SourceToggled {
        name: String,
        enabled: bool,
    },
    NoSourceMatch(String),
    /// `!marker` marked this moment, this many seconds into the stream.
    MarkerAdded {
        offset_seconds: i64,
    },
    NoSceneIds,
    /// Per server: name and what it reports.
    Bitrate(Vec<(String, ServerReading)>),
    NoStats,
    /// OBS's stats, like NOALBS's `!serverinfo`; `bitrate` is `None` while not streaming.
    ServerInfo {
        scene: Option<String>,
        bitrate: Option<u32>,
        stats: Box<control::ObsStats>,
    },
    /// A setting was turned on or off.
    Toggled(&'static str, bool),
    /// A trigger's value (`None` = off).
    Trigger {
        what: &'static str,
        unit: &'static str,
        value: Option<i32>,
    },
    TriggerSet {
        what: &'static str,
        unit: &'static str,
        value: Option<i32>,
    },
    Invalid(String),
    Usage(String),
    AliasAdded {
        alias: String,
        command: String,
        prefix: String,
    },
    AliasRemoved(String),
    AliasTaken(String),
    AliasNotFound(String),
    UnknownCommand(String),
    PrefixSet(String),
    LanguageSet(String),
    Version,
    NotAvailable(String),
    /// The switcher changed scenes on its own (`auto_switch_notification`).
    AutoSwitched {
        scene: String,
        server: Option<String>,
        bitrate: Option<u32>,
        rtt: Option<f64>,
    },
    /// The channel raided out and the stream is being stopped (`auto_stop_on_raid`,
    /// said only with `announce_raid`).
    RaidStopping {
        /// The raided channel's display name, and its URL.
        to: String,
        url: String,
    },
    /// The switcher stopped the stream after the offline timeout (always said).
    OfflineTimeout,
    /// The switcher stopped the stream after the ending timeout (always said).
    EndingTimeout,
    Ignoring(String),
    Unignored(String),
    NotIgnored(String),
    UserNotFound(String),
    /// The BELABOX link isn't running (no browser source connected).
    BelaboxUnreachable,
    BelaboxRefused(BelaboxRefusal),
    BelaboxStarting,
    BelaboxStopping,
    BelaboxBitrate(Option<u32>),
    BelaboxBitrateSet(u32),
    BelaboxInterfaceToggled {
        name: String,
        enabled: bool,
    },
    NoInterfaceMatch(String),
    /// `!bbs`: the BELABOX's interfaces and bitrate.
    BelaboxStats {
        streaming: bool,
        max_br: Option<u32>,
        total_kbps: u32,
        interfaces: Vec<BelaboxInterface>,
        /// The streamer's names for them, by interface key.
        labels: BTreeMap<String, String>,
    },
    BelaboxSensors(Vec<(String, String)>),
    /// No Moblin is connected.
    MoblinUnreachable,
    MoblinRefused(MoblinRefusal),
    MoblinStarting,
    MoblinStopping,
    /// `!mbitrate`: the preset in use (kbps) and all presets.
    MoblinBitrate {
        current: Option<u32>,
        presets: Vec<u32>,
    },
    MoblinBitrateSet(u32),
    NoMoblinPreset {
        kbps: u32,
        presets: Vec<u32>,
    },
    /// `!mscene`: the scene on air and all scenes.
    MoblinScenes {
        current: Option<String>,
        scenes: Vec<String>,
    },
    MoblinSceneSet(String),
    NoMoblinSceneMatch(String),
    /// `!mstats`.
    MoblinStats(Box<MoblinStatus>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServerReading {
    Live { bitrate: u32, rtt: Option<f64> },
    Low { bitrate: u32, rtt: Option<f64> },
    Offline,
}

fn kbps(value: u32) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn reading(bitrate: u32, rtt: Option<f64>) -> String {
    match rtt {
        Some(rtt) => format!("{} kbps, {} ms", kbps(bitrate), rtt.round() as i64),
        None => format!("{} kbps", kbps(bitrate)),
    }
}

/// NOALBS's `!serverinfo` wording.
fn server_info(scene: Option<&str>, bitrate: Option<u32>, stats: &control::ObsStats) -> String {
    let dropped = |d: control::Dropped| format!("{} ({:.1}%)", d.frames, d.percent());
    let mut parts = vec![format!("scene: {}", scene.unwrap_or("unknown"))];
    if stats.streaming {
        parts.push(match bitrate {
            Some(bitrate) => format!("bitrate: {} kbps", kbps(bitrate)),
            None => "bitrate: unknown".into(),
        });
    } else {
        parts.push("not streaming".into());
    }
    parts.push(format!("fps: {}", stats.fps.round()));
    let mut causes = Vec::new();
    if stats.streaming {
        causes.push(format!("network: {}", dropped(stats.network)));
    }
    causes.push(format!("rendering lag: {}", dropped(stats.rendering)));
    causes.push(format!("encoding lag: {}", dropped(stats.encoding)));
    parts.push(format!("dropped due to: {}", causes.join(", ")));
    parts.push(format!("CPU: {:.1}%", stats.cpu_percent));
    parts.push(format!("memory: {:.1} MB", stats.memory_mb));
    parts.push(format!(
        "free disk space: {:.1} GB",
        stats.disk_free_mb / 1024.0
    ));
    parts.join(", ")
}

fn on(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn belabox_refusal(refusal: BelaboxRefusal) -> &'static str {
    match refusal {
        BelaboxRefusal::Connecting => "Still connecting to BELABOX Cloud. Try again in a moment.",
        BelaboxRefusal::InvalidKey => {
            "BELABOX Cloud refused the secret URL. Check it in the switcher's settings."
        }
        BelaboxRefusal::EncoderOffline => "The BELABOX is offline.",
        BelaboxRefusal::NoConfig => {
            "The BELABOX has no stream settings yet. Start it once from its own page."
        }
        BelaboxRefusal::AlreadyStreaming => "The BELABOX is already streaming.",
        BelaboxRefusal::NotStreaming => "The BELABOX isn't streaming.",
        BelaboxRefusal::InvalidBitrate => "The bitrate must be between 300 and 12000 kbps.",
        BelaboxRefusal::UnknownInterface => "The BELABOX has no such network interface.",
        BelaboxRefusal::LastInterface => "The last network interface can't be turned off.",
        // Chat never sends these (WiFi, modems and stream settings are dashboard-only).
        BelaboxRefusal::UnknownWifi
        | BelaboxRefusal::UnknownModem
        | BelaboxRefusal::Streaming
        | BelaboxRefusal::Invalid(_) => "The BELABOX can't do that.",
    }
}

/// Like belabot's `!bbs`: each interface, then the total.
fn belabox_stats(
    streaming: bool,
    max_br: Option<u32>,
    total_kbps: u32,
    interfaces: &[BelaboxInterface],
    labels: &BTreeMap<String, String>,
) -> String {
    let mut parts: Vec<String> = interfaces
        .iter()
        .map(|interface| {
            let mut part = format!("{}: ", interface.display_name(labels));
            if !interface.enabled {
                part.push_str("off");
            } else if let Some(error) = &interface.error {
                part.push_str(error);
            } else {
                part.push_str(&format!("{} kbps", kbps(interface.kbps)));
            }
            if let Some(modem) = &interface.modem {
                let details: Vec<String> = [
                    modem.network.clone(),
                    modem.network_type.clone(),
                    modem.signal.map(|signal| format!("{signal}%")),
                ]
                .into_iter()
                .flatten()
                .collect();
                if !details.is_empty() {
                    part.push_str(&format!(" ({})", details.join(" ")));
                }
            }
            part
        })
        .collect();
    if parts.is_empty() {
        parts.push("no network interfaces".into());
    }
    let state = if streaming {
        match max_br {
            Some(max_br) => format!("streaming, max {} kbps", kbps(max_br)),
            None => "streaming".into(),
        }
    } else {
        "not streaming".into()
    };
    format!(
        "BELABOX ({state}): {} | Total: {} kbps",
        parts.join(" | "),
        kbps(total_kbps)
    )
}

pub fn text(_language: &str, msg: &Msg) -> String {
    english(msg)
}

/// A time into the stream: `4:07`, or `1:02:05` from an hour on.
fn clock(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn english(msg: &Msg) -> String {
    match msg {
        Msg::ObsOffline => "OBS isn't connected right now.".into(),
        Msg::Paused => {
            "This switcher is paused: the workspace has more switchers than its plan includes."
                .into()
        }
        Msg::ObsRefused(reason) => format!("OBS said no: {reason}"),
        Msg::ObsTimeout => "OBS didn't answer. Try again in a moment.".into(),
        Msg::Failed => "Something went wrong. Try again in a moment.".into(),
        Msg::NoScene(kind) => {
            format!("No {kind} scene is picked yet. Choose one on the Scenes page.")
        }
        Msg::NoSceneMatch(query) => format!("No scene matches \"{query}\"."),
        Msg::SwitchedTo(scene) => format!("Switched to {scene}"),
        Msg::StreamStarting => "Starting the stream".into(),
        Msg::StreamStopping => "Stopping the stream".into(),
        Msg::AlreadyStreaming => "Already streaming".into(),
        Msg::NotStreaming => "Not streaming".into(),
        Msg::RecordingStarting => "Recording started".into(),
        Msg::RecordingStopping => "Recording stopped".into(),
        Msg::Refreshing => "Refreshing the feed".into(),
        Msg::AlreadyRefreshing => "Already refreshing".into(),
        Msg::Fixed(sources) => format!("Fixing the feed: reconnecting {}", sources.join(", ")),
        Msg::NothingToFix(scene) => {
            format!("Nothing to fix: no stream source is playing on {scene}")
        }
        Msg::FixInstead(fix) => format!(
            "No Refresh scene is picked, so fixing instead. {}",
            english(fix)
        ),
        Msg::SourceToggled { name, enabled } => {
            format!(
                "{name} is now {}",
                if *enabled { "shown" } else { "hidden" }
            )
        }
        Msg::NoSourceMatch(query) => format!("No source on this scene matches \"{query}\"."),
        Msg::MarkerAdded { offset_seconds } => {
            format!("Marker added at {}", clock(*offset_seconds))
        }
        Msg::NoSceneIds => "This OBS version is too old for that. Update OBS.".into(),
        Msg::Bitrate(servers) => servers
            .iter()
            .map(|(name, server)| {
                let value = match server {
                    ServerReading::Live { bitrate, rtt } => reading(*bitrate, *rtt),
                    ServerReading::Low { bitrate, rtt } => {
                        format!("{} (low)", reading(*bitrate, *rtt))
                    }
                    ServerReading::Offline => "offline".into(),
                };
                if servers.len() == 1 {
                    format!("Current bitrate: {value}")
                } else {
                    format!("{name}: {value}")
                }
            })
            .collect::<Vec<_>>()
            .join(" | "),
        Msg::NoStats => "No stream stats right now.".into(),
        Msg::ServerInfo {
            scene,
            bitrate,
            stats,
        } => server_info(scene.as_deref(), *bitrate, stats),
        Msg::Toggled(what, value) => format!("{what} {}", on(*value)),
        Msg::Trigger { what, unit, value } => match value {
            Some(value) => format!("{what} is {value} {unit}"),
            None => format!("{what} is off"),
        },
        Msg::TriggerSet { what, unit, value } => match value {
            Some(value) => format!("{what} set to {value} {unit}"),
            None => format!("{what} turned off"),
        },
        Msg::Invalid(reason) => reason.clone(),
        Msg::Usage(usage) => format!("Usage: {usage}"),
        Msg::AliasAdded {
            alias,
            command,
            prefix,
        } => format!("{prefix}{alias} now runs {prefix}{command}"),
        Msg::AliasRemoved(alias) => format!("Removed the alias {alias}"),
        Msg::AliasTaken(alias) => format!("{alias} is already taken"),
        Msg::AliasNotFound(alias) => format!("There's no alias {alias}"),
        Msg::UnknownCommand(command) => format!("There's no command {command}"),
        Msg::PrefixSet(prefix) => format!("Commands now start with {prefix}"),
        Msg::LanguageSet(language) => format!("Language set to {language}"),
        Msg::Version => format!("NOALBS Cloud {}", env!("CARGO_PKG_VERSION")),
        Msg::NotAvailable(command) => format!("{command} isn't available in NOALBS Cloud yet."),
        Msg::AutoSwitched {
            scene,
            server,
            bitrate,
            rtt,
        } => match (server, bitrate) {
            (Some(server), Some(bitrate)) => {
                format!(
                    "Switched to {scene} · {server} at {}",
                    reading(*bitrate, *rtt)
                )
            }
            (Some(server), None) => format!("Switched to {scene} · {server}"),
            _ => format!("Switched to {scene}"),
        },
        Msg::RaidStopping { to, url } => {
            format!("The stream has stopped and raided {to} please follow this link {url}")
        }
        Msg::OfflineTimeout => "Offline timeout reached, stopping the stream".into(),
        Msg::EndingTimeout => "Ending scene timeout reached, stopping the stream".into(),
        Msg::Ignoring(name) => format!("Ignoring {name} from now on"),
        Msg::Unignored(name) => format!("No longer ignoring {name}"),
        Msg::NotIgnored(name) => format!("{name} isn't ignored"),
        Msg::UserNotFound(name) => format!("There's no user called {name}"),
        Msg::BelaboxUnreachable => {
            "BELABOX isn't connected right now: is OBS open with the browser source?".into()
        }
        Msg::BelaboxRefused(refusal) => belabox_refusal(*refusal).into(),
        Msg::BelaboxStarting => "Starting the BELABOX stream".into(),
        Msg::BelaboxStopping => "Stopping the BELABOX stream".into(),
        Msg::BelaboxBitrate(Some(max_br)) => format!("BELABOX max bitrate: {} kbps", kbps(*max_br)),
        Msg::BelaboxBitrate(None) => "BELABOX hasn't said its max bitrate yet.".into(),
        Msg::BelaboxBitrateSet(max_br) => {
            format!("BELABOX max bitrate set to {} kbps", kbps(*max_br))
        }
        Msg::BelaboxInterfaceToggled { name, enabled } => {
            format!("Turning {name} {}", on(*enabled))
        }
        Msg::NoInterfaceMatch(query) => format!("BELABOX has no network interface like {query}"),
        Msg::BelaboxStats {
            streaming,
            max_br,
            total_kbps,
            interfaces,
            labels,
        } => belabox_stats(*streaming, *max_br, *total_kbps, interfaces, labels),
        Msg::BelaboxSensors(sensors) if sensors.is_empty() => {
            "BELABOX hasn't sent any sensor readings yet.".into()
        }
        Msg::BelaboxSensors(sensors) => sensors
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect::<Vec<_>>()
            .join(" | "),
        Msg::MoblinUnreachable => "Moblin isn't connected right now.".into(),
        Msg::MoblinRefused(refusal) => moblin_refusal(*refusal).into(),
        Msg::MoblinStarting => "Starting the Moblin stream".into(),
        Msg::MoblinStopping => "Stopping the Moblin stream".into(),
        Msg::MoblinBitrate { current, presets } => {
            let presets = kbps_list(presets);
            match current {
                Some(current) => {
                    format!(
                        "Moblin bitrate: {} kbps · presets: {presets}",
                        kbps(*current)
                    )
                }
                None => format!("Moblin bitrate presets: {presets}"),
            }
        }
        Msg::MoblinBitrateSet(value) => format!("Moblin bitrate set to {} kbps", kbps(*value)),
        Msg::NoMoblinPreset {
            kbps: value,
            presets,
        } => format!(
            "Moblin has no {} kbps preset. Presets: {}",
            kbps(*value),
            kbps_list(presets)
        ),
        Msg::MoblinScenes { current, scenes } => match current {
            Some(current) => format!("Moblin scene: {current} · scenes: {}", scenes.join(", ")),
            None => format!("Moblin scenes: {}", scenes.join(", ")),
        },
        Msg::MoblinSceneSet(scene) => format!("Switching Moblin to {scene}"),
        Msg::NoMoblinSceneMatch(query) => format!("Moblin has no scene like {query}"),
        Msg::MoblinStats(status) => moblin_stats(status),
    }
}

fn kbps_list(values: &[u32]) -> String {
    if values.is_empty() {
        return "none".into();
    }
    let values: Vec<String> = values.iter().map(|value| kbps(*value)).collect();
    format!("{} kbps", values.join(", "))
}

fn moblin_refusal(refusal: MoblinRefusal) -> &'static str {
    match refusal {
        MoblinRefusal::NotConnected => "Moblin isn't connected right now.",
        MoblinRefusal::Paused => {
            "This switcher is paused: the workspace has more switchers than its plan includes."
        }
        MoblinRefusal::AlreadyStreaming => "Moblin is already live.",
        MoblinRefusal::NotStreaming => "Moblin isn't live.",
        MoblinRefusal::UnknownScene => "Moblin has no such scene.",
        MoblinRefusal::UnknownPreset => "Moblin has no such bitrate preset.",
        MoblinRefusal::UnknownZoomPreset => "Moblin has no such zoom preset.",
        MoblinRefusal::InvalidZoom => "The zoom must be between 0.5x and 100x.",
        MoblinRefusal::UnknownConnection => "Moblin has no such connection.",
        MoblinRefusal::LastConnection => "The last connection in use can't be turned off.",
        MoblinRefusal::InvalidPriority => "The priority must be between 1 and 10.",
        MoblinRefusal::Failed => "Moblin refused.",
        MoblinRefusal::Timeout => "Moblin didn't answer. Try again in a moment.",
    }
}

/// `!mstats`: live or not, bitrate, bonding, battery and heat.
fn moblin_stats(status: &MoblinStatus) -> String {
    let mut head = if status.streaming {
        "Moblin (live".to_owned()
    } else {
        "Moblin (not live".to_owned()
    };
    if let Some(uptime) = status.uptime.as_deref().filter(|_| status.streaming) {
        head.push_str(&format!(", {uptime}"));
    }
    head.push(')');

    let mut parts = vec![head];
    if let Some(bitrate) = &status.bitrate {
        parts.push(bitrate.clone());
    }
    if let Some(bonding) = &status.bonding {
        parts.push(format!("Bonding: {bonding}"));
    }
    if let Some(level) = status.battery_level {
        let charging = if status.battery_charging == Some(true) {
            ", charging"
        } else {
            ""
        };
        parts.push(format!("Battery {level}%{charging}"));
    }
    match status.thermal {
        Some(MoblinThermal::Serious) => parts.push("Getting hot".into()),
        Some(MoblinThermal::Critical) => parts.push("Too hot!".into()),
        _ => {}
    }
    if status.muted {
        parts.push("Muted".into());
    }
    parts.join(" | ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Msg, ServerReading, text};
    use live::{BelaboxInterface, BelaboxModem, BelaboxRefusal};

    #[test]
    fn server_info_reads_like_noalbs() {
        let stats = control::ObsStats {
            streaming: true,
            reconnecting: false,
            stream_bytes: 0,
            stream_duration_ms: 0,
            congestion: 0.0,
            network: control::Dropped {
                frames: 3,
                total: 600,
            },
            rendering: control::Dropped {
                frames: 0,
                total: 600,
            },
            encoding: control::Dropped {
                frames: 12,
                total: 600,
            },
            this_stream: true,
            fps: 59.94,
            frame_render_ms: 1.2,
            cpu_percent: 12.34,
            memory_mb: 512.0,
            disk_free_mb: 20480.0,
        };
        let msg = Msg::ServerInfo {
            scene: Some("LIVE".into()),
            bitrate: Some(6000),
            stats: Box::new(stats.clone()),
        };
        assert_eq!(
            text("en", &msg),
            "scene: LIVE, bitrate: 6,000 kbps, fps: 60, dropped due to: network: 3 (0.5%), \
             rendering lag: 0 (0.0%), encoding lag: 12 (2.0%), CPU: 12.3%, memory: 512.0 MB, \
             free disk space: 20.0 GB"
        );

        let idle = Msg::ServerInfo {
            scene: Some("Starting".into()),
            bitrate: None,
            stats: Box::new(control::ObsStats {
                streaming: false,
                ..stats
            }),
        };
        assert!(
            text("en", &idle).starts_with(
                "scene: Starting, not streaming, fps: 60, dropped due to: rendering lag"
            )
        );
    }

    #[test]
    fn belabox_stats_list_interfaces_like_belabot() {
        let usb0 = BelaboxInterface {
            name: "usb0".into(),
            key: "k1".into(),
            kbps: 2340,
            enabled: true,
            error: None,
            modem: Some(BelaboxModem {
                network: Some("Vodafone".into()),
                network_type: Some("4G".into()),
                signal: Some(72),
                ..Default::default()
            }),
        };
        let wlan0 = BelaboxInterface {
            name: "wlan0".into(),
            key: "k2".into(),
            kbps: 0,
            enabled: false,
            error: None,
            modem: None,
        };
        let stats = Msg::BelaboxStats {
            streaming: true,
            max_br: Some(6000),
            total_kbps: 2340,
            interfaces: vec![usb0, wlan0],
            labels: BTreeMap::new(),
        };
        assert_eq!(
            text("en", &stats),
            "BELABOX (streaming, max 6,000 kbps): usb0: 2,340 kbps (Vodafone 4G 72%) | wlan0: off | Total: 2,340 kbps"
        );
        // The streamer's own names replace the interface names.
        let named = Msg::BelaboxStats {
            streaming: false,
            max_br: None,
            total_kbps: 0,
            interfaces: vec![BelaboxInterface {
                name: "usb1".into(),
                key: "k1".into(),
                kbps: 0,
                enabled: false,
                error: None,
                modem: None,
            }],
            labels: BTreeMap::from([("k1".to_owned(), "Phone A".to_owned())]),
        };
        assert_eq!(
            text("en", &named),
            "BELABOX (not streaming): Phone A: off | Total: 0 kbps"
        );
        assert_eq!(
            text("en", &Msg::BelaboxRefused(BelaboxRefusal::EncoderOffline)),
            "The BELABOX is offline."
        );
    }

    #[test]
    fn moblin_replies() {
        let status = live::MoblinStatus {
            streaming: true,
            uptime: Some("0:12:03".into()),
            bitrate: Some("5.1 Mbps, 1.2 GB".into()),
            bonding: Some("62% Cellular, 38% WiFi".into()),
            battery_level: Some(87),
            battery_charging: Some(false),
            thermal: Some(live::MoblinThermal::Serious),
            ..Default::default()
        };
        assert_eq!(
            text("en", &Msg::MoblinStats(Box::new(status))),
            "Moblin (live, 0:12:03) | 5.1 Mbps, 1.2 GB | Bonding: 62% Cellular, 38% WiFi | Battery 87% | Getting hot"
        );
        assert_eq!(
            text("en", &Msg::MoblinStats(Box::default())),
            "Moblin (not live)"
        );
        assert_eq!(
            text(
                "en",
                &Msg::MoblinBitrate {
                    current: Some(6000),
                    presets: vec![1000, 3000, 6000]
                }
            ),
            "Moblin bitrate: 6,000 kbps · presets: 1,000, 3,000, 6,000 kbps"
        );
        assert_eq!(
            text(
                "en",
                &Msg::NoMoblinPreset {
                    kbps: 4500,
                    presets: vec![]
                }
            ),
            "Moblin has no 4,500 kbps preset. Presets: none"
        );
    }

    #[test]
    fn automatic_switches_say_why() {
        let switched = Msg::AutoSwitched {
            scene: "LOW".into(),
            server: Some("BELABOX Cloud".into()),
            bitrate: Some(640),
            rtt: Some(812.0),
        };
        assert_eq!(
            text("en", &switched),
            "Switched to LOW · BELABOX Cloud at 640 kbps, 812 ms"
        );
        let unknown = Msg::AutoSwitched {
            scene: "BRB".into(),
            server: None,
            bitrate: None,
            rtt: None,
        };
        assert_eq!(text("en", &unknown), "Switched to BRB");
    }

    #[test]
    fn raiding_out_links_the_raided_channel() {
        let raid = Msg::RaidStopping {
            to: "Bob".into(),
            url: "https://twitch.tv/bob".into(),
        };
        assert_eq!(
            text("en", &raid),
            "The stream has stopped and raided Bob please follow this link https://twitch.tv/bob"
        );
    }

    #[test]
    fn markers_say_how_far_into_the_stream() {
        let at = |offset_seconds| text("en", &Msg::MarkerAdded { offset_seconds });
        assert_eq!(at(0), "Marker added at 0:00");
        assert_eq!(at(247), "Marker added at 4:07");
        assert_eq!(at(3725), "Marker added at 1:02:05");
    }

    #[test]
    fn fix_names_the_sources() {
        let fixed = Msg::Fixed(vec!["SRT".into(), "IRL".into()]);
        assert_eq!(text("en", &fixed), "Fixing the feed: reconnecting SRT, IRL");
        assert_eq!(
            text(
                "en",
                &Msg::FixInstead(Box::new(Msg::NothingToFix("BRB".into())))
            ),
            "No Refresh scene is picked, so fixing instead. Nothing to fix: no stream source is playing on BRB"
        );
    }

    #[test]
    fn bitrate_names_servers_only_when_there_are_several() {
        let one = Msg::Bitrate(vec![(
            "BELABOX".into(),
            ServerReading::Live {
                bitrate: 5234,
                rtt: Some(120.4),
            },
        )]);
        assert_eq!(text("en", &one), "Current bitrate: 5,234 kbps, 120 ms");
        let two = Msg::Bitrate(vec![
            (
                "BELABOX".into(),
                ServerReading::Low {
                    bitrate: 640,
                    rtt: None,
                },
            ),
            ("MediaMTX".into(), ServerReading::Offline),
        ]);
        assert_eq!(
            text("de", &two),
            "BELABOX: 640 kbps (low) | MediaMTX: offline"
        );
    }
}
