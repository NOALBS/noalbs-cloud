//! What the bot says, in English; `tr!` turns it into the switcher's `language`
//! (see [`crate::i18n`]). Whole sentences only: a translation can't be put
//! together from English pieces.

use std::collections::BTreeMap;

use control::TriggerError;
use db::switchers::ChatFlag;
use live::{BelaboxInterface, BelaboxRefusal, MoblinRefusal, MoblinStatus, MoblinThermal};

use crate::i18n::tr;

/// A reply, with what it needs to be worded.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    ObsOffline,
    Paused,
    /// OBS's own reason, when it gives one, is in English.
    ObsRefused {
        code: u32,
        comment: Option<String>,
    },
    ObsTimeout,
    Failed,
    /// No scene of this kind is picked for the switcher.
    NoScene(SceneKind),
    NoSceneMatch(String),
    /// `!switch` switched, or someone picked the scene in the dashboard
    /// (`dashboard_switch_notification`).
    SwitchedTo(String),
    /// `!live`, `!privacy`, `!starting` or `!ending` switched.
    SwitchedToKind(SceneKind),
    StreamStarting,
    StreamStopping,
    AlreadyStreaming,
    NotStreaming,
    RecordingStarting,
    RecordingStopping,
    Refreshing,
    AlreadyRefreshing,
    /// A fix or stream action is still running for this switcher.
    Busy,
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
    Toggled(ChatFlag, bool),
    /// A trigger's value (`None` = off).
    Trigger {
        trigger: TriggerKind,
        value: Option<i32>,
    },
    TriggerSet {
        trigger: TriggerKind,
        value: Option<i32>,
    },
    InvalidTriggers(TriggerError),
    PrefixTooLong,
    UnknownLanguage,
    Usage {
        prefix: String,
        usage: Usage,
    },
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

/// A scene the switcher has a role for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneKind {
    Live,
    Privacy,
    Starting,
    Ending,
    Refresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    Low,
    Offline,
    Rtt,
    RttOffline,
}

/// What a command takes, for when it got something else.
#[derive(Debug, Clone, PartialEq)]
pub enum Usage {
    Switch,
    Source,
    BelaboxBitrate,
    BelaboxInterface,
    MoblinBitrate,
    OnOff(&'static str),
    BitrateTrigger(String),
    RttTrigger(String),
    Alias,
    Noalbs,
}

/// BELABOX names its sensors in English; one we don't know stays as it sent it.
fn sensor_name(lang: &str, name: &str) -> String {
    match name {
        "SoC temperature" => tr!(lang, "SoC temperature"),
        "SoC voltage" => tr!(lang, "SoC voltage"),
        "SoC current" => tr!(lang, "SoC current"),
        "SRT ingest" => tr!(lang, "SRT ingest"),
        _ => match name.strip_prefix("RTMP ingest - ") {
            Some(stream) => tr!(lang, "RTMP ingest - {stream}", stream = stream),
            None => name.to_owned(),
        },
    }
}

/// How a language writes numbers: what groups thousands, and the decimal mark.
fn number_format(lang: &str) -> (char, char) {
    match lang {
        "da" | "de" | "es" | "it" | "nl" | "pt-br" | "tr" => ('.', ','),
        "fr" | "nb" | "pl" | "ru" | "sv" | "uk" => (' ', ','),
        _ => (',', '.'),
    }
}

fn kbps(lang: &str, value: u32) -> String {
    let (thousands, _) = number_format(lang);
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(thousands);
        }
        out.push(c);
    }
    out
}

/// A number with one decimal.
fn decimal(lang: &str, value: f64) -> String {
    let (_, mark) = number_format(lang);
    format!("{value:.1}").replace('.', &mark.to_string())
}

fn reading(lang: &str, bitrate: u32, rtt: Option<f64>) -> String {
    match rtt {
        Some(rtt) => format!("{} kbps, {} ms", kbps(lang, bitrate), rtt.round() as i64),
        None => format!("{} kbps", kbps(lang, bitrate)),
    }
}

/// NOALBS's `!serverinfo` wording.
fn server_info(
    lang: &str,
    scene: Option<&str>,
    bitrate: Option<u32>,
    stats: &control::ObsStats,
) -> String {
    let dropped = |d: control::Dropped| format!("{} ({}%)", d.frames, decimal(lang, d.percent()));
    let mut parts = vec![match scene {
        Some(scene) => tr!(lang, "scene: {scene}", scene = scene),
        None => tr!(lang, "scene: unknown"),
    }];
    if stats.streaming {
        parts.push(match bitrate {
            Some(bitrate) => tr!(
                lang,
                "bitrate: {bitrate} kbps",
                bitrate = kbps(lang, bitrate)
            ),
            None => tr!(lang, "bitrate: unknown"),
        });
    } else {
        parts.push(tr!(lang, "not streaming"));
    }
    parts.push(tr!(lang, "fps: {fps}", fps = stats.fps.round()));
    let mut causes = Vec::new();
    if stats.streaming {
        causes.push(tr!(
            lang,
            "network: {dropped}",
            dropped = dropped(stats.network)
        ));
    }
    causes.push(tr!(
        lang,
        "rendering lag: {dropped}",
        dropped = dropped(stats.rendering)
    ));
    causes.push(tr!(
        lang,
        "encoding lag: {dropped}",
        dropped = dropped(stats.encoding)
    ));
    parts.push(tr!(
        lang,
        "dropped due to: {causes}",
        causes = causes.join(", ")
    ));
    parts.push(tr!(
        lang,
        "CPU: {percent}%",
        percent = decimal(lang, stats.cpu_percent)
    ));
    parts.push(tr!(
        lang,
        "memory: {memory} MB",
        memory = decimal(lang, stats.memory_mb)
    ));
    parts.push(tr!(
        lang,
        "free disk space: {disk} GB",
        disk = decimal(lang, stats.disk_free_mb / 1024.0)
    ));
    parts.join(", ")
}

fn no_scene(lang: &str, kind: SceneKind) -> String {
    match kind {
        SceneKind::Live => {
            tr!(
                lang,
                "No LIVE scene is picked yet. Choose one on the Scenes page."
            )
        }
        SceneKind::Privacy => {
            tr!(
                lang,
                "No Privacy scene is picked yet. Choose one on the Scenes page."
            )
        }
        SceneKind::Starting => {
            tr!(
                lang,
                "No Starting scene is picked yet. Choose one on the Scenes page."
            )
        }
        SceneKind::Ending => {
            tr!(
                lang,
                "No Ending scene is picked yet. Choose one on the Scenes page."
            )
        }
        SceneKind::Refresh => {
            tr!(
                lang,
                "No Refresh scene is picked yet. Choose one on the Scenes page."
            )
        }
    }
}

fn switched_to_kind(lang: &str, kind: SceneKind) -> String {
    match kind {
        SceneKind::Live => tr!(lang, "Switched to the LIVE scene"),
        SceneKind::Privacy => tr!(lang, "Switched to the Privacy scene"),
        SceneKind::Starting => tr!(lang, "Switched to the Starting scene"),
        SceneKind::Ending => tr!(lang, "Switched to the Ending scene"),
        SceneKind::Refresh => tr!(lang, "Switched to the Refresh scene"),
    }
}

fn toggled(lang: &str, flag: ChatFlag, value: bool) -> String {
    match (flag, value) {
        (ChatFlag::AutoSwitchNotification, true) => tr!(lang, "Automatic switch notifications on"),
        (ChatFlag::AutoSwitchNotification, false) => {
            tr!(lang, "Automatic switch notifications off")
        }
        (ChatFlag::AutoStopOnRaid, true) => tr!(lang, "Auto stop on raid on"),
        (ChatFlag::AutoStopOnRaid, false) => tr!(lang, "Auto stop on raid off"),
        (ChatFlag::PublicCommands, true) => tr!(lang, "Public commands on"),
        (ChatFlag::PublicCommands, false) => tr!(lang, "Public commands off"),
        (ChatFlag::ModCommands, true) => tr!(lang, "Mod commands on"),
        (ChatFlag::ModCommands, false) => tr!(lang, "Mod commands off"),
    }
}

fn trigger(lang: &str, trigger: TriggerKind, value: Option<i32>) -> String {
    match (trigger, value) {
        (TriggerKind::Low, Some(value)) => {
            tr!(lang, "The LOW trigger is {value} kbps", value = value)
        }
        (TriggerKind::Low, None) => tr!(lang, "The LOW trigger is off"),
        (TriggerKind::Offline, Some(value)) => {
            tr!(lang, "The offline trigger is {value} kbps", value = value)
        }
        (TriggerKind::Offline, None) => tr!(lang, "The offline trigger is off"),
        (TriggerKind::Rtt, Some(value)) => {
            tr!(lang, "The RTT trigger is {value} ms", value = value)
        }
        (TriggerKind::Rtt, None) => tr!(lang, "The RTT trigger is off"),
        (TriggerKind::RttOffline, Some(value)) => {
            tr!(lang, "The offline RTT trigger is {value} ms", value = value)
        }
        (TriggerKind::RttOffline, None) => tr!(lang, "The offline RTT trigger is off"),
    }
}

fn trigger_set(lang: &str, trigger: TriggerKind, value: Option<i32>) -> String {
    match (trigger, value) {
        (TriggerKind::Low, Some(value)) => {
            tr!(lang, "The LOW trigger set to {value} kbps", value = value)
        }
        (TriggerKind::Low, None) => tr!(lang, "The LOW trigger turned off"),
        (TriggerKind::Offline, Some(value)) => {
            tr!(
                lang,
                "The offline trigger set to {value} kbps",
                value = value
            )
        }
        (TriggerKind::Offline, None) => tr!(lang, "The offline trigger turned off"),
        (TriggerKind::Rtt, Some(value)) => {
            tr!(lang, "The RTT trigger set to {value} ms", value = value)
        }
        (TriggerKind::Rtt, None) => tr!(lang, "The RTT trigger turned off"),
        (TriggerKind::RttOffline, Some(value)) => {
            tr!(
                lang,
                "The offline RTT trigger set to {value} ms",
                value = value
            )
        }
        (TriggerKind::RttOffline, None) => tr!(lang, "The offline RTT trigger turned off"),
    }
}

fn invalid_triggers(lang: &str, error: TriggerError) -> String {
    match error {
        TriggerError::BitrateRange => {
            tr!(lang, "Bitrate triggers must be between 1 and 100,000 kbps")
        }
        TriggerError::RttRange => tr!(lang, "RTT triggers must be between 1 and 60,000 ms"),
        TriggerError::OfflineBitrate => {
            tr!(
                lang,
                "The offline bitrate must be lower than the LOW bitrate"
            )
        }
        TriggerError::OfflineRtt => {
            tr!(lang, "The offline RTT must be higher than the LOW RTT")
        }
    }
}

fn usage(lang: &str, prefix: &str, usage: &Usage) -> String {
    match usage {
        Usage::Switch => tr!(lang, "Usage: {prefix}switch <scene name>", prefix = prefix),
        Usage::Source => tr!(lang, "Usage: {prefix}source <source name>", prefix = prefix),
        Usage::BelaboxBitrate => tr!(
            lang,
            "Usage: {prefix}bbb <{min}-{max} kbps>",
            prefix = prefix,
            min = live::BELABOX_MIN_BITRATE,
            max = live::BELABOX_MAX_BITRATE
        ),
        Usage::BelaboxInterface => {
            tr!(
                lang,
                "Usage: {prefix}bbt <interface, e.g. usb0>",
                prefix = prefix
            )
        }
        Usage::MoblinBitrate => {
            tr!(
                lang,
                "Usage: {prefix}mbitrate <kbps of a preset>",
                prefix = prefix
            )
        }
        Usage::OnOff(command) => tr!(
            lang,
            "Usage: {prefix}{command} [on|off]",
            prefix = prefix,
            command = command
        ),
        Usage::BitrateTrigger(command) => tr!(
            lang,
            "Usage: {prefix}{command} <kbps|off>",
            prefix = prefix,
            command = command
        ),
        Usage::RttTrigger(command) => tr!(
            lang,
            "Usage: {prefix}{command} <ms|off>",
            prefix = prefix,
            command = command
        ),
        Usage::Alias => tr!(
            lang,
            "Usage: {prefix}alias <alias> <command> | {prefix}alias rem <alias>",
            prefix = prefix
        ),
        Usage::Noalbs => tr!(
            lang,
            "Usage: {prefix}noalbs version | prefix <new> | lang <code>",
            prefix = prefix
        ),
    }
}

fn belabox_refusal(lang: &str, refusal: BelaboxRefusal) -> String {
    match refusal {
        BelaboxRefusal::Connecting => {
            tr!(
                lang,
                "Still connecting to BELABOX Cloud. Try again in a moment."
            )
        }
        BelaboxRefusal::InvalidKey => {
            tr!(
                lang,
                "BELABOX Cloud refused the secret URL. Check it in the switcher's settings."
            )
        }
        BelaboxRefusal::EncoderOffline => tr!(lang, "The BELABOX is offline."),
        BelaboxRefusal::NoConfig => {
            tr!(
                lang,
                "The BELABOX has no stream settings yet. Start it once from its own page."
            )
        }
        BelaboxRefusal::AlreadyStreaming => tr!(lang, "The BELABOX is already streaming."),
        BelaboxRefusal::NotStreaming => tr!(lang, "The BELABOX isn't streaming."),
        BelaboxRefusal::InvalidBitrate => {
            tr!(lang, "The bitrate must be between 300 and 12000 kbps.")
        }
        BelaboxRefusal::UnknownInterface => tr!(lang, "The BELABOX has no such network interface."),
        BelaboxRefusal::LastInterface => {
            tr!(lang, "The last network interface can't be turned off.")
        }
        // Chat never sends these (WiFi, modems and stream settings are dashboard-only).
        BelaboxRefusal::UnknownWifi
        | BelaboxRefusal::UnknownModem
        | BelaboxRefusal::Streaming
        | BelaboxRefusal::Invalid(_) => tr!(lang, "The BELABOX can't do that."),
    }
}

/// Like belabot's `!bbs`: each interface, then the total.
fn belabox_stats(
    lang: &str,
    streaming: bool,
    max_br: Option<u32>,
    total_kbps: u32,
    interfaces: &[BelaboxInterface],
    labels: &BTreeMap<String, String>,
) -> String {
    let mut parts: Vec<String> = interfaces
        .iter()
        .map(|interface| {
            let name = interface.display_name(labels);
            let mut part = if !interface.enabled {
                tr!(lang, "{name}: off", name = name)
            } else if let Some(error) = &interface.error {
                format!("{name}: {error}")
            } else {
                format!("{name}: {} kbps", kbps(lang, interface.kbps))
            };
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
        parts.push(tr!(lang, "no network interfaces"));
    }
    let state = if streaming {
        match max_br {
            Some(max_br) => tr!(
                lang,
                "streaming, max {bitrate} kbps",
                bitrate = kbps(lang, max_br)
            ),
            None => tr!(lang, "streaming"),
        }
    } else {
        tr!(lang, "not streaming")
    };
    tr!(
        lang,
        "BELABOX ({state}): {interfaces} | Total: {total} kbps",
        state = state,
        interfaces = parts.join(" | "),
        total = kbps(lang, total_kbps)
    )
}

pub fn text(language: &str, msg: &Msg) -> String {
    match msg {
        // Said in the new language, to show it worked.
        Msg::LanguageSet(new) => render(new, msg),
        _ => render(language, msg),
    }
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

fn render(lang: &str, msg: &Msg) -> String {
    match msg {
        Msg::ObsOffline => tr!(lang, "OBS isn't connected right now."),
        Msg::Paused => paused(lang),
        Msg::ObsRefused {
            comment: Some(reason),
            ..
        } => tr!(lang, "OBS said no: {reason}", reason = reason),
        Msg::ObsRefused {
            code,
            comment: None,
        } => tr!(lang, "OBS said no: code {code}", code = code),
        Msg::ObsTimeout => tr!(lang, "OBS didn't answer. Try again in a moment."),
        Msg::Failed => tr!(lang, "Something went wrong. Try again in a moment."),
        Msg::NoScene(kind) => no_scene(lang, *kind),
        Msg::NoSceneMatch(query) => tr!(lang, "No scene matches \"{query}\".", query = query),
        Msg::SwitchedTo(scene) => tr!(lang, "Switched to {scene}", scene = scene),
        Msg::SwitchedToKind(kind) => switched_to_kind(lang, *kind),
        Msg::StreamStarting => tr!(lang, "Starting the stream"),
        Msg::StreamStopping => tr!(lang, "Stopping the stream"),
        Msg::AlreadyStreaming => tr!(lang, "Already streaming"),
        Msg::NotStreaming => tr!(lang, "Not streaming"),
        Msg::RecordingStarting => tr!(lang, "Recording started"),
        Msg::RecordingStopping => tr!(lang, "Recording stopped"),
        Msg::Refreshing => tr!(lang, "Refreshing the feed"),
        Msg::AlreadyRefreshing => tr!(lang, "Already refreshing"),
        Msg::Busy => tr!(
            lang,
            "Another action is still running; try again in a few seconds"
        ),
        Msg::Fixed(sources) => tr!(
            lang,
            "Fixing the feed: reconnecting {sources}",
            sources = sources.join(", ")
        ),
        Msg::NothingToFix(scene) => tr!(
            lang,
            "Nothing to fix: no stream source is playing on {scene}",
            scene = scene
        ),
        Msg::FixInstead(fix) => tr!(
            lang,
            "No Refresh scene is picked, so fixing instead. {result}",
            result = render(lang, fix)
        ),
        Msg::SourceToggled {
            name,
            enabled: true,
        } => tr!(lang, "{name} is now shown", name = name),
        Msg::SourceToggled {
            name,
            enabled: false,
        } => tr!(lang, "{name} is now hidden", name = name),
        Msg::NoSourceMatch(query) => {
            tr!(
                lang,
                "No source on this scene matches \"{query}\".",
                query = query
            )
        }
        Msg::MarkerAdded { offset_seconds } => {
            tr!(
                lang,
                "Marker added at {time}",
                time = clock(*offset_seconds)
            )
        }
        Msg::NoSceneIds => tr!(lang, "This OBS version is too old for that. Update OBS."),
        Msg::Bitrate(servers) => servers
            .iter()
            .map(|(name, server)| {
                let value = match server {
                    ServerReading::Live { bitrate, rtt } => reading(lang, *bitrate, *rtt),
                    ServerReading::Low { bitrate, rtt } => tr!(
                        lang,
                        "{reading} (low)",
                        reading = reading(lang, *bitrate, *rtt)
                    ),
                    ServerReading::Offline => tr!(lang, "offline"),
                };
                if servers.len() == 1 {
                    tr!(lang, "Current bitrate: {reading}", reading = value)
                } else {
                    format!("{name}: {value}")
                }
            })
            .collect::<Vec<_>>()
            .join(" | "),
        Msg::NoStats => tr!(lang, "No stream stats right now."),
        Msg::ServerInfo {
            scene,
            bitrate,
            stats,
        } => server_info(lang, scene.as_deref(), *bitrate, stats),
        Msg::Toggled(flag, value) => toggled(lang, *flag, *value),
        Msg::Trigger {
            trigger: kind,
            value,
        } => trigger(lang, *kind, *value),
        Msg::TriggerSet {
            trigger: kind,
            value,
        } => trigger_set(lang, *kind, *value),
        Msg::InvalidTriggers(error) => invalid_triggers(lang, *error),
        Msg::PrefixTooLong => tr!(lang, "The prefix must be 1 to 3 characters without spaces"),
        Msg::UnknownLanguage => tr!(
            lang,
            "Languages: {languages}",
            languages = db::chat_commands::LANGUAGES.join(", ")
        ),
        Msg::Usage {
            prefix,
            usage: wanted,
        } => usage(lang, prefix, wanted),
        Msg::AliasAdded {
            alias,
            command,
            prefix,
        } => tr!(
            lang,
            "{prefix}{alias} now runs {prefix}{command}",
            prefix = prefix,
            alias = alias,
            command = command
        ),
        Msg::AliasRemoved(alias) => tr!(lang, "Removed the alias {alias}", alias = alias),
        Msg::AliasTaken(alias) => tr!(lang, "{alias} is already taken", alias = alias),
        Msg::AliasNotFound(alias) => tr!(lang, "There's no alias {alias}", alias = alias),
        Msg::UnknownCommand(command) => {
            tr!(lang, "There's no command {command}", command = command)
        }
        Msg::PrefixSet(prefix) => tr!(lang, "Commands now start with {prefix}", prefix = prefix),
        Msg::LanguageSet(language) => {
            tr!(lang, "Language set to {language}", language = language)
        }
        Msg::Version => format!("NOALBS Cloud {}", env!("CARGO_PKG_VERSION")),
        Msg::NotAvailable(command) => {
            tr!(
                lang,
                "{command} isn't available in NOALBS Cloud yet.",
                command = command
            )
        }
        Msg::AutoSwitched {
            scene,
            server,
            bitrate,
            rtt,
        } => match (server, bitrate) {
            (Some(server), Some(bitrate)) => tr!(
                lang,
                "Switched to {scene} · {server} at {reading}",
                scene = scene,
                server = server,
                reading = reading(lang, *bitrate, *rtt)
            ),
            (Some(server), None) => tr!(
                lang,
                "Switched to {scene} · {server}",
                scene = scene,
                server = server
            ),
            _ => tr!(lang, "Switched to {scene}", scene = scene),
        },
        Msg::RaidStopping { to, url } => tr!(
            lang,
            "The stream has stopped and raided {to} please follow this link {url}",
            to = to,
            url = url
        ),
        Msg::OfflineTimeout => tr!(lang, "Offline timeout reached, stopping the stream"),
        Msg::EndingTimeout => tr!(lang, "Ending scene timeout reached, stopping the stream"),
        Msg::Ignoring(name) => tr!(lang, "Ignoring {name} from now on", name = name),
        Msg::Unignored(name) => tr!(lang, "No longer ignoring {name}", name = name),
        Msg::NotIgnored(name) => tr!(lang, "{name} isn't ignored", name = name),
        Msg::UserNotFound(name) => tr!(lang, "There's no user called {name}", name = name),
        Msg::BelaboxUnreachable => {
            tr!(
                lang,
                "BELABOX isn't connected right now: is OBS open with the browser source?"
            )
        }
        Msg::BelaboxRefused(refusal) => belabox_refusal(lang, *refusal),
        Msg::BelaboxStarting => tr!(lang, "Starting the BELABOX stream"),
        Msg::BelaboxStopping => tr!(lang, "Stopping the BELABOX stream"),
        Msg::BelaboxBitrate(Some(max_br)) => tr!(
            lang,
            "BELABOX max bitrate: {bitrate} kbps",
            bitrate = kbps(lang, *max_br)
        ),
        Msg::BelaboxBitrate(None) => tr!(lang, "BELABOX hasn't said its max bitrate yet."),
        Msg::BelaboxBitrateSet(max_br) => tr!(
            lang,
            "BELABOX max bitrate set to {bitrate} kbps",
            bitrate = kbps(lang, *max_br)
        ),
        Msg::BelaboxInterfaceToggled {
            name,
            enabled: true,
        } => tr!(lang, "Turning {name} on", name = name),
        Msg::BelaboxInterfaceToggled {
            name,
            enabled: false,
        } => tr!(lang, "Turning {name} off", name = name),
        Msg::NoInterfaceMatch(query) => {
            tr!(
                lang,
                "BELABOX has no network interface like {query}",
                query = query
            )
        }
        Msg::BelaboxStats {
            streaming,
            max_br,
            total_kbps,
            interfaces,
            labels,
        } => belabox_stats(lang, *streaming, *max_br, *total_kbps, interfaces, labels),
        Msg::BelaboxSensors(sensors) if sensors.is_empty() => {
            tr!(lang, "BELABOX hasn't sent any sensor readings yet.")
        }
        Msg::BelaboxSensors(sensors) => sensors
            .iter()
            .map(|(name, value)| format!("{}: {value}", sensor_name(lang, name)))
            .collect::<Vec<_>>()
            .join(" | "),
        Msg::MoblinUnreachable => moblin_unreachable(lang),
        Msg::MoblinRefused(refusal) => moblin_refusal(lang, *refusal),
        Msg::MoblinStarting => tr!(lang, "Starting the Moblin stream"),
        Msg::MoblinStopping => tr!(lang, "Stopping the Moblin stream"),
        Msg::MoblinBitrate { current, presets } => {
            let presets = kbps_list(lang, presets);
            match current {
                Some(current) => tr!(
                    lang,
                    "Moblin bitrate: {bitrate} kbps · presets: {presets}",
                    bitrate = kbps(lang, *current),
                    presets = presets
                ),
                None => tr!(lang, "Moblin bitrate presets: {presets}", presets = presets),
            }
        }
        Msg::MoblinBitrateSet(value) => tr!(
            lang,
            "Moblin bitrate set to {bitrate} kbps",
            bitrate = kbps(lang, *value)
        ),
        Msg::NoMoblinPreset {
            kbps: value,
            presets,
        } => tr!(
            lang,
            "Moblin has no {bitrate} kbps preset. Presets: {presets}",
            bitrate = kbps(lang, *value),
            presets = kbps_list(lang, presets)
        ),
        Msg::MoblinScenes { current, scenes } => match current {
            Some(current) => tr!(
                lang,
                "Moblin scene: {scene} · scenes: {scenes}",
                scene = current,
                scenes = scenes.join(", ")
            ),
            None => tr!(lang, "Moblin scenes: {scenes}", scenes = scenes.join(", ")),
        },
        Msg::MoblinSceneSet(scene) => tr!(lang, "Switching Moblin to {scene}", scene = scene),
        Msg::NoMoblinSceneMatch(query) => {
            tr!(lang, "Moblin has no scene like {query}", query = query)
        }
        Msg::MoblinStats(status) => moblin_stats(lang, status),
    }
}

fn paused(lang: &str) -> String {
    tr!(
        lang,
        "This switcher is paused: the workspace has more switchers than its plan includes."
    )
}

fn moblin_unreachable(lang: &str) -> String {
    tr!(lang, "Moblin isn't connected right now.")
}

fn kbps_list(lang: &str, values: &[u32]) -> String {
    if values.is_empty() {
        return tr!(lang, "none");
    }
    let values: Vec<String> = values.iter().map(|value| kbps(lang, *value)).collect();
    format!("{} kbps", values.join(", "))
}

fn moblin_refusal(lang: &str, refusal: MoblinRefusal) -> String {
    match refusal {
        MoblinRefusal::NotConnected => moblin_unreachable(lang),
        MoblinRefusal::Paused => paused(lang),
        MoblinRefusal::AlreadyStreaming => tr!(lang, "Moblin is already live."),
        MoblinRefusal::NotStreaming => tr!(lang, "Moblin isn't live."),
        MoblinRefusal::UnknownScene => tr!(lang, "Moblin has no such scene."),
        MoblinRefusal::UnknownPreset => tr!(lang, "Moblin has no such bitrate preset."),
        MoblinRefusal::UnknownZoomPreset => tr!(lang, "Moblin has no such zoom preset."),
        MoblinRefusal::InvalidZoom => tr!(lang, "The zoom must be between 0.5x and 100x."),
        MoblinRefusal::UnknownConnection => tr!(lang, "Moblin has no such connection."),
        MoblinRefusal::LastConnection => {
            tr!(lang, "The last connection in use can't be turned off.")
        }
        MoblinRefusal::InvalidPriority => tr!(lang, "The priority must be between 1 and 10."),
        MoblinRefusal::Failed => tr!(lang, "Moblin refused."),
        MoblinRefusal::Timeout => tr!(lang, "Moblin didn't answer. Try again in a moment."),
    }
}

/// `!mstats`: live or not, bitrate, bonding, battery and heat.
fn moblin_stats(lang: &str, status: &MoblinStatus) -> String {
    let head = match status.uptime.as_deref() {
        Some(uptime) if status.streaming => tr!(lang, "Moblin (live, {uptime})", uptime = uptime),
        _ if status.streaming => tr!(lang, "Moblin (live)"),
        _ => tr!(lang, "Moblin (not live)"),
    };

    let mut parts = vec![head];
    if let Some(bitrate) = &status.bitrate {
        parts.push(bitrate.clone());
    }
    // What each connection sends, from Moblin versions that say; otherwise their shares.
    let bonding = if status.bonding_connections.is_empty() {
        status.bonding.clone()
    } else {
        let connections: Vec<String> = status
            .bonding_connections
            .iter()
            .map(|connection| format!("{} {} kbps", connection.name, kbps(lang, connection.kbps)))
            .collect();
        Some(connections.join(", "))
    };
    if let Some(bonding) = bonding {
        parts.push(tr!(lang, "Bonding: {bonding}", bonding = bonding));
    }
    if let Some(level) = status.battery_level {
        parts.push(if status.battery_charging == Some(true) {
            tr!(lang, "Battery {level}%, charging", level = level)
        } else {
            tr!(lang, "Battery {level}%", level = level)
        });
    }
    match status.thermal {
        Some(MoblinThermal::Serious) => parts.push(tr!(lang, "Getting hot")),
        Some(MoblinThermal::Critical) => parts.push(tr!(lang, "Too hot!")),
        _ => {}
    }
    if status.muted {
        parts.push(tr!(lang, "Muted"));
    }
    parts.join(" | ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use db::switchers::ChatFlag;

    use super::{Msg, SceneKind, ServerReading, TriggerKind, Usage, text};
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
        let with_numbers = live::MoblinStatus {
            streaming: true,
            bonding: Some("62% Cellular, 38% WiFi".into()),
            bonding_connections: vec![
                live::MoblinBondingConnection {
                    name: "Cellular".into(),
                    kbps: 3200,
                    rtt_ms: Some(48),
                },
                live::MoblinBondingConnection {
                    name: "WiFi".into(),
                    kbps: 1960,
                    rtt_ms: None,
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            text("en", &Msg::MoblinStats(Box::new(with_numbers))),
            "Moblin (live) | Bonding: Cellular 3,200 kbps, WiFi 1,960 kbps"
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
            text("en", &two),
            "BELABOX: 640 kbps (low) | MediaMTX: offline"
        );
        assert_eq!(
            text("de", &two),
            "BELABOX: 640 kbps (niedrig) | MediaMTX: offline"
        );
    }

    #[test]
    fn replies_are_in_the_switchers_language() {
        let switched = Msg::SwitchedTo("LIVE".into());
        assert_eq!(text("en", &switched), "Switched to LIVE");
        assert_eq!(text("nl", &switched), "Overgeschakeld naar LIVE");
        assert_eq!(text("zh-tw", &switched), "已切換至 LIVE");
        // A language we don't know, or a code from before it was checked.
        assert_eq!(text("xx", &switched), "Switched to LIVE");
    }

    #[test]
    fn a_new_language_is_confirmed_in_that_language() {
        let set = Msg::LanguageSet("de".into());
        assert_eq!(text("en", &set), "Sprache auf de gesetzt");
        let back = Msg::LanguageSet("en".into());
        assert_eq!(text("de", &back), "Language set to en");
    }

    #[test]
    fn belabox_sensors_are_named_in_the_language() {
        let sensors = Msg::BelaboxSensors(vec![
            ("SoC temperature".to_owned(), "45.3 °C".to_owned()),
            ("RTMP ingest - live".to_owned(), "2048 Kbps".to_owned()),
            ("Something new".to_owned(), "1".to_owned()),
        ]);
        assert_eq!(
            text("en", &sensors),
            "SoC temperature: 45.3 °C | RTMP ingest - live: 2048 Kbps | Something new: 1"
        );
        assert_eq!(
            text("de", &sensors),
            "SoC-Temperatur: 45.3 °C | RTMP-Eingang - live: 2048 Kbps | Something new: 1"
        );
    }

    #[test]
    fn numbers_are_written_the_languages_way() {
        let bitrate = Msg::BelaboxBitrateSet(12000);
        assert_eq!(
            text("en", &bitrate),
            "BELABOX max bitrate set to 12,000 kbps"
        );
        assert_eq!(
            text("de", &bitrate),
            "Maximale BELABOX-Bitrate auf 12.000 kbps gesetzt"
        );
        assert_eq!(
            text("fr", &bitrate),
            "Débit max BELABOX réglé à 12 000 kbps"
        );
        assert_eq!(
            text("da", &bitrate),
            "BELABOX maks. bitrate sat til 12.000 kbps"
        );
        assert_eq!(
            text("ru", &bitrate),
            "Макс. битрейт BELABOX установлен на 12 000 kbps"
        );
    }

    #[test]
    fn serverinfo_uses_the_languages_decimal_mark() {
        let info = Msg::ServerInfo {
            scene: None,
            bitrate: None,
            stats: Box::new(control::ObsStats {
                streaming: false,
                reconnecting: false,
                stream_bytes: 0,
                stream_duration_ms: 0,
                congestion: 0.0,
                network: control::Dropped {
                    frames: 0,
                    total: 0,
                },
                rendering: control::Dropped {
                    frames: 0,
                    total: 0,
                },
                encoding: control::Dropped {
                    frames: 0,
                    total: 0,
                },
                this_stream: false,
                fps: 30.0,
                frame_render_ms: 0.0,
                cpu_percent: 12.34,
                memory_mb: 512.0,
                disk_free_mb: 2048.0,
            }),
        };
        let de = text("de", &info);
        assert!(
            de.starts_with("Szene: unbekannt, streamt nicht, fps: 30"),
            "{de}"
        );
        assert!(de.contains("CPU: 12,3 %"), "{de}");
        assert!(de.contains("freier Speicherplatz: 2,0 GB"), "{de}");
        // Turkish puts the percent sign first.
        assert!(text("tr", &info).contains("CPU: %12,3"));
    }

    #[test]
    fn typed_settings_read_as_whole_sentences() {
        assert_eq!(
            text("en", &Msg::Toggled(ChatFlag::ModCommands, false)),
            "Mod commands off"
        );
        assert_eq!(
            text(
                "en",
                &Msg::Trigger {
                    trigger: TriggerKind::RttOffline,
                    value: Some(500)
                }
            ),
            "The offline RTT trigger is 500 ms"
        );
        assert_eq!(
            text(
                "en",
                &Msg::TriggerSet {
                    trigger: TriggerKind::Low,
                    value: None
                }
            ),
            "The LOW trigger turned off"
        );
        assert_eq!(
            text("en", &Msg::NoScene(SceneKind::Privacy)),
            "No Privacy scene is picked yet. Choose one on the Scenes page."
        );
        assert_eq!(
            text("en", &Msg::SwitchedToKind(SceneKind::Live)),
            "Switched to the LIVE scene"
        );
        assert_eq!(
            text(
                "en",
                &Msg::Usage {
                    prefix: "!".into(),
                    usage: Usage::OnOff("notify")
                }
            ),
            "Usage: !notify [on|off]"
        );
        assert_eq!(
            text(
                "en",
                &Msg::ObsRefused {
                    code: 702,
                    comment: None
                }
            ),
            "OBS said no: code 702"
        );
    }
}
