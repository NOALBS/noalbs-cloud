//! A fake BELABOX Cloud with one BELABOX behind it, for trying the BELABOX
//! integration without a device:
//!
//!     cargo run -p obs-link --example fake_belabox           # listens on :18200
//!
//! Point obs-link at it with `OBS_LINK_BELABOX_URL=ws://localhost:18200` and save
//! the remote key `test` on a switcher (any other key is refused).
//!
//! Remote clients get what belaUI sends BELABOX Cloud: `config`, `pipelines`,
//! `relays`, `status` (with a modem), then `netif` and `sensors` every second.
//! `start`, `stop`, `bitrate` and `netif` work as on a BELABOX, including its
//! checks (the interface's IP must match; not the last enabled interface; a
//! start with every interface off fails with a `start_error` notification).
//! `start` validates and saves the stream settings (and sends the new `config`) like
//! belaUI's `validateConfig`. `wifi` (scan, connect, disconnect, new, forget, hotspot start /
//! stop / config) and `modems` (config, scan) work too: a new WiFi network with a password
//! starting with `wrong` (or none, for a secured one) fails with `auth`; anything else joins.
//!
//! Type into its terminal: `offline` / `online` takes the BELABOX off BELABOX
//! Cloud or back, `drop` closes every client connection.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{
    io::AsyncBufReadExt,
    net::{TcpListener, TcpStream},
    sync::broadcast,
};
use tokio_tungstenite::tungstenite::Message;

const KEY: &str = "test";

struct Netif {
    ip: &'static str,
    enabled: bool,
    /// kbps it sends while streaming.
    kbps: u64,
}

const PIPELINE: &str = "7ca3d9dd20726a7c2dad06948e1eadc6f84c461c";
/// A pipeline without audio settings (no `asrc`, no `acodec`).
const VIDEO_ONLY: &str = "1f0ea3a2d1c0a8a1b2f3d4e5a6b7c8d9e0f1a2b3";

struct Hotspot {
    name: String,
    password: String,
    channel: String,
}

struct Wifi {
    /// UUID of the connection it is on.
    conn: Option<String>,
    /// SSID → connection UUID.
    saved: BTreeMap<String, String>,
    /// SSID, signal, security, MHz.
    available: Vec<(&'static str, u32, &'static str, u32)>,
    hotspot: Option<Hotspot>,
}

struct Modem {
    autoconfig: bool,
    apn: String,
    username: String,
    password: String,
    roaming: bool,
    network: String,
    network_type: String,
    /// Operator ID → (name, availability), after a scan.
    available_networks: BTreeMap<String, (String, String)>,
}

struct Device {
    online: bool,
    streaming: bool,
    max_br: u64,
    netif: BTreeMap<&'static str, Netif>,
    tick: u64,
    /// The saved config, as belaUI broadcasts it.
    config: Value,
    wifi: Wifi,
    modem: Modem,
    next_uuid: u32,
}

impl Device {
    fn new() -> Self {
        Self {
            online: true,
            streaming: false,
            max_br: 6000,
            netif: BTreeMap::from([
                (
                    "usb0",
                    Netif {
                        ip: "10.64.1.2",
                        enabled: true,
                        kbps: 2500,
                    },
                ),
                (
                    "usb1",
                    Netif {
                        ip: "10.65.7.9",
                        enabled: true,
                        kbps: 1800,
                    },
                ),
                (
                    "wlan0",
                    Netif {
                        ip: "192.168.4.1",
                        enabled: false,
                        kbps: 0,
                    },
                ),
            ]),
            tick: 0,
            config: json!({
                "remote_key": KEY, "max_br": 6000, "delay": 0, "pipeline": PIPELINE,
                "srt_latency": 2000, "bitrate_overlay": false, "asrc": "HDMI",
                "acodec": "opus", "relay_server": "1", "relay_account": "1",
                "ssh_pass": "hunter2"
            }),
            wifi: Wifi {
                conn: Some("uuid-home".into()),
                saved: BTreeMap::from([
                    ("Home".into(), "uuid-home".into()),
                    ("Hotel".into(), "uuid-hotel".into()),
                ]),
                available: vec![
                    ("Home", 62, "WPA2", 2437),
                    ("Neighbour", 41, "WPA2", 5180),
                    ("Cafe Guest", 28, "", 2412),
                ],
                hotspot: None,
            },
            modem: Modem {
                autoconfig: false,
                apn: "internet".into(),
                username: String::new(),
                password: String::new(),
                roaming: false,
                network: String::new(),
                network_type: "4G".into(),
                available_networks: BTreeMap::new(),
            },
            next_uuid: 1,
        }
    }

    fn netif_message(&self) -> Value {
        let interfaces: serde_json::Map<String, Value> = self
            .netif
            .iter()
            .map(|(name, netif)| {
                // Wobble a bit, like real modems.
                let kbps = if self.streaming && netif.enabled {
                    netif.kbps + (self.tick * 37 + netif.kbps) % 400
                } else {
                    0
                };
                let mut value =
                    json!({ "ip": netif.ip, "tp": kbps * 1024 / 8, "enabled": netif.enabled });
                if *name == "wlan0" {
                    value["error"] = "WiFi hotspot".into();
                }
                (name.to_string(), value)
            })
            .collect();
        json!({ "netif": interfaces })
    }

    /// `status.wifi`: one adapter, either a client or a hotspot.
    fn wifi_value(&self) -> Value {
        let wifi = &self.wifi;
        let mut adapter =
            json!({ "ifname": "wlan0", "conn": wifi.conn, "hw": "Realtek RTL8812AU " });
        if let Some(hotspot) = &wifi.hotspot {
            adapter["conn"] = "uuid-hotspot".into();
            adapter["hotspot"] = json!({
                "name": hotspot.name, "password": hotspot.password, "channel": hotspot.channel,
                "available_channels": {
                    "auto": { "name": "Auto (any band)" },
                    "auto_24": { "name": "Auto (2.4 GHz)" },
                    "auto_50": { "name": "Auto (5.0 GHz)" },
                },
            });
        } else {
            adapter["supports_hotspot"] = true.into();
            adapter["saved"] = json!(wifi.saved);
            adapter["available"] = wifi
                .available
                .iter()
                .map(|(ssid, signal, security, freq)| {
                    let active = wifi
                        .conn
                        .as_ref()
                        .is_some_and(|conn| wifi.saved.get(*ssid) == Some(conn));
                    json!({ "active": active, "ssid": ssid, "signal": signal,
                        "security": security, "freq": freq })
                })
                .collect();
        }
        json!({ "0": adapter })
    }

    fn wifi_message(&self) -> Value {
        json!({ "status": { "wifi": self.wifi_value() } })
    }

    fn modem_value(&self, full: bool) -> Value {
        let modem = &self.modem;
        let mut value = json!({ "status": { "connection": "connected", "network": "Vodafone",
            "network_type": modem.network_type, "signal": 60 + self.tick % 20,
            "roaming": modem.roaming } });
        if full {
            value["ifname"] = "usb0".into();
            value["name"] = "EM7455 - 12345 | Vodafone".into();
            value["network_type"] =
                json!({ "supported": ["3G", "4G", "3G4G"], "active": modem.network_type });
            value["config"] = json!({ "autoconfig": modem.autoconfig, "apn": modem.apn,
                "username": modem.username, "password": modem.password,
                "roaming": modem.roaming, "network": modem.network });
            value["available_networks"] = modem
                .available_networks
                .iter()
                .map(|(id, (name, availability))| {
                    (
                        id.clone(),
                        json!({ "name": name, "availability": availability }),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
                .into();
        }
        value
    }

    fn status(&self) -> Value {
        json!({ "status": {
            "is_streaming": self.streaming,
            "available_updates": false,
            "updating": null,
            "wifi": self.wifi_value(),
            "asrcs": ["HDMI", "USB audio", "No audio"],
            "modems": { "1": self.modem_value(true) },
        } })
    }
}

type Shared = Arc<Mutex<Device>>;

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "18200".into());
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    println!("Fake BELABOX Cloud on ws://localhost:{port} (remote key `{KEY}`)");

    let device: Shared = Arc::new(Mutex::new(Device::new()));
    let (drop_tx, _) = broadcast::channel::<()>(4);

    let commands = {
        let device = device.clone();
        let drop_tx = drop_tx.clone();
        async move {
            let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match line.trim() {
                    "offline" => device.lock().unwrap().online = false,
                    "online" => device.lock().unwrap().online = true,
                    "drop" => {
                        let _ = drop_tx.send(());
                    }
                    other => println!("Unknown command {other:?} (offline, online, drop)"),
                }
            }
        }
    };
    tokio::spawn(commands);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(client(stream, device.clone(), drop_tx.subscribe()));
    }
}

async fn client(stream: TcpStream, device: Shared, mut dropped: broadcast::Receiver<()>) {
    let Ok(socket) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    let (mut sink, mut stream) = socket.split();
    let send = |value: Value| Message::Text(value.to_string().into());

    // Authentication first.
    let key = loop {
        match stream.next().await {
            Some(Ok(Message::Text(text))) => {
                let message: Value = serde_json::from_str(&text).unwrap_or_default();
                if let Some(key) = message["remote"]["auth/key"]["key"].as_str() {
                    break key.to_owned();
                }
            }
            Some(Ok(_)) => {}
            _ => return,
        }
    };
    if key != KEY {
        println!("Refused remote key {key:?}");
        let _ = sink
            .send(send(json!({ "remote": { "auth/key": false } })))
            .await;
        return;
    }
    println!("Remote client connected");
    let _ = sink
        .send(send(json!({ "remote": { "auth/key": true } })))
        .await;

    let mut online = None;
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let mut out = Vec::new();
                {
                    let mut device = device.lock().unwrap();
                    device.tick += 1;
                    if online != Some(device.online) {
                        online = Some(device.online);
                        out.push(json!({ "remote": { "is_encoder_online": device.online, "version": 16 } }));
                        if device.online {
                            let mut config = device.config.clone();
                            config["max_br"] = device.max_br.into();
                            out.push(json!({ "config": config }));
                            out.push(json!({ "pipelines": {
                                (PIPELINE): { "name": "h265_hdmi_1080p", "asrc": true, "acodec": true },
                                (VIDEO_ONLY): { "name": "h264_hdmi_720p_noaudio", "asrc": false, "acodec": false },
                            } }));
                            out.push(json!({ "acodecs": {
                                "opus": "Opus (better quality)", "aac": "AAC (backwards compatibility)"
                            } }));
                            out.push(json!({ "relays": {
                                "servers": {
                                    "1": { "name": "🟢 EU West (21 ms)", "default": true },
                                    "2": { "name": "🟡 US East (118 ms)" },
                                },
                                "accounts": {
                                    "1": { "name": "My stream" },
                                    "2": { "name": "Old channel", "disabled": true },
                                }
                            } }));
                            out.push(device.status());
                            out.push(json!({ "notification": { "show": [{
                                "name": "update", "type": "warning",
                                "msg": "A software update is available", "is_dismissable": true,
                                "is_persistent": true, "duration": 0
                            }] } }));
                        }
                    }
                    if device.online {
                        out.push(device.netif_message());
                        out.push(json!({ "sensors": {
                            "SoC temperature": format!("{:.1} °C", 45.0 + (device.tick % 30) as f64 / 10.0),
                            "SoC voltage": "5.012 V",
                            "SoC current": "1.402 A",
                        } }));
                        if device.tick.is_multiple_of(5) {
                            out.push(json!({ "status": { "modems": { "1": device.modem_value(false) } } }));
                        }
                    }
                }
                for message in out {
                    if sink.send(send(message)).await.is_err() {
                        return;
                    }
                }
            }
            _ = dropped.recv() => {
                println!("Dropping a remote client");
                return;
            }
            message = stream.next() => {
                let Some(Ok(message)) = message else {
                    println!("Remote client gone");
                    return;
                };
                let Message::Text(text) = message else { continue };
                let message: Value = serde_json::from_str(&text).unwrap_or_default();
                let replies = handle(&device, &message);
                for reply in replies {
                    if sink.send(send(reply)).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

/// belaUI's `handleMessage`, for what remote clients send.
fn handle(device: &Shared, message: &Value) -> Vec<Value> {
    let mut device = device.lock().unwrap();
    if !device.online {
        return Vec::new();
    }
    let error = |name: &str, msg: &str| {
        json!({ "notification": { "show": [{ "name": name, "type": "error", "msg": msg,
            "is_dismissable": true, "is_persistent": false, "duration": 10 }] }, "id": 1 })
    };

    if let Some(start) = message.get("start") {
        println!("start {start}");
        if device.streaming {
            return vec![device.status()];
        }
        if !device.netif.values().any(|n| n.enabled) {
            return vec![
                error(
                    "start_error",
                    "Failed to start, no available network connections",
                ),
                json!({ "status": { "is_streaming": false } }),
            ];
        }
        let config = match save_config(&mut device, start) {
            Ok(config) => config,
            Err(message) => {
                return vec![
                    error("start_error", &message),
                    json!({ "status": { "is_streaming": false } }),
                ];
            }
        };
        device.streaming = true;
        return vec![
            json!({ "config": config }),
            json!({ "status": { "is_streaming": true } }),
        ];
    }
    if message.get("stop").is_some() {
        println!("stop");
        device.streaming = false;
        return vec![json!({ "status": { "is_streaming": false } })];
    }
    if let Some(max_br) = message["bitrate"]["max_br"].as_u64() {
        println!("bitrate {max_br}");
        if device.streaming && (300..=12000).contains(&max_br) {
            device.max_br = max_br;
        }
        return Vec::new();
    }
    if let Some(netif) = message.get("netif") {
        println!("netif {netif}");
        let (Some(name), Some(ip), Some(enabled)) = (
            netif["name"].as_str(),
            netif["ip"].as_str(),
            netif["enabled"].as_bool(),
        ) else {
            return Vec::new();
        };
        let enabled_count = device.netif.values().filter(|n| n.enabled).count();
        let Some(interface) = device.netif.get_mut(name) else {
            return Vec::new();
        };
        if interface.ip != ip {
            return Vec::new();
        }
        if !enabled && interface.enabled && enabled_count == 1 {
            return vec![error("netif_disable_all", "Can't disable all networks")];
        }
        interface.enabled = enabled;
        return vec![device.netif_message()];
    }
    if let Some(wifi) = message.get("wifi") {
        println!("wifi {}", redact(wifi));
        return handle_wifi(&mut device, wifi);
    }
    if let Some(modems) = message.get("modems") {
        println!("modems {}", redact(modems));
        return handle_modems(&mut device, modems);
    }
    Vec::new()
}

/// A message for the log without its passwords.
fn redact(message: &Value) -> String {
    let mut message = message.to_string();
    for key in ["password"] {
        // Good enough for a fake: hide whatever follows `"password":`.
        while let Some(at) = message.find(&format!("\"{key}\":\"")) {
            let start = at + key.len() + 4;
            let Some(len) = message[start..].find('"') else {
                break;
            };
            message.replace_range(start..start + len, "<hidden>");
            message.replace_range(at..at + key.len() + 2, &format!("\"{key}_\""));
        }
    }
    message
}

fn handle_wifi(device: &mut Device, wifi: &Value) -> Vec<Value> {
    let adapter = "0";
    if let Some(uuid) = wifi["connect"].as_str() {
        if device.wifi.hotspot.is_some() || !device.wifi.saved.values().any(|saved| saved == uuid) {
            return vec![json!({ "wifi": { "connect": false, "device": adapter } })];
        }
        device.wifi.conn = Some(uuid.to_owned());
        return vec![
            json!({ "wifi": { "connect": true, "device": adapter } }),
            device.wifi_message(),
        ];
    }
    if let Some(uuid) = wifi["disconnect"].as_str() {
        if device.wifi.conn.as_deref() == Some(uuid) {
            device.wifi.conn = None;
        }
        return vec![device.wifi_message()];
    }
    if wifi.get("scan").is_some() {
        // A new neighbour shows up now and then.
        let tick = device.tick;
        device
            .wifi
            .available
            .retain(|(ssid, ..)| *ssid != "Phone hotspot");
        if tick % 2 == 0 {
            device
                .wifi
                .available
                .push(("Phone hotspot", 75, "WPA2", 2462));
        }
        return vec![device.wifi_message()];
    }
    if let Some(uuid) = wifi["forget"].as_str() {
        device.wifi.saved.retain(|_, saved| saved != uuid);
        if device.wifi.conn.as_deref() == Some(uuid) {
            device.wifi.conn = None;
        }
        return vec![device.wifi_message()];
    }
    if let Some(new) = wifi.get("new") {
        let (Some(ssid), device_id) = (new["ssid"].as_str(), new["device"].as_str()) else {
            return Vec::new();
        };
        if device_id != Some(adapter) || device.wifi.hotspot.is_some() {
            return Vec::new();
        }
        let in_range = device
            .wifi
            .available
            .iter()
            .any(|(known, ..)| *known == ssid);
        let password = new["password"].as_str();
        let secured = device
            .wifi
            .available
            .iter()
            .any(|(known, _, security, _)| *known == ssid && !security.is_empty());
        if !in_range || (secured && password.is_none_or(|p| p.starts_with("wrong"))) {
            let error = if in_range { "auth" } else { "generic" };
            return vec![json!({ "wifi": { "new": { "error": error, "device": adapter } } })];
        }
        let uuid = format!("uuid-{}", device.next_uuid);
        device.next_uuid += 1;
        device.wifi.saved.insert(ssid.to_owned(), uuid.clone());
        device.wifi.conn = Some(uuid);
        return vec![
            json!({ "wifi": { "new": { "success": true, "device": adapter } } }),
            device.wifi_message(),
        ];
    }
    if let Some(hotspot) = wifi.get("hotspot") {
        if hotspot.get("start").is_some() {
            device.wifi.conn = None;
            device.wifi.hotspot = Some(Hotspot {
                name: "BELABOX".into(),
                password: "belabox123".into(),
                channel: "auto".into(),
            });
            return vec![device.wifi_message()];
        }
        if hotspot.get("stop").is_some() {
            device.wifi.hotspot = None;
            return vec![device.wifi_message()];
        }
        if let Some(config) = hotspot.get("config") {
            let Some(current) = device.wifi.hotspot.as_mut() else {
                return Vec::new();
            };
            let reply = |error: &str| json!({ "wifi": { "hotspot": { "config": { "device": adapter, "error": error } } } });
            let name = config["name"].as_str().unwrap_or_default();
            let password = config["password"].as_str().unwrap_or_default();
            let channel = config["channel"].as_str().unwrap_or_default();
            if name.is_empty() || name.len() > 32 {
                return vec![reply("name")];
            }
            if !(8..=64).contains(&password.len()) {
                return vec![reply("password")];
            }
            if !["auto", "auto_24", "auto_50"].contains(&channel) {
                return vec![reply("channel")];
            }
            current.name = name.to_owned();
            current.password = password.to_owned();
            current.channel = channel.to_owned();
            return vec![
                json!({ "wifi": { "hotspot": { "config": { "device": adapter, "success": true } } } }),
                device.wifi_message(),
            ];
        }
    }
    Vec::new()
}

fn handle_modems(device: &mut Device, modems: &Value) -> Vec<Value> {
    if let Some(config) = modems.get("config") {
        let modem = &mut device.modem;
        let (Some(roaming), Some(autoconfig), Some(apn), Some(username), Some(password)) = (
            config["roaming"].as_bool(),
            config["autoconfig"].as_bool(),
            config["apn"].as_str(),
            config["username"].as_str(),
            config["password"].as_str(),
        ) else {
            return Vec::new();
        };
        let (Some(network), Some(network_type)) =
            (config["network"].as_str(), config["network_type"].as_str())
        else {
            return Vec::new();
        };
        if config["device"].as_str() != Some("1")
            || !["3G", "4G", "3G4G"].contains(&network_type)
            || (!network.is_empty()
                && network != modem.network
                && !modem.available_networks.contains_key(network))
        {
            return Vec::new();
        }
        modem.autoconfig = autoconfig;
        modem.apn = apn.to_owned();
        modem.username = username.to_owned();
        modem.password = password.to_owned();
        modem.roaming = roaming;
        modem.network = network.to_owned();
        modem.network_type = network_type.to_owned();
        return vec![json!({ "status": { "modems": { "1": device.modem_value(true) } } })];
    }
    if modems["scan"]["device"].as_str() == Some("1") {
        device.modem.available_networks = BTreeMap::from([
            ("20404".into(), ("Vodafone NL".into(), "available".into())),
            ("20408".into(), ("KPN".into(), "available".into())),
            ("20416".into(), ("T-Mobile NL".into(), "forbidden".into())),
        ]);
        // belaUI sends the scan result alone: the other fields stay empty.
        let networks = device.modem_value(true)["available_networks"].clone();
        return vec![
            json!({ "status": { "modems": { "1": { "available_networks": networks } } } }),
        ];
    }
    Vec::new()
}

/// belaUI's `validateConfig` + `updateConfig`: checks what `start` carries, saves it
/// and returns the config to broadcast.
fn save_config(device: &mut Device, start: &Value) -> Result<Value, String> {
    let int = |key: &str| start[key].as_i64();
    let delay = int("delay").filter(|d| (-2000..=2000).contains(d));
    let delay = delay.ok_or_else(|| format!("Invalid audio delay '{}'", start["delay"]))?;

    let pipeline = start["pipeline"].as_str().unwrap_or_default();
    if ![PIPELINE, VIDEO_ONLY].contains(&pipeline) {
        return Err("Pipeline not found".into());
    }
    let takes_audio = pipeline == PIPELINE;
    if takes_audio {
        let codec = start["acodec"].as_str().ok_or("Invalid audio codec")?;
        if !["opus", "aac"].contains(&codec) {
            return Err("Audio codec not found".into());
        }
        let source = start["asrc"].as_str().ok_or("Invalid audio source")?;
        if device.config["asrc"].as_str() != Some(source)
            && !["HDMI", "USB audio", "No audio"].contains(&source)
        {
            return Err("Selected audio source not found".into());
        }
    }
    let max_br = int("max_br")
        .filter(|br| (300..=12000).contains(br))
        .ok_or_else(|| format!("Invalid max bitrate: '{}'", start["max_br"]))?;
    let latency = int("srt_latency")
        .filter(|l| (100..=10000).contains(l))
        .ok_or_else(|| format!("Invalid SRT latency '{}' ms", start["srt_latency"]))?;

    let relay = start["relay_server"].as_str();
    if let Some(relay) = relay {
        if !["1", "2"].contains(&relay) {
            return Err("Invalid relay server".into());
        }
    } else {
        start["srtla_addr"]
            .as_str()
            .ok_or("Invalid SRTLA address")?;
        int("srtla_port")
            .filter(|p| (1..=65535).contains(p))
            .ok_or_else(|| format!("Invalid SRTLA port '{}'", start["srtla_port"]))?;
    }
    let account = start["relay_account"].as_str().filter(|_| relay.is_some());
    if let Some(account) = account {
        if !["1", "2"].contains(&account) {
            return Err("Invalid relay account specified!".into());
        }
    } else if start["srt_streamid"].as_str().is_none() {
        return Err("SRT streamid not specified".into());
    }

    let config = &mut device.config;
    config["delay"] = delay.into();
    config["pipeline"] = pipeline.into();
    config["max_br"] = max_br.into();
    config["srt_latency"] = latency.into();
    config["bitrate_overlay"] = start["bitrate_overlay"].clone();
    if takes_audio {
        config["acodec"] = start["acodec"].clone();
        config["asrc"] = start["asrc"].clone();
    }
    let config = config.as_object_mut().expect("the config is an object");
    for key in [
        "relay_server",
        "relay_account",
        "srtla_addr",
        "srtla_port",
        "srt_streamid",
    ] {
        config.remove(key);
    }
    for (key, value) in [
        ("relay_server", &start["relay_server"]),
        ("srtla_addr", &start["srtla_addr"]),
        ("srtla_port", &start["srtla_port"]),
    ] {
        if relay.is_some() == (key == "relay_server") && !value.is_null() {
            config.insert(key.into(), value.clone());
        }
    }
    match account {
        Some(account) => config.insert("relay_account".into(), account.into()),
        None => config.insert("srt_streamid".into(), start["srt_streamid"].clone()),
    };
    device.max_br = max_br as u64;
    Ok(device.config.clone())
}
