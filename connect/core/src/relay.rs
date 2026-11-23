use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::logs::ConnectLogUpload;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::programs::ProgramStatus;
use futures_util::{SinkExt, StreamExt};
use obs_protocol::{GetCurrentProgramScene, OutputStatus, RequestKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

mod cloud;
mod obs;

use cloud::{CloudBody, CommandResult, identify_payload, send_cloud, send_message};
pub use obs::stop_active_streams;
use obs::{PendingCall, RawRequests, connect_obs, decode_response, get_obs_version};

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
    pub record_active: bool,
    pub last_program_scene: Option<String>,
    pub obs_version: Option<String>,
}

pub use common::programs::ConnectCommandAction as ObsAction;

#[derive(Debug)]
pub struct ObsCommandRequest {
    pub action: ObsAction,
    pub reply: oneshot::Sender<Result<Option<ConnectLogUpload>, String>>,
}

#[derive(Debug)]
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
    pub obs_auth_failed: watch::Sender<bool>,
    pub commands: mpsc::UnboundedSender<ObsCommandRequest>,
    pub unsolicited_uploads: mpsc::UnboundedReceiver<ConnectLogUpload>,
    /// Bumped whenever OBS or the dashboard does something user-visible; the app
    /// uses it to decide when the setup has been idle.
    pub activity: Arc<AtomicU64>,
}

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("WebSocket failed: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("WebSocket JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("device token was rejected; pair again")]
    InvalidToken,
    #[error("another connection took over this switcher")]
    TakenOver,
    #[error("expected an OBS websocket Hello")]
    ObsHandshake,
    #[error("OBS websocket closed")]
    ObsClosed,
    #[error("OBS websocket request timed out")]
    ObsTimeout,
    #[error("timed out waiting for OBS output to stop")]
    ObsOutputStopTimeout,
    #[error("OBS rejected {request} (code {code}): {}", comment.as_deref().unwrap_or("no details"))]
    ObsRejected {
        request: String,
        code: u32,
        comment: Option<String>,
    },
    #[error("OBS websocket authentication failed")]
    ObsAuthentication,
    #[error("{0}")]
    UnsafeUrl(String),
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudLinkState {
    Connected,
    #[default]
    Reconnecting,
    TakenOver,
}

pub async fn run_link(
    config: LinkConfig,
    obs_state: watch::Receiver<PcObsState>,
    watchdog: watch::Sender<WatchdogMode>,
) -> Result<(), RelayError> {
    let (programs_tx, programs) = watch::channel(Vec::new());
    let result = run_link_with_programs(config, obs_state, programs, watchdog).await;
    drop(programs_tx);
    result
}

pub async fn run_link_with_programs(
    config: LinkConfig,
    obs_state: watch::Receiver<PcObsState>,
    programs: watch::Receiver<Vec<ProgramStatus>>,
    watchdog: watch::Sender<WatchdogMode>,
) -> Result<(), RelayError> {
    let (link_state, _) = watch::channel(CloudLinkState::Reconnecting);
    run_link_with_programs_and_state(config, obs_state, programs, watchdog, link_state).await
}

pub async fn run_link_with_programs_and_state(
    mut config: LinkConfig,
    mut obs_state: watch::Receiver<PcObsState>,
    mut programs: watch::Receiver<Vec<ProgramStatus>>,
    watchdog: watch::Sender<WatchdogMode>,
    link_state: watch::Sender<CloudLinkState>,
) -> Result<(), RelayError> {
    crate::url_security::validate_link_url(&config.link_url).map_err(RelayError::UnsafeUrl)?;
    let mut backoff = Duration::from_millis(500);
    loop {
        link_state.send_replace(CloudLinkState::Reconnecting);
        if let Err(error @ (RelayError::InvalidToken | RelayError::TakenOver)) = connect_once(
            &mut config,
            &mut obs_state,
            &mut programs,
            &watchdog,
            &link_state,
        )
        .await
        {
            if matches!(error, RelayError::TakenOver) {
                link_state.send_replace(CloudLinkState::TakenOver);
            }
            return Err(error);
        }
        link_state.send_replace(CloudLinkState::Reconnecting);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(10));
    }
}

async fn connect_once(
    config: &mut LinkConfig,
    obs_state: &mut watch::Receiver<PcObsState>,
    programs: &mut watch::Receiver<Vec<ProgramStatus>>,
    watchdog: &watch::Sender<WatchdogMode>,
    link_state: &watch::Sender<CloudLinkState>,
) -> Result<(), RelayError> {
    crate::install_crypto_provider();
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
                config.obs_auth_failed.send_if_modified(clear_auth_failure);
                let _ = config.state_tx.send(PcObsState::Connected);
                config.activity.fetch_add(1, Ordering::Relaxed);
                Some(socket)
            }
            failure => {
                if matches!(failure, Ok(Err(RelayError::ObsAuthentication))) {
                    config.obs_auth_failed.send_if_modified(set_auth_failure);
                }
                let _ = config.state_tx.send(PcObsState::Starting);
                None
            }
        }
    } else {
        None
    };
    let obs_version = if let Some(socket) = obs.as_mut() {
        get_obs_version(socket).await.unwrap_or_default()
    } else {
        None
    };
    let (mut upstream, _) =
        tokio::time::timeout(Duration::from_secs(15), connect_async(&config.link_url))
            .await
            .map_err(|_| link_timeout("upstream connection timed out"))??;
    send_message(
        &mut upstream,
        identify_payload(config, obs_version.as_deref()),
    )
    .await?;
    let current_state = *obs_state.borrow();
    send_pc_state(&mut upstream, current_state).await?;
    let current_programs = programs.borrow().clone();
    send_programs(&mut upstream, current_programs).await?;
    if obs.is_some() {
        send_cloud(&mut upstream, None, CloudBody::ObsWsConnected(true)).await?;
        send_pc_state(&mut upstream, PcObsState::Connected).await?;
    } else if obs_port.is_some() {
        send_cloud(&mut upstream, None, CloudBody::ObsWsConnected(false)).await?;
    }
    let mut pending_samples = if let Some(socket) = obs.as_mut() {
        start_sampling(socket).await?
    } else {
        Vec::new()
    };
    let mut obs_retry = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(2),
        Duration::from_secs(2),
    );
    let mut meter_tick = tokio::time::interval(Duration::from_millis(200));
    let mut stats_tick = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    let mut raw_requests = RawRequests::default();
    let mut command_replies = futures_util::stream::FuturesUnordered::<
        Pin<
            Box<
                dyn Future<
                        Output = (
                            String,
                            Result<
                                Result<Option<ConnectLogUpload>, String>,
                                oneshot::error::RecvError,
                            >,
                        ),
                    > + Send,
            >,
        >,
    >::new();
    let mut pending_meters = HashMap::<String, Vec<Vec<f64>>>::new();
    let mut missed_stats = 0_u8;
    let mut telemetry = ObsTelemetry {
        obs_version: obs_version.clone(),
        ..ObsTelemetry::default()
    };
    let _ = config.telemetry_tx.send(telemetry.clone());
    let mut identified = false;
    let mut request_error_kind = None;
    let mut last_received = tokio::time::Instant::now();
    let mut ping_tick = tokio::time::interval_at(
        last_received + Duration::from_secs(15),
        Duration::from_secs(15),
    );
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(last_received + Duration::from_secs(45)) => {
                return Err(link_timeout("no upstream frames received for 45 seconds"));
            }
            _ = ping_tick.tick() => {
                upstream.send(Message::Ping(Vec::new().into())).await?;
            }
            _ = obs_retry.tick(), if obs.is_none() && obs_port.is_some() => {
                if let Some(port) = obs_port {
                    match tokio::time::timeout(
                        Duration::from_secs(5),
                        connect_obs(port, &obs_password),
                    ).await {
                        Ok(Ok(mut socket)) => {
                            config.obs_auth_failed.send_if_modified(clear_auth_failure);
                            if let Ok(Some(version)) = get_obs_version(&mut socket).await {
                                telemetry.obs_version = Some(version);
                                let _ = config.telemetry_tx.send(telemetry.clone());
                            }
                            pending_samples = start_sampling(&mut socket).await?;
                            obs = Some(socket);
                            missed_stats = 0;
                            send_cloud(&mut upstream, None, CloudBody::ObsWsConnected(true)).await?;
                            let _ = config.state_tx.send(PcObsState::Connected);
                            send_pc_state(&mut upstream, PcObsState::Connected).await?;
                            config.activity.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(Err(RelayError::ObsAuthentication)) => {
                            config.obs_auth_failed.send_if_modified(set_auth_failure);
                        }
                        _ => {}
                    }
                }
            }
            _ = meter_tick.tick() => {
                raw_requests.expire();
                pending_samples.retain(|(sample, call)| *sample == Sample::Stats || !call.expired());
                if let Some(meters) = take_volume_meters(&mut pending_meters) {
                    send_cloud(&mut upstream, None, CloudBody::ObsRawEvent(serde_json::to_value(meters)?)).await?;
                }
            }
            _ = stats_tick.tick(), if obs.is_some() => {
                if let Some(index) = pending_samples.iter().position(|(sample, _)| *sample == Sample::Stats) {
                    if !pending_samples[index].1.expired() {
                        continue;
                    }
                    pending_samples.swap_remove(index);
                    missed_stats = missed_stats.saturating_add(1);
                }
                if missed_stats >= 3 {
                    obs = None;
                    raw_requests.clear();
                    pending_samples.clear();
                    let _ = config.state_tx.send(PcObsState::NotResponding);
                    send_cloud(&mut upstream, None, CloudBody::ObsWsConnected(false)).await?;
                    send_pc_state(&mut upstream, PcObsState::NotResponding).await?;
                    continue;
                }
                if let Some(socket) = obs.as_mut() {
                    pending_samples.push((Sample::Stats, PendingCall::send(socket, RequestKind::GetStats).await?));
                }
            }
            changed = obs_state.changed() => {
                if changed.is_ok() {
                    let current_state = *obs_state.borrow();
                    send_pc_state(&mut upstream, current_state).await?;
                }
            }
            changed = programs.changed() => {
                if changed.is_ok() {
                    let current_programs = programs.borrow().clone();
                    send_programs(&mut upstream, current_programs).await?;
                }
            }
            upload = config.unsolicited_uploads.recv(),
                if !config.unsolicited_uploads.is_closed() =>
            {
                if let Some(upload) = upload {
                    send_cloud(&mut upstream, None, CloudBody::LogUpload(upload)).await?;
                }
            }
            message = upstream.next() => {
                let Some(message) = message else { return Ok(()); };
                let message = message?;
                last_received = tokio::time::Instant::now();
                let Message::Text(text) = message else { continue; };
                let value: Value = serde_json::from_str(&text)?;
                if value.get("identifyAck").and_then(Value::as_bool) == Some(true) {
                    link_state.send_replace(CloudLinkState::Connected);
                }
                if let Some(kind) = value.pointer("/requestError/kind").and_then(Value::as_str) {
                    request_error_kind = Some(kind.to_owned());
                }
                if value.get("identifyAck").or_else(|| value.get("identify_ack")).and_then(Value::as_bool) == Some(false) {
                    return Err(match request_error_kind.as_deref() {
                        Some("ALREADY_CONNECTED") => RelayError::TakenOver,
                        _ => RelayError::InvalidToken,
                    });
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
                    raw_requests.clear();
                    pending_samples.clear();
                    missed_stats = 0;
                    send_cloud(&mut upstream, None, CloudBody::ObsWsConnected(false)).await?;
                    obs_port = Some(port);
                    obs_password = password;
                    config.obs_auth_failed.send_if_modified(clear_auth_failure);
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
                    if let Some(socket) = obs.as_mut()
                        && raw_requests.track(&request, value.get("token").and_then(Value::as_str))
                    {
                        socket.send(Message::Text(request.to_string().into())).await?;
                        if changes_obs(&request) {
                            config.activity.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                if let Some(command) = value.get("connectCommand") {
                    config.activity.fetch_add(1, Ordering::Relaxed);
                    let token = value.get("token").and_then(Value::as_str).map(str::to_owned);
                    let action = serde_json::from_value::<ObsAction>(command.clone()).ok();
                    match (token, action) {
                        (Some(token), Some(action)) => {
                            let (reply, receiver) = oneshot::channel();
                            if config.commands.send(ObsCommandRequest { action, reply }).is_ok() {
                                command_replies.push(Box::pin(async move {
                                    (token, receiver.await)
                                }));
                            } else {
                                send_command_result(&mut upstream, &token, Err(COMMAND_UNAVAILABLE.into())).await?;
                            }
                        }
                        (Some(token), None) => {
                            send_command_result(&mut upstream, &token, Err("Unknown OBS command".into())).await?;
                        }
                        _ => {}
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
            Some((token, result)) = command_replies.next(), if !command_replies.is_empty() => {
                send_command_result(&mut upstream, &token, result.unwrap_or_else(|_| Err(COMMAND_UNAVAILABLE.into()))).await?;
            }
            message = next_obs(&mut obs) => {
                let message = match message {
                    Some(Err(error)) => {
                        tracing::debug!(%error, "Local OBS websocket disconnected");
                        None
                    }
                    other => other.transpose()?,
                };
                let Some(message) = message.filter(|message| !matches!(message, Message::Close(_))) else {
                    obs = None;
                    raw_requests.clear();
                    pending_samples.clear();
                    missed_stats = 0;
                    send_cloud(&mut upstream, None, CloudBody::ObsWsConnected(false)).await?;
                    let _ = config.state_tx.send(PcObsState::NotResponding);
                    send_pc_state(&mut upstream, PcObsState::NotResponding).await?;
                    continue;
                };
                let Message::Text(text) = message else { continue; };
                let value: Value = serde_json::from_str(&text)?;
                let op = value.get("op").and_then(Value::as_u64);
                if matches!(op, Some(7 | 9)) {
                    if let Some(index) = pending_samples.iter().position(|(_, call)| call.matches(&value)) {
                        let (sample, call) = pending_samples.swap_remove(index);
                        match call.complete(&value).and_then(|data| sample.apply(data, &mut telemetry)) {
                            Ok(()) if sample == Sample::Stats => missed_stats = 0,
                            Ok(()) => { let _ = config.telemetry_tx.send(telemetry.clone()); }
                            Err(error) => {
                                if sample == Sample::Stats {
                                    if matches!(error, RelayError::ObsTimeout) {
                                        missed_stats = missed_stats.saturating_add(1);
                                    } else {
                                        missed_stats = 0;
                                    }
                                }
                                tracing::debug!(?sample, %error, "OBS sample failed");
                            }
                        }
                        continue;
                    }
                    if let Some(token) = raw_requests.resolve(&value) {
                        send_cloud(&mut upstream, Some(&token), CloudBody::ObsRawResponse(value)).await?;
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
                    } else if event == "RecordStateChanged"
                        && let Some(active) = value.pointer("/d/eventData/outputActive").and_then(Value::as_bool)
                    {
                        telemetry.record_active = active;
                        let _ = config.telemetry_tx.send(telemetry.clone());
                    }
                    if event == "InputVolumeMeters" {
                        collect_volume_meters(&mut pending_meters, &value);
                        continue;
                    }
                    config.activity.fetch_add(1, Ordering::Relaxed);
                    send_cloud(&mut upstream, None, CloudBody::ObsRawEvent(value)).await?;
                } else if op == Some(2) {
                    send_cloud(&mut upstream, None, CloudBody::ObsWsConnected(true)).await?;
                    let _ = config.state_tx.send(PcObsState::Connected);
                    send_pc_state(&mut upstream, PcObsState::Connected).await?;
                    config.activity.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

fn set_auth_failure(failed: &mut bool) -> bool {
    !std::mem::replace(failed, true)
}

fn clear_auth_failure(failed: &mut bool) -> bool {
    std::mem::replace(failed, false)
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
    send_cloud(socket, None, CloudBody::PcState { obs: state }).await
}

async fn send_programs(
    socket: &mut Socket,
    programs: Vec<ProgramStatus>,
) -> Result<(), RelayError> {
    send_cloud(socket, None, CloudBody::ProgramsState(programs)).await
}

const COMMAND_UNAVAILABLE: &str = "Connect couldn't run the command";

async fn send_command_result(
    socket: &mut Socket,
    token: &str,
    result: Result<Option<ConnectLogUpload>, String>,
) -> Result<(), RelayError> {
    send_cloud(
        socket,
        Some(token),
        CloudBody::ConnectCommandResult(CommandResult::from(result)),
    )
    .await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sample {
    Scene,
    Stream,
    Record,
    Stats,
}

impl Sample {
    fn apply(self, data: Option<Value>, telemetry: &mut ObsTelemetry) -> Result<(), RelayError> {
        match self {
            Self::Scene => {
                let scene: GetCurrentProgramScene = decode_response(data)?;
                telemetry.last_program_scene = Some(scene.current_program_scene_name);
            }
            Self::Stream | Self::Record => {
                let status: OutputStatus = decode_response(data)?;
                if self == Self::Stream {
                    telemetry.stream_active = status.output_active;
                } else {
                    telemetry.record_active = status.output_active;
                }
            }
            Self::Stats => {}
        }
        Ok(())
    }
}

async fn start_sampling(socket: &mut Socket) -> Result<Vec<(Sample, PendingCall)>, RelayError> {
    let mut samples = Vec::with_capacity(4);
    for (sample, kind) in [
        (Sample::Scene, RequestKind::GetCurrentProgramScene),
        (Sample::Stream, RequestKind::GetStreamStatus),
        (Sample::Record, RequestKind::GetRecordStatus),
    ] {
        samples.push((sample, PendingCall::send(socket, kind).await?));
    }
    Ok(samples)
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

/// Whether a forwarded dashboard request does something to OBS, so it counts
/// as activity for the idle timeout. An open dashboard keeps polling `Get*`
/// requests (preview, stats, status) and re-identifies for audio levels; those
/// only read, so they must not keep an unattended OBS awake.
fn changes_obs(request: &Value) -> bool {
    let changes = |request_type: Option<&Value>| {
        request_type
            .and_then(Value::as_str)
            .is_none_or(|request_type| !request_type.starts_with("Get"))
    };
    match request.get("op").and_then(Value::as_u64) {
        // Reidentify
        Some(3) => false,
        Some(6) => changes(request.pointer("/d/requestType")),
        // RequestBatch
        Some(8) => request
            .pointer("/d/requests")
            .and_then(Value::as_array)
            .is_none_or(|requests| {
                requests
                    .iter()
                    .any(|request| changes(request.get("requestType")))
            }),
        _ => true,
    }
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

#[derive(Serialize)]
struct MeterFrame {
    op: u8,
    d: MeterEvent,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MeterEvent {
    event_type: &'static str,
    event_data: MeterInputs,
}

#[derive(Serialize)]
struct MeterInputs {
    inputs: Vec<MeterInput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MeterInput {
    input_name: String,
    input_levels_mul: Vec<Vec<f64>>,
}

fn take_volume_meters(pending: &mut HashMap<String, Vec<Vec<f64>>>) -> Option<MeterFrame> {
    let meters = std::mem::take(pending);
    let inputs = meters
        .into_iter()
        .filter_map(|(input_name, levels)| {
            if !levels.iter().flatten().any(|value| *value > 0.0) {
                return None;
            }
            Some(MeterInput {
                input_name,
                input_levels_mul: levels
                    .into_iter()
                    .map(|channel| {
                        channel
                            .into_iter()
                            .map(round_to_three_significant_digits)
                            .collect()
                    })
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    (!inputs.is_empty()).then_some(MeterFrame {
        op: 5,
        d: MeterEvent {
            event_type: "InputVolumeMeters",
            event_data: MeterInputs { inputs },
        },
    })
}

fn round_to_three_significant_digits(value: f64) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return 0.0;
    }
    let factor = 10_f64.powf(2.0 - value.abs().log10().floor());
    (value * factor).round() / factor
}

#[derive(Debug, PartialEq, Eq)]
pub enum OutputStopOutcome {
    AlreadyInactive,
    Stopped,
}

#[derive(Debug)]
pub struct StopOutputs {
    pub stream: Result<OutputStopOutcome, RelayError>,
    pub recording: Result<OutputStopOutcome, RelayError>,
}

fn hash_base64(value: &[u8]) -> String {
    STANDARD.encode(Sha256::digest(value))
}

fn link_timeout(message: &str) -> RelayError {
    RelayError::WebSocket(tokio_tungstenite::tungstenite::Error::Io(
        std::io::Error::new(std::io::ErrorKind::TimedOut, message),
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    use super::{
        Arc, AtomicU64, ConnectLogUpload, Duration, LinkConfig, OutputStopOutcome, PcObsState,
        RelayError, changes_obs, collect_volume_meters, connect_obs, extract_obs_config,
        get_obs_version, hash_base64, identify_payload, parse_watchdog, run_link,
        stop_active_streams, take_volume_meters,
    };

    #[test]
    fn only_requests_that_change_obs_count_as_activity() {
        let request = |request_type: &str| json!({"op": 6, "d": {"requestType": request_type, "requestId": "1"}});
        let batch = |types: &[&str]| {
            let requests = types
                .iter()
                .map(|request_type| json!({"requestType": request_type}))
                .collect::<Vec<_>>();
            json!({"op": 8, "d": {"requestId": "1", "requests": requests}})
        };

        assert!(!changes_obs(&request("GetSourceScreenshot")));
        assert!(!changes_obs(&request("GetStats")));
        assert!(!changes_obs(
            &json!({"op": 3, "d": {"eventSubscriptions": 65535}})
        ));
        assert!(!changes_obs(&batch(&[
            "GetStreamStatus",
            "GetRecordStatus"
        ])));

        assert!(changes_obs(&request("SetCurrentProgramScene")));
        assert!(changes_obs(&request("StartStream")));
        assert!(changes_obs(&batch(&["GetSceneList", "SetInputMute"])));
        assert!(changes_obs(&json!({"op": 6, "d": {}})));
    }

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
            serde_json::to_value(take_volume_meters(&mut pending).unwrap()).unwrap(),
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
            obs_auth_failed: tokio::sync::watch::channel(false).0,
            commands: tokio::sync::mpsc::unbounded_channel().0,
            unsolicited_uploads: tokio::sync::mpsc::unbounded_channel().1,
            activity: Arc::new(AtomicU64::new(0)),
        };
        let payload = serde_json::to_value(identify_payload(&config, Some("31.0.0"))).unwrap();
        assert_eq!(
            payload,
            json!({
                "token":"device-token",
                "identify":{
                    "token":"device-token",
                    "version":"0.1.0",
                    "client":"connect",
                    "capabilities":["obs_process","obs_control","obs_logs","programs","keepalive"],
                    "os":"linux",
                    "obsVersion":"31.0.0"
                }
            })
        );
        assert_eq!(
            serde_json::to_value(identify_payload(&config, None))
                .unwrap()
                .pointer("/identify/obsVersion"),
            Some(&serde_json::Value::Null)
        );
    }

    fn idle_link_config(port: u16) -> LinkConfig {
        LinkConfig {
            link_url: format!("ws://127.0.0.1:{port}"),
            device_token: "test-device".into(),
            version: "test".into(),
            os: "linux".into(),
            obs_port: None,
            obs_password: String::new(),
            use_dashboard_obs_settings: false,
            state_tx: tokio::sync::watch::channel(PcObsState::Unknown).0,
            telemetry_tx: tokio::sync::watch::channel(super::ObsTelemetry::default()).0,
            effective_obs_tx: tokio::sync::watch::channel(None).0,
            obs_auth_failed: tokio::sync::watch::channel(false).0,
            commands: tokio::sync::mpsc::unbounded_channel().0,
            unsolicited_uploads: tokio::sync::mpsc::unbounded_channel().1,
            activity: Arc::new(AtomicU64::new(0)),
        }
    }

    async fn idle_connection(
        config: &mut LinkConfig,
        link: &tokio::sync::watch::Sender<super::CloudLinkState>,
    ) -> Result<(), super::RelayError> {
        let (_state_tx, mut state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
        let (_programs_tx, mut programs_rx) = tokio::sync::watch::channel(Vec::new());
        let (watchdog, _watchdog_rx) = tokio::sync::watch::channel(super::WatchdogMode::Off);
        super::connect_once(config, &mut state_rx, &mut programs_rx, &watchdog, link).await
    }

    #[tokio::test]
    async fn rejected_local_password_is_reported_without_dropping_the_cloud_link() {
        use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};

        let obs = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cloud = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = idle_link_config(cloud.local_addr().unwrap().port());
        config.obs_port = Some(obs.local_addr().unwrap().port());
        let mut auth_failed = config.obs_auth_failed.subscribe();
        let (link, _) = tokio::sync::watch::channel(super::CloudLinkState::Reconnecting);

        let server = async {
            let mut obs = accept_async(obs.accept().await.unwrap().0).await.unwrap();
            obs.send(Message::text(
                json!({
                    "op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.5.1",
                        "authentication":{"salt":"salt","challenge":"challenge"}}
                })
                .to_string(),
            ))
            .await
            .unwrap();
            assert!(obs.next().await.unwrap().unwrap().is_text());
            obs.close(Some(CloseFrame {
                code: CloseCode::Library(4009),
                reason: "Authentication failed".into(),
            }))
            .await
            .unwrap();

            auth_failed.changed().await.unwrap();
            assert!(*auth_failed.borrow_and_update());
            let mut cloud = accept_async(cloud.accept().await.unwrap().0).await.unwrap();
            assert!(cloud.next().await.unwrap().unwrap().is_text());
            cloud
                .send(Message::text(json!({"identifyAck":false}).to_string()))
                .await
                .unwrap();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(idle_connection(&mut config, &link), server)
        })
        .await
        .unwrap();
        assert!(matches!(result, Err(RelayError::InvalidToken)));
    }

    #[tokio::test]
    async fn upstream_handshake_has_a_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = idle_link_config(listener.local_addr().unwrap().port());
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(20)).await;
        });
        let (link, _) = tokio::sync::watch::channel(super::CloudLinkState::Reconnecting);
        let result =
            tokio::time::timeout(Duration::from_secs(17), idle_connection(&mut config, &link))
                .await
                .unwrap();
        assert!(
            matches!(result, Err(super::RelayError::WebSocket(tokio_tungstenite::tungstenite::Error::Io(error))) if error.kind() == std::io::ErrorKind::TimedOut)
        );
        server.abort();
    }

    #[tokio::test]
    async fn upstream_silence_has_a_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = idle_link_config(listener.local_addr().unwrap().port());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            socket
                .send(Message::Text(
                    json!({"identifyAck":true}).to_string().into(),
                ))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(50)).await;
        });
        let (link, state) = tokio::sync::watch::channel(super::CloudLinkState::Reconnecting);
        let result =
            tokio::time::timeout(Duration::from_secs(48), idle_connection(&mut config, &link))
                .await
                .unwrap();
        assert!(
            matches!(result, Err(super::RelayError::WebSocket(tokio_tungstenite::tungstenite::Error::Io(error))) if error.kind() == std::io::ErrorKind::TimedOut)
        );
        assert_eq!(*state.borrow(), super::CloudLinkState::Connected);
        server.abort();
    }

    #[tokio::test]
    async fn automatic_pongs_keep_an_idle_identified_link_alive() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = idle_link_config(listener.local_addr().unwrap().port());
        let pings = Arc::new(AtomicU64::new(0));
        let received = pings.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            socket
                .send(Message::Text(
                    json!({"identifyAck":true}).to_string().into(),
                ))
                .await
                .unwrap();
            while let Some(Ok(message)) = socket.next().await {
                if matches!(message, Message::Ping(_)) {
                    received.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        });
        let (link, state) = tokio::sync::watch::channel(super::CloudLinkState::Reconnecting);
        assert!(
            tokio::time::timeout(Duration::from_secs(47), idle_connection(&mut config, &link))
                .await
                .is_err()
        );
        assert_eq!(*state.borrow(), super::CloudLinkState::Connected);
        assert_eq!(pings.load(std::sync::atomic::Ordering::Relaxed), 3);
        server.abort();
    }

    #[tokio::test]
    async fn takeover_is_terminal_and_preserves_cloud_state() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = idle_link_config(listener.local_addr().unwrap().port());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            for _ in 0..3 {
                socket.next().await.unwrap().unwrap();
            }
            for frame in [
                json!({"identifyAck":true}),
                json!({"requestError":{"kind":"ALREADY_CONNECTED"}}),
                json!({"identifyAck":false}),
            ] {
                socket
                    .send(Message::Text(frame.to_string().into()))
                    .await
                    .unwrap();
            }
            while let Some(Ok(_)) = socket.next().await {}
        });
        let (_state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
        let (_programs_tx, programs_rx) = tokio::sync::watch::channel(Vec::new());
        let (watchdog, _watchdog_rx) = tokio::sync::watch::channel(super::WatchdogMode::Off);
        let (link, state) = tokio::sync::watch::channel(super::CloudLinkState::Reconnecting);
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            super::run_link_with_programs_and_state(config, state_rx, programs_rx, watchdog, link),
        )
        .await
        .unwrap();
        assert!(matches!(result, Err(super::RelayError::TakenOver)));
        assert_eq!(*state.borrow(), super::CloudLinkState::TakenOver);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn connect_command_is_queued_without_blocking_the_relay() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (frame_tx, frame_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _identify = socket.next().await.unwrap().unwrap();
            let _state = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::Text(
                    json!({"identifyAck":true}).to_string().into(),
                ))
                .await
                .unwrap();
            loop {
                let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
                    continue;
                };
                let frame = serde_json::from_str::<serde_json::Value>(&text).unwrap();
                if frame.get("programsState").is_some() {
                    assert_eq!(frame, json!({"programsState":[]}));
                    break;
                }
            }
            socket
                .send(Message::Text(
                    json!({
                        "token":"command-token",
                        "connectCommand":{"action":"upload_log"}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Text(
                    json!({"settings":{"connect":{"watchdog":"off"}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            loop {
                let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
                    continue;
                };
                let frame = serde_json::from_str::<serde_json::Value>(&text).unwrap();
                if frame.get("connectCommandResult").is_some() {
                    let _ = frame_tx.send(frame);
                    break;
                }
            }
        });

        let (state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
        let (_programs_tx, mut programs_rx) = tokio::sync::watch::channel(Vec::new());
        let (telemetry_tx, _) = tokio::sync::watch::channel(super::ObsTelemetry::default());
        let (watchdog_tx, mut watchdog_rx) =
            tokio::sync::watch::channel(super::WatchdogMode::Restart);
        let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel();
        let config = LinkConfig {
            link_url: format!("ws://127.0.0.1:{port}"),
            device_token: "paired-device-token".into(),
            version: "0.1.0".into(),
            os: "linux".into(),
            obs_port: None,
            obs_password: String::new(),
            use_dashboard_obs_settings: false,
            state_tx,
            telemetry_tx,
            effective_obs_tx: tokio::sync::watch::channel(None).0,
            obs_auth_failed: tokio::sync::watch::channel(false).0,
            commands: commands_tx,
            unsolicited_uploads: tokio::sync::mpsc::unbounded_channel().1,
            activity: Arc::new(AtomicU64::new(0)),
        };
        let relay = tokio::spawn(async move {
            let mut state_rx = state_rx;
            let mut config = config;
            super::connect_once(
                &mut config,
                &mut state_rx,
                &mut programs_rx,
                &watchdog_tx,
                &tokio::sync::watch::channel(super::CloudLinkState::Reconnecting).0,
            )
            .await
        });

        let command = tokio::time::timeout(Duration::from_secs(5), commands_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(command.action, super::ObsAction::UploadLog);
        tokio::time::timeout(Duration::from_secs(5), watchdog_rx.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*watchdog_rx.borrow_and_update(), super::WatchdogMode::Off);
        command
            .reply
            .send(Ok(Some(ConnectLogUpload {
                kind: "obs_log".into(),
                file_name: "latest.txt".into(),
                modified_at: Some("2026-11-04T12:00:00Z".into()),
                truncated: false,
                content_gzip_base64: "H4s=".into(),
            })))
            .unwrap();

        let frame = tokio::time::timeout(Duration::from_secs(5), frame_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            frame,
            json!({
                "token":"command-token",
                "connectCommandResult":{
                    "ok":true,
                    "log":{
                        "kind":"obs_log",
                        "fileName":"latest.txt",
                        "modifiedAt":"2026-11-04T12:00:00Z",
                        "truncated":false,
                        "contentGzipBase64":"H4s="
                    }
                }
            })
        );
        server.await.unwrap();
        relay.abort();
        let _ = relay.await;
    }

    #[tokio::test]
    async fn obs_v5_authentication_uses_the_server_challenge() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket.send(Message::Text(json!({
                "op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.6.0","authentication":{"salt":"salt","challenge":"challenge"}}
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
                    json!({"op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.6.0"}})
                        .to_string()
                        .into(),
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
                    json!({"op":7,"d":{"requestId":request_id,"requestType":"GetVersion","requestStatus":{"code":100,"result":true},"responseData":{"obsVersion":"31.0.0"}}})
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
        assert_raw_request_is_forwarded(RawRelayCase::Single).await;
    }

    #[tokio::test]
    async fn link_forwards_raw_batch_responses_with_the_token() {
        assert_raw_request_is_forwarded(RawRelayCase::Batch).await;
    }

    #[tokio::test]
    async fn link_clears_pending_raw_calls_after_obs_disconnects() {
        assert_raw_request_is_forwarded(RawRelayCase::Reconnect).await;
    }

    #[tokio::test]
    async fn link_returns_stop_result_after_abrupt_obs_disconnect() {
        assert_raw_request_is_forwarded(RawRelayCase::AbruptReconnect).await;
    }

    #[derive(Clone, Copy)]
    enum RawRelayCase {
        Single,
        Batch,
        Reconnect,
        AbruptReconnect,
    }

    async fn assert_raw_request_is_forwarded(case: RawRelayCase) {
        let batch = matches!(case, RawRelayCase::Batch);
        let abrupt = matches!(case, RawRelayCase::AbruptReconnect);
        let reconnect = abrupt || matches!(case, RawRelayCase::Reconnect);
        let obs_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let obs_port = obs_listener.local_addr().unwrap().port();
        let fake_obs = tokio::spawn(async move {
            let (stream, _) = obs_listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket
                .send(Message::Text(
                    json!({"op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.6.0"}})
                        .to_string()
                        .into(),
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
                    json!({"op":7,"d":{"requestId":version_id,"requestType":"GetVersion","requestStatus":{"code":100,"result":true},"responseData":{"obsVersion":"31.0.0"}}})
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
                    json!({"op":7,"d":{"requestId":scene_id,"requestType":"GetCurrentProgramScene","requestStatus":{"code":100,"result":true},"responseData":{"currentProgramSceneName":"Main"}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let Message::Text(stream_request) = socket.next().await.unwrap().unwrap() else {
                panic!("GetStreamStatus request expected");
            };
            let stream_request: serde_json::Value = serde_json::from_str(&stream_request).unwrap();
            assert_eq!(
                stream_request.pointer("/d/requestType").unwrap(),
                "GetStreamStatus"
            );
            let stream_id = stream_request.pointer("/d/requestId").unwrap().clone();
            socket
                .send(Message::Text(
                    json!({"op":7,"d":{"requestId":stream_id,"requestType":"GetStreamStatus","requestStatus":{"code":100,"result":true},"responseData":{"outputActive":true}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            let Message::Text(record_request) = socket.next().await.unwrap().unwrap() else {
                panic!("GetRecordStatus request expected");
            };
            let record_request: serde_json::Value = serde_json::from_str(&record_request).unwrap();
            assert_eq!(
                record_request.pointer("/d/requestType").unwrap(),
                "GetRecordStatus"
            );
            let record_id = record_request.pointer("/d/requestId").unwrap().clone();
            socket
                .send(Message::Text(
                    json!({"op":7,"d":{"requestId":record_id,"requestType":"GetRecordStatus","requestStatus":{"code":100,"result":true},"responseData":{"outputActive":false}}})
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
            let raw_reply = if batch {
                assert_eq!(request["op"], 8);
                json!({"op":9,"d":{"requestId":"request-1","results":[{"requestType":"GetVersion","requestStatus":{"code":100,"result":true},"responseData":{"obsVersion":"31.0.0"}}]}})
            } else {
                json!({"op":7,"d":{"requestId":"request-1","requestStatus":{"result":true},"responseData":{"ok":true}}})
            };
            if reconnect {
                if !abrupt {
                    socket.close(None).await.unwrap();
                }
                drop(socket);
                let (stream, _) = obs_listener.accept().await.unwrap();
                socket = accept_async(stream).await.unwrap();
                socket
                    .send(Message::Text(
                        json!({"op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.6.0"}})
                            .to_string()
                            .into(),
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
                let version: serde_json::Value =
                    serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                        .unwrap();
                socket.send(Message::Text(json!({"op":7,"d":{"requestId":version["d"]["requestId"],"requestType":"GetVersion","requestStatus":{"code":100,"result":true},"responseData":{"obsVersion":"31.0.0"}}}).to_string().into())).await.unwrap();
            }
            socket
                .send(Message::Text(raw_reply.to_string().into()))
                .await
                .unwrap();
            if reconnect {
                socket
                    .send(Message::Text(
                        json!({"op":5,"d":{"eventType":"AfterStaleReply"}})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
            }
            let _ = socket.next().await;
        });

        let link_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let link_port = link_listener.local_addr().unwrap().port();
        let (allow_reply, allow_reply_rx) = tokio::sync::oneshot::channel();
        let fake_link = tokio::spawn(async move {
            let mut allow_reply = Some(allow_reply);
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
            let raw_request = if batch {
                json!({"op":8,"d":{"requestId":"request-1","requests":[{"requestType":"GetVersion"}]}})
            } else {
                json!({"op":6,"d":{"requestType":"GetVersion","requestId":"request-1"}})
            };
            socket
                .send(Message::Text(
                    json!({"identifyAck":true}).to_string().into(),
                ))
                .await
                .unwrap();
            if abrupt {
                socket
                    .send(Message::text(
                        json!({
                            "token":"stop-token", "connectCommand":{"action":"stop"}
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
            }
            socket
                .send(Message::Text(
                    json!({
                        "token":"upstream-request-token",
                        "obsRawRequest":raw_request
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
                if abrupt
                    && response["obsWsConnected"] == false
                    && let Some(allow_reply) = allow_reply.take()
                {
                    allow_reply.send(()).unwrap();
                }
                if abrupt && response["token"] == "stop-token" {
                    assert_eq!(response["connectCommandResult"]["ok"], true);
                    socket
                        .send(Message::text(json!({"identifyAck":false}).to_string()))
                        .await
                        .unwrap();
                    let _ = socket.next().await;
                    break;
                }
                let correlated = response.get("token").and_then(serde_json::Value::as_str)
                    == Some("upstream-request-token");
                let finished = if reconnect {
                    assert!(
                        !correlated,
                        "reconnected OBS must not inherit stale cloud tokens"
                    );
                    response.pointer("/obsRawEvent/d/eventType") == Some(&json!("AfterStaleReply"))
                } else {
                    correlated
                };
                if finished && !abrupt {
                    if batch {
                        assert_eq!(response["obsRawResponse"]["op"], 9);
                        assert_eq!(
                            response.pointer("/obsRawResponse/d/results/0/responseData/obsVersion"),
                            Some(&json!("31.0.0"))
                        );
                    } else if !reconnect {
                        assert_eq!(
                            response.pointer("/obsRawResponse/d/responseData/ok"),
                            Some(&serde_json::Value::Bool(true))
                        );
                    }
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
        let (commands, mut command_rx) =
            tokio::sync::mpsc::unbounded_channel::<super::ObsCommandRequest>();
        let consumer = tokio::spawn(async move {
            if abrupt {
                let command = command_rx.recv().await.unwrap();
                assert_eq!(command.action, super::ObsAction::Stop);
                allow_reply_rx.await.unwrap();
                command.reply.send(Ok(None)).unwrap();
            }
        });
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
                    obs_auth_failed: tokio::sync::watch::channel(false).0,
                    commands,
                    unsolicited_uploads: tokio::sync::mpsc::unbounded_channel().1,
                    activity: Arc::new(AtomicU64::new(0)),
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
        consumer.await.unwrap();
        fake_link.await.unwrap();
        if abrupt {
            fake_obs.abort();
        } else {
            fake_obs.await.unwrap();
        }
    }

    #[tokio::test]
    async fn rejected_stop_stream_is_reported_and_recording_is_still_stopped() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket
                .send(Message::Text(
                    json!({"op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.6.0"}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    socket.next().await.unwrap().unwrap().to_text().unwrap()
                )
                .unwrap(),
                json!({"op":1,"d":{"rpcVersion":1,"eventSubscriptions":64}})
            );
            socket
                .send(Message::Text(
                    json!({"op":2,"d":{"negotiatedRpcVersion":1}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            for (kind, code, comment, data) in [
                (
                    "GetStreamStatus",
                    100,
                    None,
                    Some(json!({"outputActive":true})),
                ),
                (
                    "GetRecordStatus",
                    100,
                    None,
                    Some(json!({"outputActive":true})),
                ),
                ("StopStream", 501, Some("stream refused"), None),
                ("StopRecord", 100, None, None),
            ] {
                let request: serde_json::Value =
                    serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                        .unwrap();
                assert_eq!(request["d"]["requestType"], kind);
                socket.send(Message::Text(json!({
                    "op":7,"d":{"requestId":request["d"]["requestId"],"requestType":kind,
                        "requestStatus":{"code":code,"result":code == 100,"comment":comment},"responseData":data}
                }).to_string().into())).await.unwrap();
            }
            socket.send(Message::Text(json!({
                "op":5,"d":{"eventType":"RecordStateChanged","eventIntent":64,
                    "eventData":{"outputActive":false,"outputState":"OBS_WEBSOCKET_OUTPUT_STOPPED"}}
            }).to_string().into())).await.unwrap();
        });
        let result = tokio::time::timeout(Duration::from_secs(5), stop_active_streams(port, ""))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            result.stream,
            Err(RelayError::ObsRejected { request, code: 501, comment: Some(comment) })
                if request == "StopStream" && comment == "stream refused"
        ));
        assert_eq!(result.recording.unwrap(), OutputStopOutcome::Stopped);
        server.await.unwrap();
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
                obs_auth_failed: tokio::sync::watch::channel(false).0,
                commands: tokio::sync::mpsc::unbounded_channel().0,
                unsolicited_uploads: tokio::sync::mpsc::unbounded_channel().1,
                activity: Arc::new(AtomicU64::new(0)),
            },
            state_rx,
            watchdog_tx,
        )
        .await;
        server.await.unwrap();
        assert!(matches!(result, Err(RelayError::InvalidToken)));
    }

    async fn run_link_with_identify_rejection(kind: &'static str) -> Result<(), RelayError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _identify = socket.next().await.unwrap().unwrap();
            let _state = socket.next().await.unwrap().unwrap();
            socket
                .send(Message::Text(
                    json!({"requestError":{"kind":kind}}).to_string().into(),
                ))
                .await
                .unwrap();
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
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            run_link(
                LinkConfig {
                    link_url: format!("ws://127.0.0.1:{port}"),
                    device_token: "rejected".into(),
                    version: "0.1.0".into(),
                    os: "linux".into(),
                    obs_port: None,
                    obs_password: String::new(),
                    use_dashboard_obs_settings: false,
                    state_tx,
                    telemetry_tx,
                    effective_obs_tx: tokio::sync::watch::channel(None).0,
                    obs_auth_failed: tokio::sync::watch::channel(false).0,
                    commands: tokio::sync::mpsc::unbounded_channel().0,
                    unsolicited_uploads: tokio::sync::mpsc::unbounded_channel().1,
                    activity: Arc::new(AtomicU64::new(0)),
                },
                state_rx,
                watchdog_tx,
            ),
        )
        .await
        .expect("identify rejection should stop reconnecting");
        server.await.unwrap();
        result
    }

    #[tokio::test]
    async fn authentication_failed_identify_ack_is_invalid_token() {
        assert!(matches!(
            run_link_with_identify_rejection("AUTHENTICATION_FAILED").await,
            Err(RelayError::InvalidToken)
        ));
    }

    #[tokio::test]
    async fn already_connected_identify_ack_is_taken_over_without_reconnecting() {
        assert!(matches!(
            run_link_with_identify_rejection("ALREADY_CONNECTED").await,
            Err(RelayError::TakenOver)
        ));
    }

    #[tokio::test]
    async fn connect_watchdog_settings_reach_the_supervisor_channel() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (first_setting_tx, first_setting_rx) = tokio::sync::oneshot::channel();
        let (second_setting_seen_tx, second_setting_seen_rx) = tokio::sync::oneshot::channel();
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
            second_setting_seen_rx.await.unwrap();
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
                obs_auth_failed: tokio::sync::watch::channel(false).0,
                commands: tokio::sync::mpsc::unbounded_channel().0,
                unsolicited_uploads: tokio::sync::mpsc::unbounded_channel().1,
                activity: Arc::new(AtomicU64::new(0)),
            },
            state_rx,
            watchdog_tx,
        ));
        tokio::time::timeout(Duration::from_secs(15), watchdog_rx.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            *watchdog_rx.borrow_and_update(),
            super::WatchdogMode::RestartResume
        );
        first_setting_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(15), watchdog_rx.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*watchdog_rx.borrow_and_update(), super::WatchdogMode::Off);
        second_setting_seen_tx.send(()).unwrap();
        link.abort();
        server.await.unwrap();
    }
}
