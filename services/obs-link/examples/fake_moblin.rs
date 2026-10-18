//! A fake Moblin phone in the remote control *streamer* role, for trying the
//! Moblin integration without an iPhone:
//!
//!     PASSWORD=<the switcher's Moblin password> \
//!         cargo run -p obs-link --example fake_moblin -- <switcher ID>
//!
//! It connects to `ws://localhost:11000/moblin/<switcher ID>` (set `MOBLIN_URL`
//! to the whole Assistant URL instead, e.g. through the dashboard's proxy),
//! answers the challenge like Moblin (`remoteControlHashPassword`), sends its
//! state (with three zoom presets), answers `getStatus` (with a system monitor,
//! Moblink and DJI line), `getSettings` (two scenes, three bitrate presets, three
//! bonding connections) and the `set…` requests, pings every 30 s and reconnects
//! every 5 s like Moblin.
//!
//! Type into its terminal: `live` / `stop` (as if tapped on the phone), `mute` /
//! `unmute`, `scene` (next scene), `torch`, `drop` (closes the connection; it
//! reconnects).

use std::{sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::AsyncBufReadExt,
    sync::{Mutex, mpsc},
};
use tokio_tungstenite::tungstenite::Message;

const SCENES: [(&str, &str); 2] = [
    ("5B3F1D8E-0C2B-4E0B-9C1A-8D7E6F5A4B3C", "Back camera"),
    ("0F9E8D7C-6B5A-4F3E-2D1C-0B9A8F7E6D5C", "Front camera"),
];
/// id, bits per second
const PRESETS: [(&str, u64); 3] = [
    ("11111111-2222-4333-8444-555555555555", 1_000_000),
    ("22222222-3333-4444-8555-666666666666", 3_000_000),
    ("AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE", 6_000_000),
];
/// The back camera's zoom presets: id, name, zoom.
const ZOOM_PRESETS: [(&str, &str, f64); 3] = [
    ("20000000-0000-4000-8000-000000000001", "0.5x", 0.5),
    ("20000000-0000-4000-8000-000000000002", "1x", 1.0),
    ("20000000-0000-4000-8000-000000000003", "3x", 3.0),
];
/// What Moblin reports as the zoom preset when the zoom is none of them.
const NO_ZOOM_PRESET: &str = "99999999-0000-4000-8000-000000000000";

struct Connection {
    id: &'static str,
    name: &'static str,
    priority: i64,
    enabled: bool,
}

struct Phone {
    live: bool,
    recording: bool,
    muted: bool,
    scene: usize,
    preset: usize,
    battery: i64,
    seconds_live: u64,
    torch: bool,
    stealth_mode: bool,
    zoom: f64,
    priorities_enabled: bool,
    connections: Vec<Connection>,
}

impl Phone {
    fn full_state(&self) -> Value {
        json!({
            "scene": SCENES[self.scene].0,
            "bitrate": PRESETS[self.preset].0,
            "streaming": self.live,
            "recording": self.recording,
            "muted": self.muted,
            "zoom": self.zoom,
            "zoomPresets": ZOOM_PRESETS.iter().map(|(id, name, _)| json!({ "id": id, "name": name })).collect::<Vec<_>>(),
            "zoomPreset": self.zoom_preset(),
            "torchOn": self.torch,
            "stealthMode": self.stealth_mode,
            "debugLogging": false,
            // A Swift dictionary with enum keys arrives as an array.
            "filters": [{ "pixellate": {} }, false, { "movie": {} }, false],
        })
    }

    fn zoom_preset(&self) -> &'static str {
        ZOOM_PRESETS
            .iter()
            .find(|(_, _, x)| (*x - self.zoom).abs() < f64::EPSILON)
            .map_or(NO_ZOOM_PRESET, |(id, _, _)| id)
    }

    fn settings(&self) -> Value {
        json!({ "getSettings": { "data": {
            "streams": [{ "id": "C0FFEE00-0000-4000-8000-000000000000", "name": "Main" }],
            "scenes": SCENES.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>(),
            "bitratePresets": PRESETS.iter().map(|(id, bitrate)| json!({ "id": id, "bitrate": bitrate })).collect::<Vec<_>>(),
            "mics": [{ "id": "builtin", "name": "Bottom" }],
            "srt": {
                "connectionPrioritiesEnabled": self.priorities_enabled,
                "connectionPriorities": self.connections.iter().map(|c| json!({
                    "id": c.id, "name": c.name, "priority": c.priority, "enabled": c.enabled,
                })).collect::<Vec<_>>(),
            },
        } } })
    }

    fn status(&self) -> Value {
        let mut top_right = Map::new();
        top_right.insert(
            "systemMonitor".into(),
            json!({ "message": format!("{}%/{}% 312 MB", 14 + self.seconds_live % 7, 38 + self.seconds_live % 11), "ok": true }),
        );
        top_right.insert(
            "moblink".into(),
            json!({ "message": "1 relay, Connected", "ok": true }),
        );
        top_right.insert(
            "djiDevices".into(),
            json!({ "message": "Osmo Action 4: Streaming", "ok": true }),
        );
        top_right.insert(
            "audioLevel".into(),
            json!({ "message": "-20 dB", "ok": true }),
        );
        if self.live {
            let kbps = PRESETS[self.preset].1 / 1000;
            let wobble = self.seconds_live * 37 % 400;
            top_right.insert(
                "bitrate".into(),
                json!({ "message": format!("{:.1} Mbps, {} MB", (kbps + wobble) as f64 / 1000.0, self.seconds_live * kbps / 8000), "ok": true }),
            );
            let secs = self.seconds_live;
            top_right.insert(
                "uptime".into(),
                json!({ "message": format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60), "ok": true }),
            );
            top_right.insert(
                "srtla".into(),
                json!({ "message": "62% Cellular, 38% WiFi", "ok": true }),
            );
            top_right.insert(
                "srtlaRtts".into(),
                json!({ "message": "Cellular 84 ms, WiFi 41 ms", "ok": true }),
            );
        }
        json!({ "getStatus": {
            "general": {
                "batteryCharging": false,
                "batteryLevel": self.battery,
                "flame": if self.seconds_live > 600 { "Yellow" } else { "White" },
                "wiFiSsid": "Not for the dashboard",
                "isLive": self.live,
                "isRecording": self.recording,
                "isMuted": self.muted,
            },
            "topLeft": {
                "mic": { "message": "Bottom", "ok": true },
                "zoom": { "message": format!("{:.1}", self.zoom), "ok": true },
            },
            "topRight": top_right,
        } })
    }
}

fn hash_password(challenge: &str, salt: &str, password: &str) -> String {
    let inner = STANDARD.encode(Sha256::digest(format!("{password}{salt}")));
    STANDARD.encode(Sha256::digest(format!("{inner}{challenge}")))
}

fn state_event(state: Value) -> String {
    json!({ "event": { "data": { "state": { "data": state } } } }).to_string()
}

#[tokio::main]
async fn main() {
    let url = std::env::var("MOBLIN_URL").unwrap_or_else(|_| {
        let id = std::env::args()
            .nth(1)
            .expect("usage: fake_moblin <switcher ID> (or set MOBLIN_URL)");
        format!("ws://localhost:11000/moblin/{id}")
    });
    let password =
        std::env::var("PASSWORD").expect("set PASSWORD to the switcher's Moblin password");

    let phone = Arc::new(Mutex::new(Phone {
        live: false,
        recording: false,
        muted: false,
        scene: 0,
        preset: 2,
        battery: 87,
        seconds_live: 0,
        torch: false,
        stealth_mode: false,
        zoom: 1.0,
        priorities_enabled: false,
        connections: vec![
            Connection {
                id: "C0000000-0000-4000-8000-000000000001",
                name: "Cellular",
                priority: 1,
                enabled: true,
            },
            Connection {
                id: "C0000000-0000-4000-8000-000000000002",
                name: "WiFi",
                priority: 1,
                enabled: true,
            },
            Connection {
                id: "C0000000-0000-4000-8000-000000000003",
                name: "Moblink: iPhone 12",
                priority: 1,
                enabled: true,
            },
        ],
    }));
    // Partial states from the terminal, or `None` to drop the connection.
    let (events, mut events_rx) = mpsc::unbounded_channel::<Option<Value>>();

    {
        let phone = phone.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut phone = phone.lock().await;
                let event = match line.trim() {
                    "live" => {
                        phone.live = true;
                        Some(json!({ "streaming": true }))
                    }
                    "stop" => {
                        phone.live = false;
                        phone.seconds_live = 0;
                        Some(json!({ "streaming": false }))
                    }
                    "mute" | "unmute" => {
                        phone.muted = line.trim() == "mute";
                        Some(json!({ "muted": phone.muted }))
                    }
                    "torch" => {
                        phone.torch = !phone.torch;
                        Some(json!({ "torchOn": phone.torch }))
                    }
                    "scene" => {
                        phone.scene = (phone.scene + 1) % SCENES.len();
                        Some(json!({ "scene": SCENES[phone.scene].0 }))
                    }
                    "drop" => None,
                    other => {
                        println!(
                            "unknown command {other:?}; try live, stop, mute, unmute, scene, torch, drop"
                        );
                        continue;
                    }
                };
                let _ = events.send(event);
            }
            // No terminal (stdin closed): keep the phone connected anyway.
            std::future::pending::<()>().await;
            drop(events);
        });
    }
    {
        let phone = phone.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tick.tick().await;
                let mut phone = phone.lock().await;
                if phone.live {
                    phone.seconds_live += 1;
                    if phone.seconds_live % 60 == 0 {
                        phone.battery = (phone.battery - 1).max(1);
                    }
                }
            }
        });
    }

    loop {
        println!("connecting to {url}");
        match tokio_tungstenite::connect_async(url.as_str()).await {
            Ok((socket, _)) => {
                run(socket, &password, &phone, &mut events_rx).await;
                println!("disconnected");
            }
            Err(error) => println!("couldn't connect: {error}"),
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn run(
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    password: &str,
    phone: &Mutex<Phone>,
    events: &mut mpsc::UnboundedReceiver<Option<Value>>,
) {
    let (mut sink, mut stream) = socket.split();
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    ping.reset();
    let mut identified = false;

    loop {
        tokio::select! {
            message = stream.next() => {
                let Some(Ok(Message::Text(text))) = message else {
                    match message {
                        Some(Ok(_)) => continue,
                        _ => return,
                    }
                };
                let Ok(Value::Object(message)) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let Some((kind, body)) = message.into_iter().next() else {
                    continue;
                };
                match kind.as_str() {
                    "hello" => {
                        let auth = &body["authentication"];
                        let hash = hash_password(
                            auth["challenge"].as_str().unwrap_or_default(),
                            auth["salt"].as_str().unwrap_or_default(),
                            password,
                        );
                        let identify = json!({ "identify": { "streamerId": "FAKE-MOBLIN", "authentication": hash } });
                        if sink.send(Message::Text(identify.to_string().into())).await.is_err() {
                            return;
                        }
                    }
                    "identified" => {
                        if body["result"].get("ok").is_some() {
                            println!("identified");
                            identified = true;
                            let state = phone.lock().await.full_state();
                            if sink.send(Message::Text(state_event(state).into())).await.is_err() {
                                return;
                            }
                        } else {
                            println!("refused: {}", body["result"]);
                        }
                    }
                    "request" if identified => {
                        let id = body["id"].clone();
                        let Some((request, args)) = body["data"].as_object().and_then(|data| data.iter().next()) else {
                            continue;
                        };
                        let (data, event) = {
                            let mut phone = phone.lock().await;
                            match request.as_str() {
                                "getStatus" => (Some(phone.status()), None),
                                "getSettings" => (Some(phone.settings()), None),
                                "setLive" => {
                                    phone.live = args["on"].as_bool().unwrap_or(false);
                                    if !phone.live {
                                        phone.seconds_live = 0;
                                    }
                                    println!("{} by the assistant", if phone.live { "live" } else { "stopped" });
                                    (None, Some(json!({ "streaming": phone.live })))
                                }
                                "setRecord" => {
                                    phone.recording = args["on"].as_bool().unwrap_or(false);
                                    (None, Some(json!({ "recording": phone.recording })))
                                }
                                "setMute" => {
                                    phone.muted = args["on"].as_bool().unwrap_or(false);
                                    (None, Some(json!({ "muted": phone.muted })))
                                }
                                "setScene" => match SCENES.iter().position(|(id, _)| Some(*id) == args["id"].as_str()) {
                                    Some(scene) => {
                                        phone.scene = scene;
                                        (None, Some(json!({ "scene": SCENES[scene].0 })))
                                    }
                                    None => (None, None),
                                },
                                "setBitratePreset" => match PRESETS.iter().position(|(id, _)| Some(*id) == args["id"].as_str()) {
                                    Some(preset) => {
                                        phone.preset = preset;
                                        (None, Some(json!({ "bitrate": PRESETS[preset].0 })))
                                    }
                                    None => (None, None),
                                },
                                "setTorch" => {
                                    phone.torch = args["on"].as_bool().unwrap_or(false);
                                    (None, Some(json!({ "torchOn": phone.torch })))
                                }
                                "setStealthMode" => {
                                    phone.stealth_mode = args["on"].as_bool().unwrap_or(false);
                                    (None, Some(json!({ "stealthMode": phone.stealth_mode })))
                                }
                                "setZoom" => {
                                    // Like a camera from 0.5x to 15x.
                                    phone.zoom = args["x"].as_f64().unwrap_or(1.0).clamp(0.5, 15.0);
                                    (None, Some(json!({ "zoom": phone.zoom, "zoomPreset": phone.zoom_preset() })))
                                }
                                "setZoomPreset" => match ZOOM_PRESETS.iter().find(|(id, _, _)| Some(*id) == args["id"].as_str()) {
                                    Some((id, _, x)) => {
                                        phone.zoom = *x;
                                        (None, Some(json!({ "zoomPreset": id, "zoom": x })))
                                    }
                                    None => (None, None),
                                },
                                "setSrtConnectionPrioritiesEnabled" => {
                                    phone.priorities_enabled = args["enabled"].as_bool().unwrap_or(false);
                                    println!("connection priorities {}", if phone.priorities_enabled { "on" } else { "off" });
                                    (None, None)
                                }
                                "setSrtConnectionPriority" => {
                                    if let Some(connection) = phone.connections.iter_mut().find(|c| Some(c.id) == args["id"].as_str()) {
                                        connection.priority = args["priority"].as_i64().unwrap_or(1).clamp(1, 10);
                                        connection.enabled = args["enabled"].as_bool().unwrap_or(true);
                                        println!("{}: priority {}, {}", connection.name, connection.priority, if connection.enabled { "on" } else { "off" });
                                    }
                                    (None, None)
                                }
                                other => {
                                    println!("request {other} (answered ok)");
                                    (None, None)
                                }
                            }
                        };
                        let mut response = json!({ "response": { "id": id, "result": { "ok": {} } } });
                        if let Some(data) = data {
                            response["response"]["data"] = data;
                        }
                        if sink.send(Message::Text(response.to_string().into())).await.is_err() {
                            return;
                        }
                        if let Some(event) = event
                            && sink.send(Message::Text(state_event(event).into())).await.is_err()
                        {
                            return;
                        }
                    }
                    "pong" => {}
                    _ => {}
                }
            }
            event = events.recv() => match event {
                Some(Some(state)) if identified => {
                    if sink.send(Message::Text(state_event(state).into())).await.is_err() {
                        return;
                    }
                }
                Some(Some(_)) => {}
                Some(None) | None => {
                    let _ = sink.close().await;
                    return;
                }
            },
            _ = ping.tick() => {
                if sink.send(Message::Text(json!({ "ping": {} }).to_string().into())).await.is_err() {
                    return;
                }
            }
        }
    }
}
