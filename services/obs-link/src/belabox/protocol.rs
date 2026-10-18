//! BELABOX Cloud's remote protocol, as belaUI (`ws_nodejs` branch) and belabot
//! speak it: what arrives is folded into a [`State`], which also builds what we
//! send. Pure, so it's tested without a socket.
//!
//! Every frame is a JSON object keyed by message type (`netif`, `status`, …),
//! sometimes with an extra `id` (the client a reply is for), so each key is
//! handled on its own. The BELABOX's types are looser than belabot assumed
//! (fields missing on devices that never streamed), so everything is optional.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use chrono::Utc;
use live::{
    BelaboxAction, BelaboxChoice, BelaboxDetails, BelaboxField, BelaboxHotspot, BelaboxInterface,
    BelaboxLink, BelaboxModem, BelaboxModemConfig, BelaboxModemDetails, BelaboxModemNetwork,
    BelaboxNotice, BelaboxNoticeKind, BelaboxPipeline, BelaboxRefusal, BelaboxRelayAccount,
    BelaboxRelayServer, BelaboxResult, BelaboxResultKind, BelaboxSensor, BelaboxSettingsUpdate,
    BelaboxStatus, BelaboxStreamSettings, BelaboxWifiDevice, BelaboxWifiNetwork,
};
use serde_json::{Map, Value, json};

/// The remote protocol version we speak: belaUI's current one. belabot says 6, but the
/// WiFi manager, relay accounts, hotspot and modem manager are versions 5, 12, 13 and 14,
/// and BELABOX Cloud may send a client only what its version knows. Not yet tried against
/// the real BELABOX Cloud.
const PROTOCOL_VERSION: u32 = 16;

/// The saved settings `start` copies (belaUI's `getConfig()`); the rest of the
/// config (remote key, SSH password, …) never leaves.
const START_FIELDS: &[&str] = &[
    "pipeline",
    "delay",
    "max_br",
    "srt_latency",
    "bitrate_overlay",
    "asrc",
    "acodec",
    "relay_server",
    "relay_account",
    "srtla_addr",
    "srtla_port",
    "srt_streamid",
];

pub fn auth(key: &str) -> String {
    json!({ "remote": { "auth/key": { "key": key, "version": PROTOCOL_VERSION } } }).to_string()
}

pub fn keepalive() -> String {
    json!({ "keepalive": null }).to_string()
}

/// Something worth telling the connection (or the activity feed) about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// BELABOX Cloud accepted (`true`) or refused the remote key.
    Authenticated(bool),
    /// The BELABOX came online or went offline (not reported for the first
    /// reading).
    EncoderOnline(bool),
    Streaming(bool),
    InterfaceToggled {
        name: String,
        enabled: bool,
    },
    /// A new error notification from the BELABOX.
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Netif {
    ip: Option<String>,
    /// Bytes sent over the last second.
    tp: u64,
    enabled: bool,
    error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Modem {
    ifname: Option<String>,
    name: Option<String>,
    network_types: Vec<String>,
    network_type: Option<String>,
    /// `None` without a SIM.
    config: Option<ModemConfig>,
    /// Operator ID → (name, availability), from the last scan.
    networks: BTreeMap<String, (String, Option<String>)>,
    status: Option<BelaboxModem>,
}

/// A modem's APN settings. The password stays here: the dialog only learns that
/// there is one, and a save without a new one sends this back.
#[derive(Debug, Clone, Default)]
struct ModemConfig {
    autoconfig: Option<bool>,
    apn: String,
    username: String,
    password: String,
    roaming: bool,
    network: String,
}

#[derive(Debug, Clone)]
struct Scan {
    ssid: String,
    active: bool,
    signal: Option<u32>,
    security: Option<String>,
    frequency: Option<u32>,
}

#[derive(Debug, Clone)]
struct Hotspot {
    name: String,
    /// Kept for a save that leaves the password out.
    password: Option<String>,
    channel: String,
    channels: Vec<BelaboxChoice>,
    warnings: Vec<String>,
}

/// A WiFi adapter, as `status.wifi` lists it.
#[derive(Debug, Clone, Default)]
struct WifiAdapter {
    interface: String,
    hardware: Option<String>,
    /// UUID of the connection it is on.
    connection: Option<String>,
    available: Vec<Scan>,
    /// SSID → connection UUID.
    saved: BTreeMap<String, String>,
    /// Set while it is a hotspot.
    hotspot: Option<Hotspot>,
    supports_hotspot: bool,
}

#[derive(Debug, Clone)]
struct PipelineInfo {
    name: String,
    asrc: bool,
    acodec: bool,
}

#[derive(Debug, Clone)]
struct Notice {
    notice: BelaboxNotice,
    /// `None`: until BELABOX removes it.
    expires: Option<Instant>,
}

/// What we know about the BELABOX. It outlives reconnects to BELABOX Cloud, so
/// only real changes of the BELABOX become [`Change`]s.
#[derive(Debug, Default)]
pub struct State {
    connected: bool,
    authenticated: bool,
    invalid_key: bool,
    /// `None` until BELABOX Cloud says.
    online: Option<bool>,
    /// `None` until the BELABOX says (and again once it goes offline).
    streaming: Option<bool>,
    config: Option<Map<String, Value>>,
    max_br: Option<u32>,
    /// A max bitrate set while not streaming (BELABOX ignores those), sent with
    /// the next `start`; until then it's what the status shows.
    next_max_br: Option<u32>,
    /// Stream settings changed here (config key → value, `null` = remove it): BELABOX
    /// only saves them when it starts, so our next `start` carries them.
    pending: Map<String, Value>,
    pipelines: BTreeMap<String, PipelineInfo>,
    /// Relay server ID → (name, default).
    relay_servers: BTreeMap<String, (String, bool)>,
    /// Relay account ID → (name, disabled).
    relay_accounts: BTreeMap<String, (String, bool)>,
    /// Audio input names.
    asrcs: Vec<String>,
    /// Audio codec ID → name.
    acodecs: BTreeMap<String, String>,
    netif: BTreeMap<String, Netif>,
    /// Modem ID → modem.
    modems: BTreeMap<String, Modem>,
    /// WiFi adapter ID → adapter.
    wifi: BTreeMap<String, WifiAdapter>,
    /// How the last WiFi request ended.
    result: Option<BelaboxResult>,
    sensors: Vec<BelaboxSensor>,
    notices: Vec<Notice>,
}

impl State {
    /// A socket to BELABOX Cloud is open; authentication comes next.
    pub fn connected(&mut self) {
        self.connected = true;
        self.authenticated = false;
    }

    /// The socket closed: the live readings are gone until we're back.
    pub fn disconnected(&mut self) {
        self.connected = false;
        self.authenticated = false;
        self.netif.clear();
        self.sensors.clear();
    }

    pub fn invalid_key(&self) -> bool {
        self.invalid_key
    }

    pub fn streaming(&self) -> bool {
        self.link() == BelaboxLink::Online && self.streaming == Some(true)
    }

    pub fn link(&self) -> BelaboxLink {
        if self.invalid_key {
            BelaboxLink::InvalidKey
        } else if !self.connected || !self.authenticated {
            BelaboxLink::Connecting
        } else {
            match self.online {
                Some(true) => BelaboxLink::Online,
                Some(false) => BelaboxLink::EncoderOffline,
                None => BelaboxLink::Connecting,
            }
        }
    }

    /// Folds one frame from BELABOX Cloud in.
    pub fn apply(&mut self, frame: &str, now: Instant) -> Vec<Change> {
        let mut changes = Vec::new();
        let Ok(Value::Object(frame)) = serde_json::from_str::<Value>(frame) else {
            return changes;
        };

        for (kind, value) in frame {
            match kind.as_str() {
                "remote" => self.on_remote(&value, &mut changes),
                "config" => {
                    if let Value::Object(config) = value {
                        self.max_br = config.get("max_br").and_then(as_u32);
                        self.config = Some(config);
                    }
                }
                "pipelines" => self.on_pipelines(&value),
                "relays" => self.on_relays(&value),
                "acodecs" => self.acodecs = acodecs(&value),
                "wifi" => self.on_wifi_result(&value),
                "status" => self.on_status(&value, &mut changes),
                "netif" => self.on_netif(&value, &mut changes),
                "sensors" => {
                    if let Value::Object(sensors) = value {
                        self.sensors = sensors
                            .into_iter()
                            .map(|(name, value)| BelaboxSensor {
                                name,
                                value: match value {
                                    Value::String(value) => value,
                                    other => other.to_string(),
                                },
                            })
                            .collect();
                    }
                }
                "bitrate" => {
                    if let Some(max_br) = value.get("max_br").and_then(as_u32) {
                        self.max_br = Some(max_br);
                    }
                }
                "notification" => self.on_notification(&value, now, &mut changes),
                // `id` (who a reply is for), and messages we don't use.
                _ => {}
            }
        }

        changes
    }

    fn on_remote(&mut self, value: &Value, changes: &mut Vec<Change>) {
        if let Some(accepted) = value.get("auth/key").and_then(Value::as_bool) {
            self.authenticated = accepted;
            self.invalid_key = !accepted;
            changes.push(Change::Authenticated(accepted));
        }
        if let Some(online) = value.get("is_encoder_online").and_then(Value::as_bool) {
            if self.online == Some(!online) {
                changes.push(Change::EncoderOnline(online));
            }
            self.online = Some(online);
            if !online {
                self.streaming = None;
                self.netif.clear();
                self.modems.clear();
                self.wifi.clear();
                self.result = None;
                self.sensors.clear();
                self.notices.clear();
            }
        }
    }

    fn on_status(&mut self, value: &Value, changes: &mut Vec<Change>) {
        if let Some(streaming) = value.get("is_streaming").and_then(Value::as_bool) {
            if self.streaming == Some(!streaming) {
                changes.push(Change::Streaming(streaming));
            }
            self.streaming = Some(streaming);
            // It started with that bitrate (or with its own, if started on the BELABOX).
            if streaming {
                if let Some(max_br) = self.next_max_br.take() {
                    self.max_br = Some(max_br);
                }
                // Saved by the start; the next `config` has them.
                self.pending.clear();
            }
        }

        if let Some(Value::Array(sources)) = value.get("asrcs") {
            self.asrcs = sources
                .iter()
                .filter_map(|source| source.as_str().map(str::to_owned))
                .collect();
        }
        if let Some(Value::Object(adapters)) = value.get("wifi") {
            self.wifi = adapters
                .iter()
                .map(|(id, adapter)| (id.clone(), wifi_adapter(adapter)))
                .collect();
        }

        // Every message lists all modems; partial ones only carry `status`.
        if let Some(Value::Object(modems)) = value.get("modems") {
            self.modems.retain(|id, _| modems.contains_key(id));
            for (id, update) in modems {
                let modem = self.modems.entry(id.clone()).or_default();
                if let Some(ifname) = update.get("ifname").and_then(Value::as_str) {
                    modem.ifname = Some(ifname.to_owned());
                }
                // The settings come with the full description of a modem, not with
                // the frequent status-only updates.
                if let Some(name) = update.get("name").and_then(Value::as_str) {
                    modem.name = Some(name.to_owned());
                }
                if let Some(types) = update.get("network_type") {
                    modem.network_types = strings(&types["supported"]);
                    modem.network_type = types["active"].as_str().map(str::to_owned);
                }
                if let Some(config) = update.get("config").filter(|c| c.is_object()) {
                    modem.config = Some(ModemConfig {
                        autoconfig: config["autoconfig"].as_bool(),
                        apn: config["apn"].as_str().unwrap_or_default().to_owned(),
                        username: config["username"].as_str().unwrap_or_default().to_owned(),
                        password: config["password"].as_str().unwrap_or_default().to_owned(),
                        roaming: config["roaming"].as_bool().unwrap_or(false),
                        network: config["network"].as_str().unwrap_or_default().to_owned(),
                    });
                } else if update.get("no_sim").and_then(Value::as_bool) == Some(true) {
                    modem.config = None;
                }
                if let Some(Value::Object(networks)) = update.get("available_networks") {
                    modem.networks = networks
                        .iter()
                        .map(|(id, network)| {
                            (
                                id.clone(),
                                (
                                    network["name"].as_str().unwrap_or(id).to_owned(),
                                    network["availability"].as_str().map(str::to_owned),
                                ),
                            )
                        })
                        .collect();
                }
                if let Some(status) = update.get("status").filter(|s| s.is_object()) {
                    modem.status = Some(BelaboxModem {
                        network: status["network"].as_str().map(str::to_owned),
                        network_type: status["network_type"].as_str().map(str::to_owned),
                        signal: as_u32(&status["signal"]),
                        roaming: status["roaming"].as_bool(),
                        connection: status["connection"].as_str().map(str::to_owned),
                    });
                }
            }
        }
    }

    fn on_netif(&mut self, value: &Value, changes: &mut Vec<Change>) {
        let Value::Object(interfaces) = value else {
            return;
        };
        let netif: BTreeMap<String, Netif> = interfaces
            .iter()
            .map(|(name, netif)| {
                let netif = Netif {
                    ip: netif["ip"].as_str().map(str::to_owned),
                    tp: netif["tp"].as_f64().map_or(0, |tp| tp.max(0.0) as u64),
                    enabled: netif["enabled"].as_bool().unwrap_or(false),
                    error: netif["error"].as_str().map(str::to_owned),
                };
                (name.clone(), netif)
            })
            .collect();

        for (name, new) in &netif {
            if let Some(old) = self.netif.get(name)
                && old.enabled != new.enabled
            {
                changes.push(Change::InterfaceToggled {
                    name: name.clone(),
                    enabled: new.enabled,
                });
            }
        }
        self.netif = netif;
    }

    fn on_notification(&mut self, value: &Value, now: Instant, changes: &mut Vec<Change>) {
        if let Some(Value::Array(shown)) = value.get("show") {
            for shown in shown {
                let (Some(name), Some(message)) = (shown["name"].as_str(), shown["msg"].as_str())
                else {
                    continue;
                };
                let kind = match shown["type"].as_str() {
                    Some("success") => BelaboxNoticeKind::Success,
                    Some("error") => BelaboxNoticeKind::Error,
                    _ => BelaboxNoticeKind::Warning,
                };
                let expires = shown["duration"]
                    .as_f64()
                    .filter(|seconds| *seconds > 0.0)
                    .map(|seconds| now + Duration::from_secs_f64(seconds));

                // Persistent ones are sent again and again; only a new one counts.
                let known = self.notices.iter().position(|n| n.notice.name == name);
                let is_new = known.is_none_or(|i| self.notices[i].notice.message != message);
                if is_new && kind == BelaboxNoticeKind::Error {
                    changes.push(Change::Error(message.to_owned()));
                }
                if let Some(i) = known {
                    self.notices.remove(i);
                }
                self.notices.push(Notice {
                    notice: BelaboxNotice {
                        name: name.to_owned(),
                        kind,
                        message: message.to_owned(),
                    },
                    expires,
                });
            }
        }
        if let Some(Value::Array(removed)) = value.get("remove") {
            self.notices.retain(|n| {
                !removed
                    .iter()
                    .any(|name| name.as_str() == Some(&n.notice.name))
            });
        }
    }

    /// What tells an interface's device apart, so a name the streamer gave it
    /// survives the interface being renamed (`usb0` → `usb1`) as far as BELABOX
    /// lets us tell devices apart. belaUI names a modem "model - last 5 digits of
    /// its IMEI | operator", so the part before the operator is per device (the
    /// operator changes with the SIM, the modem ID and `ifname` with the USB
    /// port). For WiFi it only sends the adapter's vendor and product (it keeps the
    /// MAC to itself), so two identical adapters share an identity. Anything else
    /// (ethernet) has nothing but its name.
    fn interface_identity(&self, ifname: &str) -> String {
        let modem = self
            .modems
            .values()
            .find(|modem| modem.ifname.as_deref() == Some(ifname))
            .and_then(|modem| modem.name.as_deref())
            .and_then(|name| name.split(" | ").next())
            .map(str::trim);
        let wifi = self
            .wifi
            .values()
            .find(|adapter| adapter.interface == ifname)
            .and_then(|adapter| adapter.hardware.as_deref())
            .map(str::trim);
        match modem.or(wifi) {
            Some(key) if !key.is_empty() => key.to_owned(),
            _ => ifname.to_owned(),
        }
    }

    /// The status for the dashboard (its `updated_at` is set when published).
    pub fn status(&mut self, now: Instant) -> BelaboxStatus {
        self.notices
            .retain(|n| n.expires.is_none_or(|expires| expires > now));
        let link = self.link();
        let online = link == BelaboxLink::Online;
        let config = self.effective_config();
        let config = config.as_ref();

        let mut interfaces: Vec<BelaboxInterface> = if online {
            self.netif
                .iter()
                .map(|(name, netif)| BelaboxInterface {
                    name: name.clone(),
                    key: opaque_key(&self.interface_identity(name)),
                    kbps: kbps(netif.tp),
                    enabled: netif.enabled,
                    error: netif.error.clone(),
                    modem: self
                        .modems
                        .values()
                        .find(|modem| modem.ifname.as_deref() == Some(name))
                        .and_then(|modem| modem.status.clone()),
                })
                .collect()
        } else {
            Vec::new()
        };
        // Identical adapters can't be told apart: they keep their interface names.
        let keys: Vec<String> = interfaces.iter().map(|i| i.key.clone()).collect();
        for interface in &mut interfaces {
            if keys.iter().filter(|key| **key == interface.key).count() > 1 {
                interface.key = opaque_key(&interface.name);
            }
        }

        BelaboxStatus {
            updated_at: Utc::now(),
            link,
            streaming: self.streaming(),
            max_br: self.next_max_br.or(self.max_br),
            total_kbps: interfaces.iter().map(|i| i.kbps).sum(),
            interfaces,
            sensors: if online {
                self.sensors.clone()
            } else {
                Vec::new()
            },
            notifications: self.notices.iter().map(|n| n.notice.clone()).collect(),
            pipeline: config
                .and_then(|c| c.get("pipeline"))
                .and_then(Value::as_str)
                .map(|id| {
                    self.pipelines
                        .get(id)
                        .map_or_else(|| id.to_owned(), |pipeline| pipeline.name.clone())
                }),
            srt_latency: config.and_then(|c| c.get("srt_latency")).and_then(as_u32),
            relay: config
                .and_then(|c| c.get("relay_server"))
                .and_then(Value::as_str)
                .and_then(|id| self.relay_servers.get(id).map(|(name, _)| name.clone())),
        }
    }

    /// Ok when the BELABOX can be given orders.
    fn ready(&self) -> Result<(), BelaboxRefusal> {
        match self.link() {
            BelaboxLink::Online => Ok(()),
            BelaboxLink::Connecting => Err(BelaboxRefusal::Connecting),
            BelaboxLink::EncoderOffline => Err(BelaboxRefusal::EncoderOffline),
            BelaboxLink::InvalidKey => Err(BelaboxRefusal::InvalidKey),
        }
    }

    /// What to do for `action`: the frame to send, or `None` when it only changed
    /// what our next `start` carries (stream settings), or why it can't be done now.
    pub fn prepare(&mut self, action: &BelaboxAction) -> Result<Option<String>, BelaboxRefusal> {
        match action {
            BelaboxAction::Settings { settings } => self.set_settings(settings).map(|()| None),
            other => self.frame(other).map(Some),
        }
    }

    /// The frame that carries out `action`.
    fn frame(&self, action: &BelaboxAction) -> Result<String, BelaboxRefusal> {
        self.ready()?;
        let streaming = self.streaming == Some(true);

        let frame = match action {
            BelaboxAction::Start => {
                if streaming {
                    return Err(BelaboxRefusal::AlreadyStreaming);
                }
                json!({ "start": self.start_settings()? })
            }
            BelaboxAction::Stop => {
                if !streaming {
                    return Err(BelaboxRefusal::NotStreaming);
                }
                json!({ "stop": 0 })
            }
            BelaboxAction::Bitrate { max_br } => {
                if !(live::BELABOX_MIN_BITRATE..=live::BELABOX_MAX_BITRATE).contains(max_br) {
                    return Err(BelaboxRefusal::InvalidBitrate);
                }
                // Always sent: we may not know it's streaming (no `status` since we
                // connected). If it isn't, BELABOX ignores it and our next start
                // carries it (see `bitrate_sent`).
                json!({ "bitrate": { "max_br": max_br } })
            }
            BelaboxAction::Interface { name, enabled } => {
                let netif = self
                    .netif
                    .get(name)
                    .ok_or(BelaboxRefusal::UnknownInterface)?;
                let others_enabled = self
                    .netif
                    .iter()
                    .any(|(other, netif)| other != name && netif.enabled);
                if !enabled && netif.enabled && !others_enabled {
                    return Err(BelaboxRefusal::LastInterface);
                }
                // BELABOX only accepts the request with the interface's current IP.
                let ip = netif
                    .ip
                    .as_deref()
                    .ok_or(BelaboxRefusal::UnknownInterface)?;
                json!({ "netif": { "name": name, "ip": ip, "enabled": enabled } })
            }
            BelaboxAction::WifiScan => json!({ "wifi": { "scan": true } }),
            BelaboxAction::WifiConnect { uuid } => {
                if !self.wifi_has_saved(uuid) {
                    return Err(BelaboxRefusal::UnknownWifi);
                }
                json!({ "wifi": { "connect": uuid } })
            }
            BelaboxAction::WifiDisconnect { uuid } => {
                if !self
                    .wifi
                    .values()
                    .any(|adapter| adapter.connection.as_deref() == Some(uuid))
                {
                    return Err(BelaboxRefusal::UnknownWifi);
                }
                json!({ "wifi": { "disconnect": uuid } })
            }
            BelaboxAction::WifiForget { uuid } => {
                if !self.wifi_has_saved(uuid) {
                    return Err(BelaboxRefusal::UnknownWifi);
                }
                json!({ "wifi": { "forget": uuid } })
            }
            BelaboxAction::WifiNew {
                device,
                ssid,
                password,
            } => {
                let adapter = self.wifi.get(device).ok_or(BelaboxRefusal::UnknownWifi)?;
                if adapter.hotspot.is_some() {
                    return Err(BelaboxRefusal::UnknownWifi);
                }
                if ssid.is_empty() || ssid.len() > 32 {
                    return Err(BelaboxRefusal::Invalid(BelaboxField::WifiName));
                }
                // A WPA passphrase; open networks have none.
                if let Some(password) = password
                    && !(8..=63).contains(&password.chars().count())
                {
                    return Err(BelaboxRefusal::Invalid(BelaboxField::WifiPassword));
                }
                let mut new = json!({ "device": device, "ssid": ssid });
                if let Some(password) = password {
                    new["password"] = password.as_str().into();
                }
                json!({ "wifi": { "new": new } })
            }
            BelaboxAction::Hotspot { device, active } => {
                let adapter = self.wifi.get(device).ok_or(BelaboxRefusal::UnknownWifi)?;
                if !adapter.supports_hotspot && adapter.hotspot.is_none() {
                    return Err(BelaboxRefusal::UnknownWifi);
                }
                let order = if *active { "start" } else { "stop" };
                json!({ "wifi": { "hotspot": { order: { "device": device } } } })
            }
            BelaboxAction::HotspotConfig {
                device,
                name,
                password,
                channel,
            } => {
                let hotspot = self
                    .wifi
                    .get(device)
                    .and_then(|adapter| adapter.hotspot.as_ref())
                    .ok_or(BelaboxRefusal::UnknownWifi)?;
                if name.is_empty() || name.len() > 32 {
                    return Err(BelaboxRefusal::Invalid(BelaboxField::HotspotName));
                }
                let password = match password {
                    Some(password) => password.as_str(),
                    None => hotspot.password.as_deref().unwrap_or_default(),
                };
                if !(8..=64).contains(&password.chars().count()) {
                    return Err(BelaboxRefusal::Invalid(BelaboxField::HotspotPassword));
                }
                if !hotspot.channels.iter().any(|choice| &choice.id == channel) {
                    return Err(BelaboxRefusal::Invalid(BelaboxField::HotspotChannel));
                }
                json!({ "wifi": { "hotspot": { "config": {
                    "device": device, "name": name, "password": password, "channel": channel,
                } } } })
            }
            BelaboxAction::ModemConfig {
                modem,
                autoconfig,
                apn,
                username,
                password,
                roaming,
                network,
                network_type,
            } => {
                let known = self.modems.get(modem).ok_or(BelaboxRefusal::UnknownModem)?;
                let config = known.config.as_ref().ok_or(BelaboxRefusal::UnknownModem)?;
                if !known.network_types.contains(network_type) {
                    return Err(BelaboxRefusal::Invalid(BelaboxField::NetworkType));
                }
                // Automatic, the one already saved, or one the last scan found.
                if !network.is_empty()
                    && *network != config.network
                    && !known.networks.contains_key(network)
                {
                    return Err(BelaboxRefusal::Invalid(BelaboxField::Network));
                }
                if !autoconfig && apn.trim().is_empty() {
                    return Err(BelaboxRefusal::Invalid(BelaboxField::Apn));
                }
                let password = password.as_deref().unwrap_or(&config.password);
                json!({ "modems": { "config": {
                    "device": modem, "autoconfig": autoconfig, "apn": apn, "username": username,
                    "password": password, "roaming": roaming, "network": network,
                    "network_type": network_type,
                } } })
            }
            BelaboxAction::ModemScan { modem } => {
                if self
                    .modems
                    .get(modem)
                    .is_none_or(|modem| modem.config.is_none())
                {
                    return Err(BelaboxRefusal::UnknownModem);
                }
                json!({ "modems": { "scan": { "device": modem } } })
            }
            // `prepare` takes these.
            BelaboxAction::Settings { .. } => return Err(BelaboxRefusal::NoConfig),
        };

        Ok(frame.to_string())
    }

    fn wifi_has_saved(&self, uuid: &str) -> bool {
        self.wifi
            .values()
            .any(|adapter| adapter.saved.values().any(|saved| saved == uuid))
    }

    /// The BELABOX's saved config with our unsent changes on top.
    fn effective_config(&self) -> Option<Map<String, Value>> {
        let mut config = self.config.clone()?;
        for (key, value) in &self.pending {
            config.insert(key.clone(), value.clone());
        }
        Some(config)
    }

    /// Checks stream settings like belaUI's `validateConfig` would and keeps them for
    /// our next `start`: BELABOX only saves them when it starts.
    fn set_settings(&mut self, update: &BelaboxSettingsUpdate) -> Result<(), BelaboxRefusal> {
        self.ready()?;
        if self.streaming == Some(true) {
            return Err(BelaboxRefusal::Streaming);
        }
        let current = self.effective_config().ok_or(BelaboxRefusal::NoConfig)?;
        let invalid = |field| Err(BelaboxRefusal::Invalid(field));

        let Some(pipeline) = self.pipelines.get(&update.pipeline) else {
            return invalid(BelaboxField::Pipeline);
        };
        if !(-2000..=2000).contains(&update.audio_delay) {
            return invalid(BelaboxField::AudioDelay);
        }
        if !(100..=10_000).contains(&update.srt_latency) {
            return invalid(BelaboxField::SrtLatency);
        }

        let mut pending = Map::new();
        pending.insert("pipeline".into(), update.pipeline.clone().into());
        pending.insert("delay".into(), update.audio_delay.into());
        pending.insert("srt_latency".into(), update.srt_latency.into());
        pending.insert("bitrate_overlay".into(), update.bitrate_overlay.into());

        if pipeline.acodec {
            let Some(codec) = update.audio_codec.as_deref() else {
                return invalid(BelaboxField::AudioCodec);
            };
            if !self.acodecs.contains_key(codec) {
                return invalid(BelaboxField::AudioCodec);
            }
            pending.insert("acodec".into(), codec.into());
        }
        if pipeline.asrc {
            let Some(source) = update.audio_source.as_deref() else {
                return invalid(BelaboxField::AudioSource);
            };
            // The saved one may be an input that isn't plugged in right now.
            let saved = current.get("asrc").and_then(Value::as_str) == Some(source);
            if !saved && !self.asrcs.iter().any(|known| known == source) {
                return invalid(BelaboxField::AudioSource);
            }
            pending.insert("asrc".into(), source.into());
        }

        match (&update.relay_server, &update.srtla_address) {
            (Some(server), _) => {
                if !self.relay_servers.contains_key(server) {
                    return invalid(BelaboxField::Relay);
                }
                pending.insert("relay_server".into(), server.clone().into());
                pending.insert("srtla_addr".into(), Value::Null);
                pending.insert("srtla_port".into(), Value::Null);
            }
            (None, Some(address)) => {
                let address = address.trim();
                if address.is_empty() || address.chars().any(char::is_whitespace) {
                    return invalid(BelaboxField::Address);
                }
                let port = update.srtla_port.filter(|port| *port != 0);
                let Some(port) = port else {
                    return invalid(BelaboxField::Port);
                };
                pending.insert("relay_server".into(), Value::Null);
                pending.insert("srtla_addr".into(), address.into());
                pending.insert("srtla_port".into(), port.into());
            }
            (None, None) => return invalid(BelaboxField::Address),
        }

        match &update.relay_account {
            Some(account) => {
                if update.relay_server.is_none() || !self.relay_accounts.contains_key(account) {
                    return invalid(BelaboxField::RelayAccount);
                }
                pending.insert("relay_account".into(), account.clone().into());
                pending.insert("srt_streamid".into(), Value::Null);
            }
            None => {
                pending.insert("relay_account".into(), Value::Null);
                match update.stream_id.as_deref() {
                    Some(key) if !key.is_empty() => {
                        pending.insert("srt_streamid".into(), key.into());
                    }
                    Some(_) => return invalid(BelaboxField::StreamId),
                    // Keep the BELABOX's, if it has one.
                    None => {
                        let has = current
                            .get("srt_streamid")
                            .and_then(Value::as_str)
                            .is_some_and(|key| !key.is_empty());
                        if !has {
                            return invalid(BelaboxField::StreamId);
                        }
                    }
                }
            }
        }

        self.pending.extend(pending);
        Ok(())
    }

    /// How the last WiFi request ended (`wifi.new`, `wifi.connect`, `wifi.hotspot.config`).
    fn on_wifi_result(&mut self, value: &Value) {
        let device = |value: &Value| id_string(&value["device"]);
        let mut results = Vec::new();

        if let Some(new) = value.get("new").filter(|n| n.is_object()) {
            results.push((
                BelaboxResultKind::WifiNew,
                device(new),
                if new["success"].as_bool() == Some(true) {
                    None
                } else {
                    Some(new["error"].as_str().unwrap_or("generic").to_owned())
                },
            ));
        }
        // `{connect: true|false, device}`
        if let Some(success) = value.get("connect").and_then(Value::as_bool) {
            results.push((
                BelaboxResultKind::WifiConnect,
                device(value),
                (!success).then(|| "generic".to_owned()),
            ));
        }
        if let Some(config) = value["hotspot"]
            .get("config")
            .filter(|config| config.is_object())
        {
            results.push((
                BelaboxResultKind::HotspotConfig,
                device(config),
                if config["success"].as_bool() == Some(true) {
                    None
                } else {
                    Some(config["error"].as_str().unwrap_or("saving").to_owned())
                },
            ));
        }

        for (kind, device, error) in results {
            let id = self.result.as_ref().map_or(1, |result| result.id + 1);
            self.result = Some(BelaboxResult {
                id,
                kind,
                device: device.unwrap_or_default(),
                error,
            });
        }
    }

    fn on_pipelines(&mut self, value: &Value) {
        let Value::Object(pipelines) = value else {
            return;
        };
        self.pipelines = pipelines
            .iter()
            .filter_map(|(id, pipeline)| {
                Some((
                    id.clone(),
                    PipelineInfo {
                        name: pipeline["name"].as_str()?.to_owned(),
                        asrc: pipeline["asrc"].as_bool().unwrap_or(false),
                        acodec: pipeline["acodec"].as_bool().unwrap_or(false),
                    },
                ))
            })
            .collect();
    }

    fn on_relays(&mut self, value: &Value) {
        self.relay_servers = entries(&value["servers"], "default");
        self.relay_accounts = entries(&value["accounts"], "disabled");
    }

    /// What the settings dialog needs.
    pub fn details(&self) -> Result<BelaboxDetails, BelaboxRefusal> {
        self.ready()?;
        let config = self.effective_config();
        let text = |key: &str| {
            config
                .as_ref()
                .and_then(|config| config.get(key))
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let number = |key: &str| {
            config
                .as_ref()
                .and_then(|config| config.get(key))
                .and_then(Value::as_i64)
        };

        let mut wifi: Vec<BelaboxWifiDevice> = self
            .wifi
            .iter()
            .map(|(id, adapter)| adapter.to_device(id))
            .collect();
        wifi.sort_by_key(|device| (device.id.len(), device.id.clone()));

        let mut modems: Vec<BelaboxModemDetails> = self
            .modems
            .iter()
            .map(|(id, modem)| BelaboxModemDetails {
                id: id.clone(),
                interface: modem.ifname.clone(),
                name: modem.name.clone().unwrap_or_else(|| format!("Modem {id}")),
                no_sim: modem.config.is_none(),
                network_types: modem.network_types.clone(),
                network_type: modem.network_type.clone(),
                config: modem.config.as_ref().map(|config| BelaboxModemConfig {
                    autoconfig: config.autoconfig,
                    apn: config.apn.clone(),
                    username: config.username.clone(),
                    has_password: !config.password.is_empty(),
                    roaming: config.roaming,
                    network: config.network.clone(),
                }),
                networks: modem
                    .networks
                    .iter()
                    .map(|(id, (name, availability))| BelaboxModemNetwork {
                        id: id.clone(),
                        name: name.clone(),
                        availability: availability.clone(),
                    })
                    .collect(),
                status: modem.status.clone(),
            })
            .collect();
        modems.sort_by_key(|modem| (modem.id.len(), modem.id.clone()));

        Ok(BelaboxDetails {
            streaming: self.streaming == Some(true),
            wifi,
            modems,
            audio_sources: self.asrcs.clone(),
            audio_codecs: self
                .acodecs
                .iter()
                .map(|(id, name)| BelaboxChoice {
                    id: id.clone(),
                    name: name.clone(),
                })
                .collect(),
            pipelines: self
                .pipelines
                .iter()
                .map(|(id, pipeline)| BelaboxPipeline {
                    id: id.clone(),
                    name: pipeline.name.clone(),
                    audio_source: pipeline.asrc,
                    audio_codec: pipeline.acodec,
                })
                .collect(),
            relay_servers: self
                .relay_servers
                .iter()
                .map(|(id, (name, default))| BelaboxRelayServer {
                    id: id.clone(),
                    name: name.clone(),
                    default: *default,
                })
                .collect(),
            relay_accounts: self
                .relay_accounts
                .iter()
                .map(|(id, (name, disabled))| BelaboxRelayAccount {
                    id: id.clone(),
                    name: name.clone(),
                    disabled: *disabled,
                })
                .collect(),
            settings: BelaboxStreamSettings {
                pipeline: text("pipeline"),
                audio_source: text("asrc"),
                audio_codec: text("acodec"),
                audio_delay: number("delay").and_then(|n| i32::try_from(n).ok()),
                srt_latency: number("srt_latency").and_then(|n| u32::try_from(n).ok()),
                bitrate_overlay: config
                    .as_ref()
                    .and_then(|config| config.get("bitrate_overlay"))
                    .and_then(Value::as_bool),
                relay_server: text("relay_server"),
                relay_account: text("relay_account"),
                srtla_address: text("srtla_addr"),
                srtla_port: number("srtla_port").and_then(|n| u16::try_from(n).ok()),
                has_stream_id: text("srt_streamid").is_some(),
            },
            settings_pending: !self.pending.is_empty(),
            result: self.result.clone(),
        })
    }

    /// Remembers a bitrate we set: BELABOX doesn't echo it to the sender. Unless
    /// we know it's streaming (then it applied it), it's also kept for our next
    /// `start`.
    pub fn bitrate_sent(&mut self, max_br: u32) {
        if self.streaming == Some(true) {
            self.max_br = Some(max_br);
        } else {
            self.next_max_br = Some(max_br);
        }
    }

    /// The saved settings, as `start` wants them.
    fn start_settings(&self) -> Result<Map<String, Value>, BelaboxRefusal> {
        let config = self.effective_config().ok_or(BelaboxRefusal::NoConfig)?;
        let mut start: Map<String, Value> = START_FIELDS
            .iter()
            .filter_map(|&field| Some((field.to_owned(), config.get(field)?.clone())))
            .filter(|(_, value)| !value.is_null())
            .collect();

        if let Some(max_br) = self.next_max_br.or(self.max_br) {
            start.insert("max_br".into(), max_br.into());
        }
        let has = |field: &str| start.get(field).is_some();
        if !has("pipeline") || !has("max_br") || !has("srt_latency") {
            return Err(BelaboxRefusal::NoConfig);
        }
        start.entry("delay").or_insert(0.into());
        start.entry("bitrate_overlay").or_insert(false.into());

        Ok(start)
    }
}

/// `{id: {name, <flag>}}` → id → (name, flag).
fn entries(value: &Value, flag: &str) -> BTreeMap<String, (String, bool)> {
    let Value::Object(entries) = value else {
        return BTreeMap::new();
    };
    entries
        .iter()
        .filter_map(|(id, entry)| {
            Some((
                id.clone(),
                (
                    entry["name"].as_str()?.to_owned(),
                    entry[flag].as_bool().unwrap_or(false),
                ),
            ))
        })
        .collect()
}

/// `["a", "b"]` → strings; anything else is empty.
fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// BELABOX's audio codecs: `{id: "display name"}`.
fn acodecs(value: &Value) -> BTreeMap<String, String> {
    let Value::Object(codecs) = value else {
        return BTreeMap::new();
    };
    codecs
        .iter()
        .filter_map(|(id, name)| Some((id.clone(), name.as_str()?.to_owned())))
        .collect()
}

/// An ID BELABOX may send as a number or a string (WiFi adapters count from 0).
fn id_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// One entry of `status.wifi`.
fn wifi_adapter(value: &Value) -> WifiAdapter {
    let hotspot = value
        .get("hotspot")
        .filter(|h| h.is_object())
        .map(|h| Hotspot {
            name: h["name"].as_str().unwrap_or_default().to_owned(),
            password: h["password"].as_str().map(str::to_owned),
            channel: h["channel"].as_str().unwrap_or("auto").to_owned(),
            channels: h["available_channels"]
                .as_object()
                .map(|channels| {
                    channels
                        .iter()
                        .map(|(id, channel)| BelaboxChoice {
                            id: id.clone(),
                            name: channel["name"].as_str().unwrap_or(id).to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            warnings: strings(&h["warnings"]),
        });

    WifiAdapter {
        interface: value["ifname"].as_str().unwrap_or_default().to_owned(),
        hardware: value["hw"].as_str().map(str::trim).map(str::to_owned),
        connection: value["conn"].as_str().map(str::to_owned),
        available: value["available"]
            .as_array()
            .map(|networks| {
                networks
                    .iter()
                    .filter_map(|network| {
                        Some(Scan {
                            ssid: network["ssid"].as_str()?.to_owned(),
                            active: network["active"].as_bool().unwrap_or(false),
                            signal: as_u32(&network["signal"]),
                            security: network["security"].as_str().map(str::to_owned),
                            frequency: as_u32(&network["freq"]),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        saved: value["saved"]
            .as_object()
            .map(|saved| {
                saved
                    .iter()
                    .filter_map(|(ssid, uuid)| Some((ssid.clone(), uuid.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default(),
        supports_hotspot: value["supports_hotspot"].as_bool().unwrap_or(false),
        hotspot,
    }
}

impl WifiAdapter {
    /// Saved networks and the ones in range as one list: connected first, then by signal.
    fn to_device(&self, id: &str) -> BelaboxWifiDevice {
        let mut networks: Vec<BelaboxWifiNetwork> = self
            .available
            .iter()
            .map(|scan| BelaboxWifiNetwork {
                ssid: scan.ssid.clone(),
                available: true,
                connected: scan.active,
                saved: self.saved.get(&scan.ssid).cloned(),
                signal: scan.signal,
                security: scan.security.clone().filter(|s| !s.is_empty() && s != "--"),
                frequency: scan.frequency,
            })
            .collect();
        for (ssid, uuid) in &self.saved {
            if !networks.iter().any(|network| &network.ssid == ssid) {
                networks.push(BelaboxWifiNetwork {
                    ssid: ssid.clone(),
                    available: false,
                    connected: false,
                    saved: Some(uuid.clone()),
                    signal: None,
                    security: None,
                    frequency: None,
                });
            }
        }
        networks.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then(b.available.cmp(&a.available))
                .then(b.signal.cmp(&a.signal))
                .then(a.ssid.cmp(&b.ssid))
        });

        BelaboxWifiDevice {
            id: id.to_owned(),
            interface: self.interface.clone(),
            hardware: self.hardware.clone(),
            networks: if self.hotspot.is_some() {
                Vec::new()
            } else {
                networks
            },
            hotspot: self.hotspot.as_ref().map(|hotspot| BelaboxHotspot {
                name: hotspot.name.clone(),
                has_password: hotspot.password.as_deref().is_some_and(|p| !p.is_empty()),
                channel: hotspot.channel.clone(),
                channels: hotspot.channels.clone(),
                warnings: hotspot.warnings.clone(),
            }),
            supports_hotspot: self.supports_hotspot || self.hotspot.is_some(),
        }
    }
}

/// A number BELABOX may send as a JSON number or a numeric string.
fn as_u32(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_f64().map(|f| f.max(0.0) as u64))
            .and_then(|n| u32::try_from(n).ok()),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// What a name is saved under, from a device's identity: opaque, since the
/// status goes to everyone who may view the switcher and a modem's identity
/// holds the last digits of its IMEI.
fn opaque_key(identity: &str) -> String {
    common::tokens::hash(identity)
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Bytes per second → kbps, as belaUI shows it.
fn kbps(tp: u64) -> u32 {
    u32::try_from(tp * 8 / 1024).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{"config":{"remote_key":"secret","ssh_pass":"hunter2","max_br":6000,"delay":0,
        "pipeline":"abc","srt_latency":2000,"bitrate_overlay":false,"asrc":"HDMI","acodec":"opus",
        "relay_server":"3","relay_account":"7"}}"#;
    const PIPELINES: &str =
        r#"{"pipelines":{"abc":{"name":"h265_hdmi","asrc":true,"acodec":true}}}"#;
    const RELAYS: &str = r#"{"relays":{"servers":{"3":{"name":"EU (42 ms)"}},"accounts":{}}}"#;
    const NETIF: &str = r#"{"netif":{"usb0":{"ip":"10.0.0.2","tp":256000,"enabled":true},
        "wlan0":{"ip":"192.168.1.10","tp":0,"enabled":false,"error":"WiFi hotspot"}}}"#;

    fn online() -> State {
        let now = Instant::now();
        let mut state = State::default();
        state.connected();
        state.apply(r#"{"remote":{"auth/key":true}}"#, now);
        state.apply(r#"{"remote":{"is_encoder_online":true,"version":16}}"#, now);
        for frame in [CONFIG, PIPELINES, RELAYS, NETIF] {
            state.apply(frame, now);
        }
        state.apply(r#"{"status":{"is_streaming":false,"asrcs":[]}}"#, now);
        state
    }

    #[test]
    fn a_refused_key_is_final() {
        let mut state = State::default();
        state.connected();
        let changes = state.apply(r#"{"remote":{"auth/key":false}}"#, Instant::now());
        assert_eq!(changes, [Change::Authenticated(false)]);
        assert_eq!(state.link(), BelaboxLink::InvalidKey);
        assert_eq!(
            state.frame(&BelaboxAction::Start),
            Err(BelaboxRefusal::InvalidKey)
        );
    }

    #[test]
    fn link_follows_the_cloud_and_the_encoder() {
        let now = Instant::now();
        let mut state = State::default();
        assert_eq!(state.link(), BelaboxLink::Connecting);
        state.connected();
        state.apply(r#"{"remote":{"auth/key":true}}"#, now);
        assert_eq!(state.link(), BelaboxLink::Connecting);

        // The first reading isn't a change.
        let changes = state.apply(r#"{"remote":{"is_encoder_online":false}}"#, now);
        assert_eq!(changes, []);
        assert_eq!(state.link(), BelaboxLink::EncoderOffline);

        let changes = state.apply(r#"{"remote":{"is_encoder_online":true}}"#, now);
        assert_eq!(changes, [Change::EncoderOnline(true)]);
        assert_eq!(state.link(), BelaboxLink::Online);

        state.disconnected();
        assert_eq!(state.link(), BelaboxLink::Connecting);
    }

    #[test]
    fn status_shows_interfaces_modems_and_settings_without_ips() {
        let now = Instant::now();
        let mut state = online();
        state.apply(
            r#"{"status":{"modems":{"1":{"ifname":"usb0","name":"EM7455 - 12345 | Vodafone",
                "config":{"apn":"internet","password":"secret"},
                "status":{"connection":"connected","network":"Vodafone","network_type":"4G","signal":72,"roaming":false}}}}}"#,
            now,
        );
        state.apply(
            r#"{"sensors":{"SoC temperature":"45.3 °C","SRT ingest":"1234 Kbps, 20 ms RTT"}}"#,
            now,
        );

        let status = state.status(now);
        assert_eq!(status.link, BelaboxLink::Online);
        assert!(!status.streaming);
        assert_eq!(status.max_br, Some(6000));
        assert_eq!(status.pipeline.as_deref(), Some("h265_hdmi"));
        assert_eq!(status.srt_latency, Some(2000));
        assert_eq!(status.relay.as_deref(), Some("EU (42 ms)"));
        assert_eq!(status.total_kbps, 2000);
        assert_eq!(status.interfaces.len(), 2);
        assert_eq!(status.interfaces[0].name, "usb0");
        assert_eq!(status.interfaces[0].kbps, 2000);
        let modem = status.interfaces[0].modem.as_ref().unwrap();
        assert_eq!(modem.network.as_deref(), Some("Vodafone"));
        assert_eq!(modem.signal, Some(72));
        assert_eq!(status.interfaces[1].error.as_deref(), Some("WiFi hotspot"));
        assert_eq!(status.sensors.len(), 2);

        let json = serde_json::to_string(&status).unwrap();
        for secret in ["10.0.0.2", "192.168.1.10", "secret", "12345"] {
            assert!(!json.contains(secret), "{secret} leaked: {json}");
        }
    }

    #[test]
    fn partial_modem_updates_merge() {
        let now = Instant::now();
        let mut state = online();
        state.apply(
            r#"{"status":{"modems":{"1":{"ifname":"usb0","status":{"signal":40}}}}}"#,
            now,
        );
        state.apply(
            r#"{"status":{"modems":{"1":{"status":{"signal":80}}}}}"#,
            now,
        );
        let status = state.status(now);
        let modem = status.interfaces[0].modem.as_ref().unwrap();
        assert_eq!(modem.signal, Some(80));

        state.apply(r#"{"status":{"modems":{}}}"#, now);
        assert_eq!(state.status(now).interfaces[0].modem, None);
    }

    fn keys(state: &mut State) -> Vec<(String, String)> {
        let status = state.status(Instant::now());
        status
            .interfaces
            .into_iter()
            .map(|i| (i.name, i.key))
            .collect()
    }

    #[test]
    fn interface_keys_follow_the_device_not_the_port() {
        let now = Instant::now();
        let mut state = with(&[
            r#"{"status":{"modems":{"1":{"ifname":"usb0","name":"EM7455 - 12345 | Vodafone"}}}}"#,
            r#"{"status":{"wifi":{"0":{"ifname":"wlan0","hw":"Realtek RTL8812AU "}}}}"#,
        ]);
        assert_eq!(
            keys(&mut state),
            [
                ("usb0".into(), opaque_key("EM7455 - 12345")),
                ("wlan0".into(), opaque_key("Realtek RTL8812AU"))
            ]
        );

        // The same modem on another port (and ID), with another SIM.
        state.apply(r#"{"status":{"modems":{}}}"#, now);
        state.apply(
            r#"{"netif":{"usb1":{"ip":"10.0.0.3","tp":0,"enabled":true},"eth0":{"ip":"10.1.0.2","tp":0,"enabled":true}}}"#,
            now,
        );
        state.apply(
            r#"{"status":{"modems":{"2":{"ifname":"usb1","name":"EM7455 - 12345 | <NO SIM>"}}}}"#,
            now,
        );
        assert_eq!(
            keys(&mut state),
            [
                ("eth0".into(), opaque_key("eth0")),
                ("usb1".into(), opaque_key("EM7455 - 12345"))
            ]
        );
    }

    #[test]
    fn identical_adapters_keep_their_interface_names_as_keys() {
        let now = Instant::now();
        let mut state = with(&[
            r#"{"status":{"wifi":{"0":{"ifname":"wlan0","hw":"Mediatek"},"1":{"ifname":"wlan1","hw":"Mediatek"}}}}"#,
        ]);
        state.apply(
            r#"{"netif":{"wlan0":{"ip":"192.168.1.10","tp":0,"enabled":true},"wlan1":{"ip":"192.168.1.11","tp":0,"enabled":true}}}"#,
            now,
        );
        assert_eq!(
            keys(&mut state),
            [
                ("wlan0".into(), opaque_key("wlan0")),
                ("wlan1".into(), opaque_key("wlan1"))
            ]
        );
    }

    #[test]
    fn streaming_and_interface_changes_are_reported() {
        let now = Instant::now();
        let mut state = online();
        let changes = state.apply(r#"{"status":{"is_streaming":true}}"#, now);
        assert_eq!(changes, [Change::Streaming(true)]);
        assert!(state.streaming());

        let changes = state.apply(
            r#"{"netif":{"usb0":{"ip":"10.0.0.2","tp":0,"enabled":false},"wlan0":{"ip":"192.168.1.10","tp":0,"enabled":false}},"id":4}"#,
            now,
        );
        assert_eq!(
            changes,
            [Change::InterfaceToggled {
                name: "usb0".into(),
                enabled: false
            }]
        );

        // Going offline forgets the stream without calling it stopped.
        let changes = state.apply(r#"{"remote":{"is_encoder_online":false}}"#, now);
        assert_eq!(changes, [Change::EncoderOnline(false)]);
        let changes = state.apply(r#"{"remote":{"is_encoder_online":true}}"#, now);
        assert_eq!(changes, [Change::EncoderOnline(true)]);
        let changes = state.apply(r#"{"status":{"is_streaming":false}}"#, now);
        assert_eq!(changes, []);
    }

    #[test]
    fn start_copies_the_stream_settings_only() {
        let mut state = online();
        state.bitrate_sent(4500);
        let frame: Value =
            serde_json::from_str(&state.frame(&BelaboxAction::Start).unwrap()).unwrap();
        assert_eq!(
            frame,
            json!({"start": {"pipeline": "abc", "delay": 0, "max_br": 4500, "srt_latency": 2000,
                "bitrate_overlay": false, "asrc": "HDMI", "acodec": "opus",
                "relay_server": "3", "relay_account": "7"}})
        );
    }

    #[test]
    fn start_needs_saved_settings() {
        let now = Instant::now();
        let mut state = State::default();
        state.connected();
        state.apply(r#"{"remote":{"auth/key":true}}"#, now);
        state.apply(r#"{"remote":{"is_encoder_online":true}}"#, now);
        state.apply(r#"{"config":{"remote_key":"secret"}}"#, now);
        assert_eq!(
            state.frame(&BelaboxAction::Start),
            Err(BelaboxRefusal::NoConfig)
        );
    }

    #[test]
    fn a_bitrate_set_while_not_streaming_waits_for_the_next_start() {
        let now = Instant::now();
        let mut state = online();
        // Sent anyway (we may not know it's streaming), like belabot.
        assert_eq!(
            state.frame(&BelaboxAction::Bitrate { max_br: 4000 }),
            Ok(r#"{"bitrate":{"max_br":4000}}"#.to_owned())
        );
        state.bitrate_sent(4000);
        assert_eq!(state.status(now).max_br, Some(4000));
        // A config from BELABOX (e.g. after a reconnect) doesn't undo it.
        state.apply(CONFIG, now);
        assert_eq!(state.status(now).max_br, Some(4000));

        let start: Value =
            serde_json::from_str(&state.frame(&BelaboxAction::Start).unwrap()).unwrap();
        assert_eq!(start["start"]["max_br"], 4000);
        state.apply(r#"{"status":{"is_streaming":true}}"#, now);
        state.apply(CONFIG, now);
        // Streaming: BELABOX's own setting counts again.
        assert_eq!(state.status(now).max_br, Some(6000));
    }

    #[test]
    fn commands_check_the_stream_state() {
        let now = Instant::now();
        let mut state = online();
        assert_eq!(
            state.frame(&BelaboxAction::Stop),
            Err(BelaboxRefusal::NotStreaming)
        );

        state.apply(r#"{"status":{"is_streaming":true}}"#, now);
        assert_eq!(
            state.frame(&BelaboxAction::Start),
            Err(BelaboxRefusal::AlreadyStreaming)
        );
        assert_eq!(
            state.frame(&BelaboxAction::Bitrate { max_br: 200 }),
            Err(BelaboxRefusal::InvalidBitrate)
        );
        assert_eq!(
            state.frame(&BelaboxAction::Bitrate { max_br: 5000 }),
            Ok(r#"{"bitrate":{"max_br":5000}}"#.to_owned())
        );
        assert_eq!(
            state.frame(&BelaboxAction::Stop),
            Ok(r#"{"stop":0}"#.to_owned())
        );
    }

    #[test]
    fn interfaces_are_toggled_by_name_with_their_ip() {
        let state = online();
        let frame: Value = serde_json::from_str(
            &state
                .frame(&BelaboxAction::Interface {
                    name: "wlan0".into(),
                    enabled: true,
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            frame,
            json!({"netif": {"name": "wlan0", "ip": "192.168.1.10", "enabled": true}})
        );

        assert_eq!(
            state.frame(&BelaboxAction::Interface {
                name: "usb0".into(),
                enabled: false
            }),
            Err(BelaboxRefusal::LastInterface)
        );
        assert_eq!(
            state.frame(&BelaboxAction::Interface {
                name: "eth9".into(),
                enabled: true
            }),
            Err(BelaboxRefusal::UnknownInterface)
        );
    }

    #[test]
    fn notifications_come_and_go() {
        let now = Instant::now();
        let mut state = online();
        let show = r#"{"notification":{"show":[{"name":"start_error","type":"error",
            "msg":"Failed to start, no available network connections","is_dismissable":true,
            "is_persistent":false,"duration":10}]},"id":3}"#;
        assert_eq!(
            state.apply(show, now),
            [Change::Error(
                "Failed to start, no available network connections".into()
            )]
        );
        // Shown again: not new.
        assert_eq!(state.apply(show, now), []);
        assert_eq!(state.status(now).notifications.len(), 1);
        assert_eq!(
            state.status(now + Duration::from_secs(11)).notifications,
            []
        );

        state.apply(
            r#"{"notification":{"show":[{"name":"netif_dup_ip","type":"warning","msg":"Duplicate IP","duration":0}]}}"#,
            now,
        );
        assert_eq!(state.status(now).notifications.len(), 1);
        state.apply(r#"{"notification":{"remove":["netif_dup_ip"]}}"#, now);
        assert_eq!(state.status(now).notifications, []);
    }

    // --- WiFi, modems and stream settings --------------------------------------

    const WIFI_STATUS: &str = r#"{"status":{"wifi":{
        "0":{"ifname":"wlan0","conn":"uuid-home","hw":"Realtek RTL8812AU ",
            "available":[
                {"active":false,"ssid":"Cafe","signal":80,"security":"WPA2","freq":5180},
                {"active":true,"ssid":"Home","signal":55,"security":"WPA2","freq":2412},
                {"active":false,"ssid":"Open","signal":30,"security":"","freq":2437}],
            "saved":{"Home":"uuid-home","Away":"uuid-away"},"supports_hotspot":true},
        "1":{"ifname":"wlan1","conn":"uuid-hs","hw":"Mediatek","hotspot":{
            "name":"BELABOX","password":"hunter22","channel":"auto",
            "available_channels":{"auto":{"name":"Auto (any band)"},"auto_50":{"name":"Auto (5.0 GHz)"}}}}
        }}}"#;

    const MODEM_STATUS: &str = r#"{"status":{"modems":{"1":{
        "ifname":"usb0","name":"EM7455 - 12345 | Vodafone",
        "network_type":{"supported":["3G","4G"],"active":"4G"},
        "config":{"autoconfig":true,"apn":"internet","username":"u","password":"pw",
            "roaming":false,"network":""},
        "available_networks":{},
        "status":{"connection":"connected","network":"Vodafone","network_type":"4G",
            "signal":70,"roaming":false}}}}}"#;

    fn with(frames: &[&str]) -> State {
        let mut state = online();
        for frame in frames {
            state.apply(frame, Instant::now());
        }
        state
    }

    fn sent(state: &State, action: BelaboxAction) -> Value {
        serde_json::from_str(&state.frame(&action).unwrap()).unwrap()
    }

    #[test]
    fn wifi_lists_networks_saved_and_in_range_without_the_hotspot_password() {
        let details = with(&[WIFI_STATUS]).details().unwrap();
        let adapter = &details.wifi[0];
        assert_eq!(adapter.interface, "wlan0");
        assert_eq!(adapter.hardware.as_deref(), Some("Realtek RTL8812AU"));
        assert!(adapter.supports_hotspot);

        // Connected first, then by signal, then the saved one that's out of range.
        let ssids: Vec<_> = adapter.networks.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(ssids, ["Home", "Cafe", "Open", "Away"]);
        assert!(adapter.networks[0].connected);
        assert_eq!(adapter.networks[0].saved.as_deref(), Some("uuid-home"));
        assert_eq!(adapter.networks[2].security, None);
        assert!(!adapter.networks[3].available);

        let hotspot = details.wifi[1].hotspot.as_ref().unwrap();
        assert!(hotspot.has_password);
        assert_eq!(hotspot.channels.len(), 2);
        assert!(details.wifi[1].networks.is_empty());
        assert!(
            !serde_json::to_string(&details)
                .unwrap()
                .contains("hunter22"),
            "the hotspot password must not leave obs-link"
        );
    }

    #[test]
    fn wifi_orders_are_checked_against_what_the_belabox_has() {
        let state = with(&[WIFI_STATUS]);
        assert_eq!(
            sent(
                &state,
                BelaboxAction::WifiConnect {
                    uuid: "uuid-away".into()
                }
            ),
            json!({"wifi": {"connect": "uuid-away"}})
        );
        assert_eq!(
            state.frame(&BelaboxAction::WifiConnect {
                uuid: "nope".into()
            }),
            Err(BelaboxRefusal::UnknownWifi)
        );
        // Only the network it's on can be disconnected.
        assert!(
            state
                .frame(&BelaboxAction::WifiDisconnect {
                    uuid: "uuid-home".into()
                })
                .is_ok()
        );
        assert_eq!(
            state.frame(&BelaboxAction::WifiDisconnect {
                uuid: "uuid-away".into()
            }),
            Err(BelaboxRefusal::UnknownWifi)
        );
        assert_eq!(
            sent(
                &state,
                BelaboxAction::WifiNew {
                    device: "0".into(),
                    ssid: "Cafe".into(),
                    password: Some("12345678".into()),
                }
            ),
            json!({"wifi": {"new": {"device": "0", "ssid": "Cafe", "password": "12345678"}}})
        );
        let open = sent(
            &state,
            BelaboxAction::WifiNew {
                device: "0".into(),
                ssid: "Open".into(),
                password: None,
            },
        );
        assert!(open["wifi"]["new"].get("password").is_none());
        for (ssid, password, field) in [
            ("Cafe", Some("short"), BelaboxField::WifiPassword),
            ("", None, BelaboxField::WifiName),
        ] {
            assert_eq!(
                state.frame(&BelaboxAction::WifiNew {
                    device: "0".into(),
                    ssid: ssid.into(),
                    password: password.map(str::to_owned),
                }),
                Err(BelaboxRefusal::Invalid(field))
            );
        }
        // A hotspot can't join networks.
        assert_eq!(
            state.frame(&BelaboxAction::WifiNew {
                device: "1".into(),
                ssid: "Cafe".into(),
                password: None,
            }),
            Err(BelaboxRefusal::UnknownWifi)
        );
    }

    #[test]
    fn the_hotspot_keeps_its_password_when_only_the_name_changes() {
        let state = with(&[WIFI_STATUS]);
        let config = |password: Option<&str>, channel: &str| BelaboxAction::HotspotConfig {
            device: "1".into(),
            name: "Mine".into(),
            password: password.map(str::to_owned),
            channel: channel.into(),
        };
        assert_eq!(
            sent(&state, config(None, "auto_50")),
            json!({"wifi": {"hotspot": {"config": {
                "device": "1", "name": "Mine", "password": "hunter22", "channel": "auto_50"
            }}}})
        );
        assert_eq!(
            state.frame(&config(Some("short"), "auto")),
            Err(BelaboxRefusal::Invalid(BelaboxField::HotspotPassword))
        );
        assert_eq!(
            state.frame(&config(None, "auto_24")),
            Err(BelaboxRefusal::Invalid(BelaboxField::HotspotChannel))
        );
        // Not a hotspot (yet): only start works there.
        assert_eq!(
            state.frame(&BelaboxAction::HotspotConfig {
                device: "0".into(),
                name: "Mine".into(),
                password: Some("longenough".into()),
                channel: "auto".into(),
            }),
            Err(BelaboxRefusal::UnknownWifi)
        );
        assert_eq!(
            sent(
                &state,
                BelaboxAction::Hotspot {
                    device: "0".into(),
                    active: true
                }
            ),
            json!({"wifi": {"hotspot": {"start": {"device": "0"}}}})
        );
        assert_eq!(
            sent(
                &state,
                BelaboxAction::Hotspot {
                    device: "1".into(),
                    active: false
                }
            ),
            json!({"wifi": {"hotspot": {"stop": {"device": "1"}}}})
        );
    }

    #[test]
    fn wifi_results_arrive_later_and_count_up() {
        let mut state = with(&[WIFI_STATUS]);
        assert_eq!(state.details().unwrap().result, None);

        state.apply(
            r#"{"wifi":{"new":{"error":"auth","device":"0"}}}"#,
            Instant::now(),
        );
        let first = state.details().unwrap().result.unwrap();
        assert_eq!(first.kind, BelaboxResultKind::WifiNew);
        assert_eq!(first.error.as_deref(), Some("auth"));
        assert_eq!(first.device, "0");

        state.apply(r#"{"wifi":{"connect":true,"device":0}}"#, Instant::now());
        let second = state.details().unwrap().result.unwrap();
        assert_eq!(second.kind, BelaboxResultKind::WifiConnect);
        assert_eq!(second.error, None);
        assert_eq!(second.device, "0");
        assert!(second.id > first.id);

        state.apply(
            r#"{"wifi":{"hotspot":{"config":{"device":"1","error":"password"}}}}"#,
            Instant::now(),
        );
        let third = state.details().unwrap().result.unwrap();
        assert_eq!(third.kind, BelaboxResultKind::HotspotConfig);
        assert_eq!(third.error.as_deref(), Some("password"));
    }

    #[test]
    fn modems_keep_their_password_and_merge_partial_updates() {
        let mut state = with(&[MODEM_STATUS]);
        let details = state.details().unwrap();
        let modem = &details.modems[0];
        assert_eq!(modem.name, "EM7455 - 12345 | Vodafone");
        assert_eq!(modem.network_types, ["3G", "4G"]);
        assert_eq!(modem.network_type.as_deref(), Some("4G"));
        let config = modem.config.as_ref().unwrap();
        assert!(config.has_password);
        assert_eq!(config.autoconfig, Some(true));
        assert!(!serde_json::to_string(&details).unwrap().contains("\"pw\""));

        // A scan result and a status update each carry only their part.
        state.apply(
            r#"{"status":{"modems":{"1":{"available_networks":{"23415":{"name":"Vodafone UK","availability":"available"}}}}}}"#,
            Instant::now(),
        );
        state.apply(
            r#"{"status":{"modems":{"1":{"status":{"signal":10,"connection":"scanning"}}}}}"#,
            Instant::now(),
        );
        let modem = state.details().unwrap().modems.remove(0);
        assert_eq!(modem.networks[0].id, "23415");
        assert_eq!(modem.networks[0].availability.as_deref(), Some("available"));
        assert_eq!(modem.status.unwrap().signal, Some(10));
        assert_eq!(modem.config.unwrap().apn, "internet");

        let config = |network: &str, network_type: &str, password: Option<&str>| {
            BelaboxAction::ModemConfig {
                modem: "1".into(),
                autoconfig: false,
                apn: "web".into(),
                username: "u".into(),
                password: password.map(str::to_owned),
                roaming: true,
                network: network.into(),
                network_type: network_type.into(),
            }
        };
        assert_eq!(
            sent(&state, config("23415", "3G", None)),
            json!({"modems": {"config": {
                "device": "1", "autoconfig": false, "apn": "web", "username": "u",
                "password": "pw", "roaming": true, "network": "23415", "network_type": "3G"
            }}})
        );
        assert_eq!(
            sent(&state, config("", "4G", Some("new")))["modems"]["config"]["password"],
            "new"
        );
        assert_eq!(
            state.frame(&config("99999", "4G", None)),
            Err(BelaboxRefusal::Invalid(BelaboxField::Network))
        );
        assert_eq!(
            state.frame(&config("", "5G", None)),
            Err(BelaboxRefusal::Invalid(BelaboxField::NetworkType))
        );
        assert_eq!(
            state
                .frame(&BelaboxAction::ModemScan { modem: "1".into() })
                .map(|f| f.contains("scan")),
            Ok(true)
        );
        assert_eq!(
            state.frame(&BelaboxAction::ModemScan { modem: "9".into() }),
            Err(BelaboxRefusal::UnknownModem)
        );
    }

    fn settings() -> BelaboxSettingsUpdate {
        BelaboxSettingsUpdate {
            pipeline: "abc".into(),
            audio_source: Some("HDMI".into()),
            audio_codec: Some("aac".into()),
            audio_delay: -150,
            srt_latency: 3000,
            bitrate_overlay: true,
            relay_server: Some("3".into()),
            relay_account: Some("7".into()),
            srtla_address: None,
            srtla_port: None,
            stream_id: None,
        }
    }

    const SETTINGS_FRAMES: [&str; 3] = [
        r#"{"status":{"asrcs":["HDMI","USB audio"]}}"#,
        r#"{"acodecs":{"opus":"Opus","aac":"AAC"}}"#,
        r#"{"relays":{"servers":{"3":{"name":"EU"},"4":{"name":"US"}},
            "accounts":{"7":{"name":"Mine"},"8":{"name":"Old","disabled":true}}}}"#,
    ];

    #[test]
    fn stream_settings_wait_for_the_next_start() {
        let mut state = with(&SETTINGS_FRAMES);
        assert!(!state.details().unwrap().settings_pending);

        assert_eq!(
            state.prepare(&BelaboxAction::Settings {
                settings: settings()
            }),
            Ok(None)
        );
        let details = state.details().unwrap();
        assert!(details.settings_pending);
        assert_eq!(details.settings.audio_codec.as_deref(), Some("aac"));
        assert_eq!(details.settings.audio_delay, Some(-150));
        assert_eq!(details.settings.srt_latency, Some(3000));
        assert_eq!(details.settings.bitrate_overlay, Some(true));
        assert_eq!(state.status(Instant::now()).srt_latency, Some(3000));

        // The next start carries them; BELABOX saves them then.
        let start = sent(&state, BelaboxAction::Start);
        assert_eq!(start["start"]["acodec"], "aac");
        assert_eq!(start["start"]["delay"], -150);
        assert_eq!(start["start"]["srt_latency"], 3000);
        assert_eq!(start["start"]["relay_account"], "7");

        // Once it streams they're the BELABOX's own.
        state.apply(r#"{"status":{"is_streaming":true}}"#, Instant::now());
        assert!(!state.details().unwrap().settings_pending);
    }

    #[test]
    fn switching_between_a_relay_and_your_own_server_drops_the_other() {
        let mut state = with(&SETTINGS_FRAMES);
        let own = BelaboxSettingsUpdate {
            relay_server: None,
            relay_account: None,
            srtla_address: Some(" ingest.example.com ".into()),
            srtla_port: Some(5000),
            stream_id: Some("live_123".into()),
            ..settings()
        };
        state
            .prepare(&BelaboxAction::Settings { settings: own })
            .unwrap();

        let start = sent(&state, BelaboxAction::Start);
        let start = &start["start"];
        assert_eq!(start["srtla_addr"], "ingest.example.com");
        assert_eq!(start["srtla_port"], 5000);
        assert_eq!(start["srt_streamid"], "live_123");
        assert!(start.get("relay_server").is_none());
        assert!(start.get("relay_account").is_none());
        let settings = state.details().unwrap().settings;
        assert_eq!(
            settings.srtla_address.as_deref(),
            Some("ingest.example.com")
        );
        assert!(settings.has_stream_id);

        // And back: the relay's account replaces the stream ID.
        state
            .prepare(&BelaboxAction::Settings {
                settings: settings_for_relay(),
            })
            .unwrap();
        let start = sent(&state, BelaboxAction::Start);
        let start = &start["start"];
        assert_eq!(start["relay_server"], "3");
        assert!(start.get("srtla_addr").is_none());
        assert!(start.get("srt_streamid").is_none());
    }

    fn settings_for_relay() -> BelaboxSettingsUpdate {
        settings()
    }

    #[test]
    fn stream_settings_are_checked_like_belaui_does() {
        let mut state = with(&SETTINGS_FRAMES);
        let mut refuse = |settings: BelaboxSettingsUpdate, field: BelaboxField| {
            assert_eq!(
                state.prepare(&BelaboxAction::Settings { settings }),
                Err(BelaboxRefusal::Invalid(field))
            );
        };

        refuse(
            BelaboxSettingsUpdate {
                pipeline: "nope".into(),
                ..settings()
            },
            BelaboxField::Pipeline,
        );
        refuse(
            BelaboxSettingsUpdate {
                audio_delay: 2001,
                ..settings()
            },
            BelaboxField::AudioDelay,
        );
        refuse(
            BelaboxSettingsUpdate {
                srt_latency: 99,
                ..settings()
            },
            BelaboxField::SrtLatency,
        );
        refuse(
            BelaboxSettingsUpdate {
                srt_latency: 10_001,
                ..settings()
            },
            BelaboxField::SrtLatency,
        );
        refuse(
            BelaboxSettingsUpdate {
                audio_codec: Some("mp3".into()),
                ..settings()
            },
            BelaboxField::AudioCodec,
        );
        refuse(
            BelaboxSettingsUpdate {
                audio_codec: None,
                ..settings()
            },
            BelaboxField::AudioCodec,
        );
        refuse(
            BelaboxSettingsUpdate {
                audio_source: Some("Mic".into()),
                ..settings()
            },
            BelaboxField::AudioSource,
        );
        refuse(
            BelaboxSettingsUpdate {
                relay_server: Some("9".into()),
                ..settings()
            },
            BelaboxField::Relay,
        );
        refuse(
            BelaboxSettingsUpdate {
                relay_account: Some("9".into()),
                ..settings()
            },
            BelaboxField::RelayAccount,
        );
        refuse(
            BelaboxSettingsUpdate {
                relay_server: None,
                srtla_address: None,
                ..settings()
            },
            BelaboxField::Address,
        );
        refuse(
            BelaboxSettingsUpdate {
                relay_server: None,
                relay_account: None,
                srtla_address: Some("host".into()),
                srtla_port: Some(0),
                stream_id: Some("k".into()),
                ..settings()
            },
            BelaboxField::Port,
        );
        // An account needs its relay; your own server needs a stream ID the BELABOX
        // doesn't have yet (the saved config uses an account).
        refuse(
            BelaboxSettingsUpdate {
                relay_server: None,
                srtla_address: Some("host".into()),
                srtla_port: Some(5000),
                ..settings()
            },
            BelaboxField::RelayAccount,
        );
        refuse(
            BelaboxSettingsUpdate {
                relay_account: None,
                stream_id: None,
                ..settings()
            },
            BelaboxField::StreamId,
        );
        assert!(
            !state.details().unwrap().settings_pending,
            "a refusal changes nothing"
        );

        // The saved audio source may be an input that isn't plugged in right now.
        state.apply(r#"{"status":{"asrcs":["USB audio"]}}"#, Instant::now());
        assert!(
            state
                .prepare(&BelaboxAction::Settings {
                    settings: settings()
                })
                .is_ok()
        );
    }

    #[test]
    fn stream_settings_are_refused_while_streaming_or_offline() {
        let mut state = with(&SETTINGS_FRAMES);
        state.apply(r#"{"status":{"is_streaming":true}}"#, Instant::now());
        assert_eq!(
            state.prepare(&BelaboxAction::Settings {
                settings: settings()
            }),
            Err(BelaboxRefusal::Streaming)
        );
        state.apply(r#"{"remote":{"is_encoder_online":false}}"#, Instant::now());
        assert_eq!(
            state.prepare(&BelaboxAction::Settings {
                settings: settings()
            }),
            Err(BelaboxRefusal::EncoderOffline)
        );
        assert_eq!(state.details(), Err(BelaboxRefusal::EncoderOffline));
    }

    #[test]
    fn details_never_hold_the_stream_id_or_secrets() {
        let state = with(&SETTINGS_FRAMES);
        let json = serde_json::to_string(&state.details().unwrap()).unwrap();
        assert!(!json.contains("secret"), "remote key");
        assert!(!json.contains("hunter2"), "ssh password");
    }
}
