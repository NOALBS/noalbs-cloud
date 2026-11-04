use std::{collections::HashMap, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{net::TcpStream, sync::watch};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PcObsState {
    #[default]
    Unknown,
    NotRunning,
    Starting,
    Connected,
    NotResponding,
    WaitingForInput,
    SafeMode,
    Crashed,
    WebsocketDisabled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogMode {
    Off,
    #[default]
    Restart,
    RestartResume,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObsTelemetry {
    pub stream_active: bool,
    pub last_program_scene: Option<String>,
    pub obs_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinkConfig {
    pub link_url: String,
    pub device_token: String,
    pub version: String,
    pub os: String,
    pub obs_port: Option<u16>,
    pub obs_password: String,
    pub use_dashboard_obs_settings: bool,
    pub state_tx: watch::Sender<PcObsState>,
    pub telemetry_tx: watch::Sender<ObsTelemetry>,
    pub effective_obs_tx: watch::Sender<Option<(u16, String)>>,
}

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("WebSocket failed: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("WebSocket JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("device token was rejected; pair again")]
    InvalidToken,
    #[error("OBS websocket is not configured")]
    NoObsConfig,
    #[error("OBS websocket authentication failed")]
    ObsAuthentication,
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub async fn run_link(
    config: LinkConfig,
    mut obs_state: watch::Receiver<PcObsState>,
    watchdog: watch::Sender<WatchdogMode>,
) -> Result<(), RelayError> {
    let mut backoff = Duration::from_millis(500);
    loop {
        if let Err(RelayError::InvalidToken) =
            connect_once(&config, &mut obs_state, &watchdog).await
        {
            return Err(RelayError::InvalidToken);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(10));
    }
}

async fn connect_once(
    config: &LinkConfig,
    obs_state: &mut watch::Receiver<PcObsState>,
    watchdog: &watch::Sender<WatchdogMode>,
) -> Result<(), RelayError> {
    let mut obs_port = if config.use_dashboard_obs_settings {
        None
    } else {
        config.obs_port
    };
    let mut obs_password = if config.use_dashboard_obs_settings {
        String::new()
    } else {
        config.obs_password.clone()
    };
    if !config.use_dashboard_obs_settings {
        let _ = config
            .effective_obs_tx
            .send(obs_port.map(|port| (port, obs_password.clone())));
    }
    let mut obs = if let Some(port) = obs_port {
        match tokio::time::timeout(Duration::from_secs(5), connect_obs(port, &obs_password)).await {
            Ok(Ok(socket)) => {
                let _ = config.state_tx.send(PcObsState::Connected);
                Some(socket)
            }
            Ok(Err(_)) | Err(_) => {
                let _ = config.state_tx.send(PcObsState::Starting);
                None
            }
        }
    } else {
        None
    };
    let obs_version = if let Some(socket) = obs.as_mut() {
        tokio::time::timeout(Duration::from_secs(5), get_obs_version(socket))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
    } else {
        None
    };
    let mut pending_scene_request = if let Some(socket) = obs.as_mut() {
        Some(request_current_program_scene(socket).await?)
    } else {
        None
    };
    let mut pending_stream_status = if let Some(socket) = obs.as_mut() {
        Some(request_stream_status(socket).await?)
    } else {
        None
    };
    let (mut upstream, _) = connect_async(&config.link_url).await?;
    upstream
        .send(Message::Text(
            identify_payload(config, obs_version.as_deref())
                .to_string()
                .into(),
        ))
        .await?;
    let current_state = *obs_state.borrow();
    send_pc_state(&mut upstream, current_state).await?;
    if obs.is_some() {
        upstream
            .send(Message::Text(
                json!({"obsWsConnected": true}).to_string().into(),
            ))
            .await?;
        send_pc_state(&mut upstream, PcObsState::Connected).await?;
    } else if obs_port.is_some() {
        upstream
            .send(Message::Text(
                json!({"obsWsConnected": false}).to_string().into(),
            ))
            .await?;
    }
    let mut obs_retry = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(2),
        Duration::from_secs(2),
    );
    let mut meter_tick = tokio::time::interval(Duration::from_millis(200));
    let mut stats_tick = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    let mut request_tokens = HashMap::<String, String>::new();
    let mut pending_meters = HashMap::<String, Vec<Vec<f64>>>::new();
    let mut pending_stats: Option<(String, tokio::time::Instant)> = None;
    let mut missed_stats = 0_u8;
    let mut telemetry = ObsTelemetry {
        obs_version: obs_version.clone(),
        ..ObsTelemetry::default()
    };
    let _ = config.telemetry_tx.send(telemetry.clone());
    let mut identified = false;
    loop {
        tokio::select! {
            _ = obs_retry.tick(), if obs.is_none() && obs_port.is_some() => {
                if let Some(port) = obs_port
                    && let Ok(Ok(socket)) = tokio::time::timeout(
                        Duration::from_secs(5),
                        connect_obs(port, &obs_password),
                    ).await
                {
                    obs = Some(socket);
                    pending_scene_request = if let Some(socket) = obs.as_mut() {
                        if let Some(version) = tokio::time::timeout(
                            Duration::from_secs(5),
                            get_obs_version(socket),
                        )
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .flatten()
                        {
                            telemetry.obs_version = Some(version);
                            let _ = config.telemetry_tx.send(telemetry.clone());
                        }
                        Some(request_current_program_scene(socket).await?)
                    } else {
                        None
                    };
                    pending_stream_status = if let Some(socket) = obs.as_mut() {
                        Some(request_stream_status(socket).await?)
                    } else {
                        None
                    };
                    pending_stats = None;
                    missed_stats = 0;
                    upstream.send(Message::Text(json!({"obsWsConnected": true}).to_string().into())).await?;
                    let _ = config.state_tx.send(PcObsState::Connected);
                    send_pc_state(&mut upstream, PcObsState::Connected).await?;
                }
            }
            _ = meter_tick.tick() => {
                if let Some(meters) = take_volume_meters(&mut pending_meters) {
                    upstream.send(Message::Text(json!({"obsRawEvent": meters}).to_string().into())).await?;
                }
            }
            _ = stats_tick.tick(), if obs.is_some() => {
                if pending_stats
                    .as_ref()
                    .is_some_and(|(_, deadline)| *deadline <= tokio::time::Instant::now())
                {
                    pending_stats = None;
                    missed_stats = missed_stats.saturating_add(1);
                    if missed_stats >= 3 {
                        obs = None;
                        request_tokens.clear();
                        pending_scene_request = None;
                        pending_stream_status = None;
                        pending_stats = None;
                        let _ = config.state_tx.send(PcObsState::NotResponding);
                        upstream.send(Message::Text(json!({"obsWsConnected": false}).to_string().into())).await?;
                        send_pc_state(&mut upstream, PcObsState::NotResponding).await?;
                        continue;
                    }
                }
                if pending_stats.is_none()
                    && let Some(socket) = obs.as_mut()
                {
                    let request_id = format!("noalbs-connect-stats-{}", uuid::Uuid::new_v4());
                    socket.send(Message::Text(json!({
                        "op": 6,
                        "d": {"requestType": "GetStats", "requestId": request_id}
                    }).to_string().into())).await?;
                    pending_stats = Some((
                        request_id,
                        tokio::time::Instant::now() + Duration::from_secs(5),
                    ));
                }
            }
            changed = obs_state.changed() => {
                if changed.is_ok() {
                    let current_state = *obs_state.borrow();
                    send_pc_state(&mut upstream, current_state).await?;
                }
            }
            message = upstream.next() => {
                let Some(message) = message else { return Ok(()); };
                let message = message?;
                let Message::Text(text) = message else { continue; };
                let value: Value = serde_json::from_str(&text)?;
                if value.get("identifyAck").or_else(|| value.get("identify_ack")).and_then(Value::as_bool) == Some(false) {
                    return Err(RelayError::InvalidToken);
                }
                if let Some(mode) = value.get("settings").and_then(|v| v.get("connect"))
                    .and_then(|v| v.get("watchdog")).and_then(Value::as_str)
                    .and_then(parse_watchdog)
                {
                    let _ = watchdog.send(mode);
                }
                if config.use_dashboard_obs_settings
                    && let Some((port, password)) = extract_obs_config(&value)
                    && (obs_port != Some(port) || obs_password != password)
                {
                    obs = None;
                    request_tokens.clear();
                    pending_scene_request = None;
                    pending_stream_status = None;
                    pending_stats = None;
                    missed_stats = 0;
                    upstream.send(Message::Text(json!({"obsWsConnected": false}).to_string().into())).await?;
                    obs_port = Some(port);
                    obs_password = password;
                    let _ = config
                        .effective_obs_tx
                        .send(Some((port, obs_password.clone())));
                    let _ = config.state_tx.send(PcObsState::Starting);
                    send_pc_state(&mut upstream, PcObsState::Starting).await?;
                }
                if let Some(raw) = value.get("obsRawRequest").or_else(|| value.get("obs_raw_request")) {
                    let request = match raw {
                        Value::String(text) => serde_json::from_str::<Value>(text)?,
                        other => other.clone(),
                    };
                    let request_id = extract_request_id(&request);
                    if let (Some(id), Some(token)) = (request_id, value.get("token").and_then(Value::as_str)) {
                        request_tokens.insert(id.to_owned(), token.to_owned());
                    }
                    if let Some(socket) = obs.as_mut() {
                        socket.send(Message::Text(request.to_string().into())).await?;
                    }
                }
                if !identified
                    && value
                        .get("identifyAck")
                        .or_else(|| value.get("identify_ack"))
                        .and_then(Value::as_bool)
                        == Some(true)
                {
                    identified = true;
                    let current_state = *obs_state.borrow();
                    send_pc_state(&mut upstream, current_state).await?;
                }
            }
            message = next_obs(&mut obs) => {
                let Some(message) = message else {
                    obs = None;
                    request_tokens.clear();
                    pending_scene_request = None;
                    pending_stream_status = None;
                    pending_stats = None;
                    missed_stats = 0;
                    upstream.send(Message::Text(json!({"obsWsConnected": false}).to_string().into())).await?;
                    let _ = config.state_tx.send(PcObsState::NotResponding);
                    send_pc_state(&mut upstream, PcObsState::NotResponding).await?;
                    continue;
                };
                let message = message?;
                let Message::Text(text) = message else { continue; };
                let value: Value = serde_json::from_str(&text)?;
                let op = value.get("op").and_then(Value::as_u64);
                if op == Some(7) {
                    let id = extract_request_id(&value);
                    if id.is_some_and(|id| pending_scene_request.as_deref() == Some(id)) {
                        pending_scene_request = None;
                        if let Some(scene) = value
                            .pointer("/d/responseData/currentProgramSceneName")
                            .and_then(Value::as_str)
                        {
                            telemetry.last_program_scene = Some(scene.to_owned());
                            let _ = config.telemetry_tx.send(telemetry.clone());
                        }
                        continue;
                    }
                    if id.is_some_and(|id| pending_stream_status.as_deref() == Some(id)) {
                        pending_stream_status = None;
                        if let Some(active) = value
                            .pointer("/d/responseData/outputActive")
                            .and_then(Value::as_bool)
                        {
                            telemetry.stream_active = active;
                            let _ = config.telemetry_tx.send(telemetry.clone());
                        }
                        continue;
                    }
                    if id.is_some_and(|id| {
                        pending_stats
                            .as_ref()
                            .is_some_and(|(pending_id, _)| pending_id == id)
                    }) {
                        pending_stats = None;
                        missed_stats = 0;
                        continue;
                    }
                    if let Some(token) = id.and_then(|id| request_tokens.remove(id)) {
                        upstream.send(Message::Text(json!({"token": token, "obsRawResponse": value}).to_string().into())).await?;
                    }
                } else if op == Some(5) {
                    let event = value.pointer("/d/eventType").and_then(Value::as_str).unwrap_or_default();
                    if event == "InputSettingsChanged" { continue; }
                    if event == "StreamStateChanged"
                        && let Some(active) = value.pointer("/d/eventData/outputActive").and_then(Value::as_bool)
                    {
                        telemetry.stream_active = active;
                        let _ = config.telemetry_tx.send(telemetry.clone());
                    } else if event == "CurrentProgramSceneChanged"
                        && let Some(scene) = value.pointer("/d/eventData/sceneName").and_then(Value::as_str)
                    {
                        telemetry.last_program_scene = Some(scene.to_owned());
                        let _ = config.telemetry_tx.send(telemetry.clone());
                    }
                    if event == "InputVolumeMeters" {
                        collect_volume_meters(&mut pending_meters, &value);
                        continue;
                    }
                    upstream.send(Message::Text(json!({"obsRawEvent": value}).to_string().into())).await?;
                } else if op == Some(2) {
                    upstream.send(Message::Text(json!({"obsWsConnected": true}).to_string().into())).await?;
                    let _ = config.state_tx.send(PcObsState::Connected);
                    send_pc_state(&mut upstream, PcObsState::Connected).await?;
                }
            }
        }
    }
}

async fn next_obs(
    obs: &mut Option<Socket>,
) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
    match obs {
        Some(socket) => socket.next().await,
        None => std::future::pending().await,
    }
}

async fn send_pc_state(socket: &mut Socket, state: PcObsState) -> Result<(), RelayError> {
    socket
        .send(Message::Text(
            json!({"pcState": {"obs": state}}).to_string().into(),
        ))
        .await?;
    Ok(())
}

fn parse_watchdog(value: &str) -> Option<WatchdogMode> {
    match value {
        "off" => Some(WatchdogMode::Off),
        "restart" => Some(WatchdogMode::Restart),
        "restart_resume" => Some(WatchdogMode::RestartResume),
        _ => None,
    }
}

fn extract_obs_config(value: &Value) -> Option<(u16, String)> {
    let settings = value
        .get("settings")
        .or_else(|| value.pointer("/d/settings"))?;
    let obs = settings.get("obs").or_else(|| value.pointer("/d/obs"))?;
    let port = obs
        .get("port")
        .or_else(|| obs.get("obsPort"))
        .or_else(|| obs.get("wsPort"))
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())?;
    let password = obs
        .get("password")
        .or_else(|| obs.get("obsPassword"))
        .or_else(|| obs.get("obs_password"))
        .or_else(|| obs.get("wsPassword"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Some((port, password))
}

fn extract_request_id(value: &Value) -> Option<&str> {
    value
        .pointer("/d/requestId")
        .or_else(|| value.pointer("/d/request_id"))
        .or_else(|| value.get("requestId"))
        .or_else(|| value.get("request_id"))
        .and_then(Value::as_str)
}

fn collect_volume_meters(pending: &mut HashMap<String, Vec<Vec<f64>>>, event: &Value) {
    let Some(inputs) = event
        .pointer("/d/eventData/inputs")
        .and_then(Value::as_array)
    else {
        return;
    };
    for input in inputs {
        let Some(name) = input.get("inputName").and_then(Value::as_str) else {
            continue;
        };
        let Some(channels) = input.get("inputLevelsMul").and_then(Value::as_array) else {
            continue;
        };
        let merged = pending.entry(name.to_owned()).or_default();
        for (channel_index, channel) in channels.iter().enumerate() {
            let Some(levels) = channel.as_array() else {
                continue;
            };
            if merged.len() <= channel_index {
                merged.resize_with(channel_index + 1, Vec::new);
            }
            for (level_index, value) in levels.iter().enumerate() {
                let Some(value) = value.as_f64().filter(|value| value.is_finite()) else {
                    continue;
                };
                if merged[channel_index].len() <= level_index {
                    merged[channel_index].resize(level_index + 1, 0.0);
                }
                merged[channel_index][level_index] = merged[channel_index][level_index].max(value);
            }
        }
    }
}

fn take_volume_meters(pending: &mut HashMap<String, Vec<Vec<f64>>>) -> Option<Value> {
    let meters = std::mem::take(pending);
    let inputs = meters
        .into_iter()
        .filter_map(|(input_name, levels)| {
            if !levels.iter().flatten().any(|value| *value > 0.0) {
                return None;
            }
            Some(json!({
                "inputName": input_name,
                "inputLevelsMul": levels.into_iter().map(|channel| channel.into_iter().map(round_to_three_significant_digits).collect::<Vec<_>>()).collect::<Vec<_>>()
            }))
        })
        .collect::<Vec<_>>();
    (!inputs.is_empty()).then(|| {
        json!({
            "op": 5,
            "d": {
                "eventType": "InputVolumeMeters",
                "eventData": { "inputs": inputs }
            }
        })
    })
}

fn round_to_three_significant_digits(value: f64) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return 0.0;
    }
    let factor = 10_f64.powf(2.0 - value.abs().log10().floor());
    (value * factor).round() / factor
}

async fn connect_obs(port: u16, password: &str) -> Result<Socket, RelayError> {
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{port}")).await?;
    let hello = read_json(&mut socket).await?;
    if let Some(auth) = hello.pointer("/d/authentication") {
        let salt = auth.get("salt").and_then(Value::as_str).unwrap_or_default();
        let challenge = auth
            .get("challenge")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let secret = hash_base64(format!("{password}{salt}").as_bytes());
        let authentication = hash_base64(format!("{secret}{challenge}").as_bytes());
        socket
            .send(Message::Text(
                json!({"op":1,"d":{"rpcVersion":1,"authentication":authentication}})
                    .to_string()
                    .into(),
            ))
            .await?;
    } else {
        socket
            .send(Message::Text(
                json!({"op":1,"d":{"rpcVersion":1}}).to_string().into(),
            ))
            .await?;
    }
    let identified = read_json(&mut socket).await?;
    if identified.get("op").and_then(Value::as_u64) != Some(2) {
        return Err(RelayError::ObsAuthentication);
    }
    Ok(socket)
}

async fn get_obs_version(socket: &mut Socket) -> Result<Option<String>, RelayError> {
    let request_id = "noalbs-connect-get-version";
    socket
        .send(Message::Text(
            json!({
                "op": 6,
                "d": {
                    "requestType": "GetVersion",
                    "requestId": request_id
                }
            })
            .to_string()
            .into(),
        ))
        .await?;
    loop {
        let response = read_json(socket).await?;
        if response.get("op").and_then(Value::as_u64) == Some(7)
            && extract_request_id(&response) == Some(request_id)
        {
            return Ok(response
                .pointer("/d/responseData/obsVersion")
                .and_then(Value::as_str)
                .map(str::to_owned));
        }
    }
}

async fn request_current_program_scene(socket: &mut Socket) -> Result<String, RelayError> {
    let request_id = format!("noalbs-connect-scene-{}", uuid::Uuid::new_v4());
    socket
        .send(Message::Text(
            json!({
                "op": 6,
                "d": {
                    "requestType": "GetCurrentProgramScene",
                    "requestId": request_id
                }
            })
            .to_string()
            .into(),
        ))
        .await?;
    Ok(request_id)
}

async fn request_stream_status(socket: &mut Socket) -> Result<String, RelayError> {
    let request_id = format!("noalbs-connect-stream-status-{}", uuid::Uuid::new_v4());
    socket
        .send(Message::Text(
            json!({
                "op": 6,
                "d": {
                    "requestType": "GetStreamStatus",
                    "requestId": request_id
                }
            })
            .to_string()
            .into(),
        ))
        .await?;
    Ok(request_id)
}

pub async fn stop_active_streams(port: u16, password: &str) -> Result<(), RelayError> {
    let mut socket = tokio::time::timeout(Duration::from_secs(5), connect_obs(port, password))
        .await
        .map_err(|_| RelayError::NoObsConfig)??;
    for (status_request, stop_request) in [
        ("GetStreamStatus", "StopStream"),
        ("GetRecordStatus", "StopRecord"),
    ] {
        let status = tokio::time::timeout(
            Duration::from_secs(5),
            request_obs(&mut socket, status_request),
        )
        .await
        .map_err(|_| RelayError::NoObsConfig)??;
        if status
            .pointer("/d/responseData/outputActive")
            .and_then(Value::as_bool)
            == Some(true)
        {
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                request_obs(&mut socket, stop_request),
            )
            .await;
        }
    }
    Ok(())
}

async fn request_obs(socket: &mut Socket, request_type: &str) -> Result<Value, RelayError> {
    let request_id = format!("noalbs-connect-stop-{}", uuid::Uuid::new_v4());
    socket
        .send(Message::Text(
            json!({
                "op": 6,
                "d": {"requestType": request_type, "requestId": request_id}
            })
            .to_string()
            .into(),
        ))
        .await?;
    loop {
        let response = read_json(socket).await?;
        if response.get("op").and_then(Value::as_u64) == Some(7)
            && extract_request_id(&response) == Some(request_id.as_str())
        {
            return Ok(response);
        }
    }
}

async fn read_json(socket: &mut Socket) -> Result<Value, RelayError> {
    loop {
        let message = socket.next().await.ok_or(RelayError::NoObsConfig)??;
        if let Message::Text(text) = message {
            return Ok(serde_json::from_str(&text)?);
        }
    }
}

fn hash_base64(value: &[u8]) -> String {
    STANDARD.encode(Sha256::digest(value))
}

fn identify_payload(config: &LinkConfig, obs_version: Option<&str>) -> Value {
    json!({
        "token": config.device_token,
        "identify": {
            "token": config.device_token,
            "version": config.version,
            "client": "connect",
            "capabilities": ["obs_process"],
            "os": config.os,
            "obsVersion": obs_version
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    use super::{
        Duration, LinkConfig, PcObsState, RelayError, collect_volume_meters, connect_obs,
        extract_obs_config, get_obs_version, hash_base64, identify_payload, parse_watchdog,
        run_link, take_volume_meters,
    };

    #[test]
    fn connect_state_uses_the_live_wire_values() {
        assert_eq!(
            serde_json::to_value(PcObsState::NotResponding).unwrap(),
            json!("not_responding")
        );
        assert_eq!(
            serde_json::to_value(PcObsState::WebsocketDisabled).unwrap(),
            json!("websocket_disabled")
        );
    }

    #[test]
    fn watchdog_values_match_the_server_contract() {
        assert!(parse_watchdog("restart_resume").is_some());
        assert!(parse_watchdog("not-a-mode").is_none());
    }

    #[test]
    fn dashboard_obs_settings_are_read_only_when_present() {
        assert_eq!(
            extract_obs_config(&json!({"settings":{"obs":{"port":4455,"password":"secret"}}})),
            Some((4455, "secret".into()))
        );
        assert_eq!(
            extract_obs_config(&json!({"settings":{"obs":{"port":4455}}})),
            Some((4455, String::new()))
        );
        assert_eq!(
            extract_obs_config(&json!({"settings":{"connect":{}}})),
            None
        );
    }

    #[test]
    fn volume_meters_keep_the_loudest_levels_and_drop_silence() {
        let mut pending = HashMap::new();
        collect_volume_meters(
            &mut pending,
            &json!({"d":{"eventData":{"inputs":[
                {"inputName":"Mic","inputLevelsMul":[[0.12,0.3]]},
                {"inputName":"Silent","inputLevelsMul":[[0.0,0.0]]}
            ]}}}),
        );
        collect_volume_meters(
            &mut pending,
            &json!({"d":{"eventData":{"inputs":[
                {"inputName":"Mic","inputLevelsMul":[[0.9,0.2]]}
            ]}}}),
        );
        assert_eq!(
            take_volume_meters(&mut pending).unwrap(),
            json!({"op":5,"d":{"eventType":"InputVolumeMeters","eventData":{
                "inputs":[{"inputName":"Mic","inputLevelsMul":[[0.9,0.3]]}]
            }}})
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn connect_identify_wire_shape_matches_obs_link() {
        let config = LinkConfig {
            link_url: "wss://app.noalbs.com/link".into(),
            device_token: "device-token".into(),
            version: "0.1.0".into(),
            os: "linux".into(),
            obs_port: None,
            obs_password: String::new(),
            use_dashboard_obs_settings: false,
            state_tx: tokio::sync::watch::channel(PcObsState::Unknown).0,
            telemetry_tx: tokio::sync::watch::channel(super::ObsTelemetry::default()).0,
            effective_obs_tx: tokio::sync::watch::channel(None).0,
        };
        let payload = identify_payload(&config, Some("31.0.0"));
        assert_eq!(
            payload,
            json!({
                "token":"device-token",
                "identify":{
                    "token":"device-token",
                    "version":"0.1.0",
                    "client":"connect",
                    "capabilities":["obs_process"],
                    "os":"linux",
                    "obsVersion":"31.0.0"
                }
            })
        );
        assert_eq!(
            identify_payload(&config, None).pointer("/identify/obsVersion"),
            Some(&serde_json::Value::Null)
        );
    }

    #[tokio::test]
    async fn obs_v5_authentication_uses_the_server_challenge() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket.send(Message::Text(json!({
                "op":0,"d":{"rpcVersion":1,"authentication":{"salt":"salt","challenge":"challenge"}}
            }).to_string().into())).await.unwrap();
            let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
                panic!("identify expected");
            };
            let identify: serde_json::Value = serde_json::from_str(&text).unwrap();
            let secret = hash_base64(b"passwordsalt");
            let auth = hash_base64(format!("{secret}challenge").as_bytes());
            assert_eq!(identify.pointer("/d/authentication").unwrap(), &json!(auth));
            socket
                .send(Message::Text(
                    json!({"op":2,"d":{"negotiatedRpcVersion":1}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
        });
        let _socket = connect_obs(port, "password").await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn obs_version_comes_from_get_version() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket
                .send(Message::Text(
                    json!({"op":0,"d":{"rpcVersion":1}}).to_string().into(),
                ))
                .await
                .unwrap();
            let _identify = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::Text(
                    json!({"op":2,"d":{"negotiatedRpcVersion":1}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let Message::Text(request) = socket.next().await.unwrap().unwrap() else {
                panic!("GetVersion request expected");
            };
            let request: serde_json::Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request.pointer("/d/requestType").unwrap(), "GetVersion");
            let request_id = request.pointer("/d/requestId").unwrap().clone();
            socket
                .send(Message::Text(
                    json!({"op":7,"d":{"requestId":request_id,"responseData":{"obsVersion":"31.0.0"}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
        });
        let mut socket = connect_obs(port, "").await.unwrap();
        assert_eq!(
            get_obs_version(&mut socket).await.unwrap(),
            Some("31.0.0".into())
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn link_forwards_raw_requests_and_responses_with_the_token() {
        let obs_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let obs_port = obs_listener.local_addr().unwrap().port();
        let fake_obs = tokio::spawn(async move {
            let (stream, _) = obs_listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket
                .send(Message::Text(
                    json!({"op":0,"d":{"rpcVersion":1}}).to_string().into(),
                ))
                .await
                .unwrap();
            let _identify = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::Text(
                    json!({"op":2,"d":{"negotiatedRpcVersion":1}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let Message::Text(version_request) = socket.next().await.unwrap().unwrap() else {
                panic!("GetVersion request expected");
            };
            let version_request: serde_json::Value =
                serde_json::from_str(&version_request).unwrap();
            let version_id = version_request.pointer("/d/requestId").unwrap().clone();
            socket
                .send(Message::Text(
                    json!({"op":7,"d":{"requestId":version_id,"responseData":{"obsVersion":"31.0.0"}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let Message::Text(scene_request) = socket.next().await.unwrap().unwrap() else {
                panic!("GetCurrentProgramScene request expected");
            };
            let scene_request: serde_json::Value = serde_json::from_str(&scene_request).unwrap();
            assert_eq!(
                scene_request.pointer("/d/requestType").unwrap(),
                "GetCurrentProgramScene"
            );
            let scene_id = scene_request.pointer("/d/requestId").unwrap().clone();
            socket
                .send(Message::Text(
                    json!({"op":7,"d":{"requestId":scene_id,"responseData":{"currentProgramSceneName":"Main"}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let Message::Text(stream_request) = socket.next().await.unwrap().unwrap() else {
                panic!("GetStreamStatus request expected");
            };
            let stream_request: serde_json::Value =
                serde_json::from_str(&stream_request).unwrap();
            assert_eq!(
                stream_request.pointer("/d/requestType").unwrap(),
                "GetStreamStatus"
            );
            let stream_id = stream_request.pointer("/d/requestId").unwrap().clone();
            socket
                .send(Message::Text(
                    json!({"op":7,"d":{"requestId":stream_id,"responseData":{"outputActive":true}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let Message::Text(request) = socket.next().await.unwrap().unwrap() else {
                panic!("forwarded request expected");
            };
            let request: serde_json::Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request.pointer("/d/requestId").unwrap(), "request-1");
            socket
                .send(Message::Text(
                    json!({"op":7,"d":{"requestId":"request-1","requestStatus":{"result":true},"responseData":{"ok":true}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let _ = socket.next().await;
        });

        let link_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let link_port = link_listener.local_addr().unwrap().port();
        let fake_link = tokio::spawn(async move {
            let (stream, _) = link_listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let Message::Text(identify) = socket.next().await.unwrap().unwrap() else {
                panic!("identify expected");
            };
            let identify: serde_json::Value = serde_json::from_str(&identify).unwrap();
            assert_eq!(identify.pointer("/identify/client").unwrap(), "connect");
            assert_eq!(identify.pointer("/identify/obsVersion").unwrap(), "31.0.0");
            let Message::Text(pc_state) = socket.next().await.unwrap().unwrap() else {
                panic!("pcState expected");
            };
            let pc_state: serde_json::Value = serde_json::from_str(&pc_state).unwrap();
            assert_eq!(pc_state.pointer("/pcState/obs").unwrap(), "connected");
            socket
                .send(Message::Text(
                    json!({"identifyAck":true}).to_string().into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Text(
                    json!({
                        "token":"upstream-request-token",
                        "obsRawRequest":{"op":6,"d":{"requestType":"GetVersion","requestId":"request-1"}}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            loop {
                let Message::Text(response) = socket.next().await.unwrap().unwrap() else {
                    continue;
                };
                let response: serde_json::Value = serde_json::from_str(&response).unwrap();
                if response.get("token").and_then(serde_json::Value::as_str)
                    == Some("upstream-request-token")
                {
                    assert_eq!(
                        response.pointer("/obsRawResponse/d/responseData/ok"),
                        Some(&serde_json::Value::Bool(true))
                    );
                    socket
                        .send(Message::Text(
                            json!({"identifyAck":false}).to_string().into(),
                        ))
                        .await
                        .unwrap();
                    let _ = socket.next().await;
                    break;
                }
            }
        });

        let (state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
        let (telemetry_tx, telemetry_rx) =
            tokio::sync::watch::channel(super::ObsTelemetry::default());
        let (watchdog_tx, _) = tokio::sync::watch::channel(super::WatchdogMode::Restart);
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            run_link(
                LinkConfig {
                    link_url: format!("ws://127.0.0.1:{link_port}"),
                    device_token: "paired-device-token".into(),
                    version: "0.1.0".into(),
                    os: "linux".into(),
                    obs_port: Some(obs_port),
                    obs_password: String::new(),
                    use_dashboard_obs_settings: false,
                    state_tx,
                    telemetry_tx,
                    effective_obs_tx: tokio::sync::watch::channel(None).0,
                },
                state_rx,
                watchdog_tx,
            ),
        )
        .await
        .expect("the link should stop after the rejected identify acknowledgement");
        assert!(matches!(result, Err(RelayError::InvalidToken)));
        assert_eq!(
            telemetry_rx.borrow().last_program_scene.as_deref(),
            Some("Main")
        );
        assert!(telemetry_rx.borrow().stream_active);
        fake_link.await.unwrap();
        fake_obs.await.unwrap();
    }

    #[tokio::test]
    async fn rejected_device_ack_stops_reconnects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _identify = socket.next().await.unwrap().unwrap();
            let _state = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::Text(
                    json!({"identifyAck":false}).to_string().into(),
                ))
                .await
                .unwrap();
        });
        let (state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
        let (telemetry_tx, _) = tokio::sync::watch::channel(super::ObsTelemetry::default());
        let (watchdog_tx, _) = tokio::sync::watch::channel(super::WatchdogMode::Restart);
        let result = run_link(
            LinkConfig {
                link_url: format!("ws://127.0.0.1:{port}"),
                device_token: "invalid".into(),
                version: "0.1.0".into(),
                os: "linux".into(),
                obs_port: None,
                obs_password: String::new(),
                use_dashboard_obs_settings: false,
                state_tx,
                telemetry_tx,
                effective_obs_tx: tokio::sync::watch::channel(None).0,
            },
            state_rx,
            watchdog_tx,
        )
        .await;
        server.await.unwrap();
        assert!(matches!(result, Err(RelayError::InvalidToken)));
    }

    #[tokio::test]
    async fn connect_watchdog_settings_reach_the_supervisor_channel() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (first_setting_tx, first_setting_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _identify = socket.next().await.unwrap().unwrap();
            let _state = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::Text(
                    json!({"identifyAck":true,"settings":{"connect":{"watchdog":"restart_resume"}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            first_setting_rx.await.unwrap();
            socket
                .send(Message::Text(
                    json!({"settings":{"connect":{"watchdog":"off"}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let _ = socket.next().await;
        });
        let (state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
        let (telemetry_tx, _) = tokio::sync::watch::channel(super::ObsTelemetry::default());
        let (effective_obs_tx, _) = tokio::sync::watch::channel(None);
        let (watchdog_tx, mut watchdog_rx) =
            tokio::sync::watch::channel(super::WatchdogMode::Restart);
        let link = tokio::spawn(run_link(
            LinkConfig {
                link_url: format!("ws://127.0.0.1:{port}"),
                device_token: "paired-device-token".into(),
                version: "0.1.0".into(),
                os: "linux".into(),
                obs_port: None,
                obs_password: String::new(),
                use_dashboard_obs_settings: false,
                state_tx,
                telemetry_tx,
                effective_obs_tx,
            },
            state_rx,
            watchdog_tx,
        ));
        tokio::time::timeout(Duration::from_secs(5), watchdog_rx.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            *watchdog_rx.borrow_and_update(),
            super::WatchdogMode::RestartResume
        );
        first_setting_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), watchdog_rx.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*watchdog_rx.borrow_and_update(), super::WatchdogMode::Off);
        link.abort();
        server.await.unwrap();
    }
}
