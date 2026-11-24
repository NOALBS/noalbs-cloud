use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::logs::ConnectLogUpload;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::programs::ProgramStatus;
use futures_util::{StreamExt, stream::FuturesUnordered};
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
mod local;
mod obs;

use cloud::{CloudBody, CommandResult, identify_payload, send_cloud, send_message};
use local::{LocalObs, RawRequest};
pub use obs::{probe_obs, stop_active_streams};

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
    pub last_identified_attempt: Option<Instant>,
    stream_fresh: bool,
    record_fresh: bool,
}

impl ObsTelemetry {
    pub fn current_outputs_active(&self) -> Option<bool> {
        if (self.stream_fresh && self.stream_active) || (self.record_fresh && self.record_active) {
            Some(true)
        } else if self.stream_fresh && self.record_fresh {
            Some(false)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ObsHealth {
    #[default]
    Waiting,
    Connected,
    Disconnected,
    Disabled,
}

impl ObsHealth {
    pub fn display(self, process: PcObsState) -> PcObsState {
        match (self, process) {
            (Self::Disabled, _) => PcObsState::WebsocketDisabled,
            (Self::Connected, _) => PcObsState::Connected,
            (
                Self::Disconnected,
                PcObsState::Starting | PcObsState::WaitingForInput | PcObsState::Unknown,
            ) => PcObsState::NotResponding,
            _ => process,
        }
    }
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
    pub obs_health: watch::Sender<ObsHealth>,
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
    config: LinkConfig,
    obs_state: watch::Receiver<PcObsState>,
    programs: watch::Receiver<Vec<ProgramStatus>>,
    watchdog: watch::Sender<WatchdogMode>,
    link_state: watch::Sender<CloudLinkState>,
) -> Result<(), RelayError> {
    crate::url_security::validate_link_url(&config.link_url).map_err(RelayError::UnsafeUrl)?;
    crate::install_crypto_provider();
    let local = LocalObs::new(&config);
    let local_process = obs_state.clone();
    tokio::select! {
        result = run_cloud(config, obs_state, programs, watchdog, link_state, &local) => result,
        () = local.run(local_process) => unreachable!("the OBS worker runs for the lifetime of the relay"),
    }
}

const INITIAL_BACKOFF: Duration = Duration::from_millis(500);

fn retry_delay(backoff: &mut Duration, identified: bool) -> Duration {
    if identified {
        *backoff = INITIAL_BACKOFF;
    }
    let delay = *backoff;
    *backoff = (*backoff * 2).min(Duration::from_secs(10));
    delay
}

async fn run_cloud(
    mut config: LinkConfig,
    mut obs_state: watch::Receiver<PcObsState>,
    mut programs: watch::Receiver<Vec<ProgramStatus>>,
    watchdog: watch::Sender<WatchdogMode>,
    link_state: watch::Sender<CloudLinkState>,
    local: &LocalObs,
) -> Result<(), RelayError> {
    let mut backoff = INITIAL_BACKOFF;
    loop {
        link_state.send_replace(CloudLinkState::Reconnecting);
        match connect_once(
            &mut config,
            &mut obs_state,
            &mut programs,
            &watchdog,
            &link_state,
            local,
        )
        .await
        {
            Err(error @ (RelayError::InvalidToken | RelayError::TakenOver)) => {
                if matches!(error, RelayError::TakenOver) {
                    link_state.send_replace(CloudLinkState::TakenOver);
                }
                return Err(error);
            }
            Err(error) => tracing::debug!(%error, "Cloud websocket unavailable"),
            Ok(()) => {}
        }
        let delay = retry_delay(
            &mut backoff,
            *link_state.borrow() == CloudLinkState::Connected,
        );
        link_state.send_replace(CloudLinkState::Reconnecting);
        tokio::time::sleep(delay).await;
    }
}

async fn connect_once(
    config: &mut LinkConfig,
    obs_state: &mut watch::Receiver<PcObsState>,
    programs: &mut watch::Receiver<Vec<ProgramStatus>>,
    watchdog: &watch::Sender<WatchdogMode>,
    link_state: &watch::Sender<CloudLinkState>,
    local: &LocalObs,
) -> Result<(), RelayError> {
    let (mut upstream, _) =
        tokio::time::timeout(Duration::from_secs(15), connect_async(&config.link_url))
            .await
            .map_err(|_| link_timeout("upstream connection timed out"))??;
    let mut sessions = local.session.subscribe();
    let mut health = local.health.subscribe();
    let mut telemetry = local.telemetry.subscribe();
    let mut events = local.events.subscribe();
    let version = telemetry.borrow().obs_version.clone();
    send_message(&mut upstream, identify_payload(config, version.as_deref())).await?;
    let mut published = PublishedState::default();
    let initial = *obs_state.borrow_and_update();
    published
        .send_pc_state(&mut upstream, initial, local, true)
        .await?;
    let current_programs = programs.borrow_and_update().clone();
    send_programs(&mut upstream, current_programs).await?;
    published.send_session(&mut upstream, local).await?;
    let mut command_replies = FuturesUnordered::new();
    let mut raw_replies = FuturesUnordered::new();
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
                cloud::send_frame(&mut upstream, Message::Ping(Vec::new().into())).await?;
            }
            Ok(()) = sessions.changed() => {
                published.send_session(&mut upstream, local).await?;
            }
            Ok(()) = health.changed() => {
                let process = *obs_state.borrow();
                published.send_pc_state(&mut upstream, process, local, false).await?;
            }
            Ok(()) = telemetry.changed() => {
                let process = *obs_state.borrow();
                published.send_pc_state(&mut upstream, process, local, false).await?;
            }
            Ok(()) = obs_state.changed() => {
                let process = *obs_state.borrow_and_update();
                published.send_pc_state(&mut upstream, process, local, false).await?;
            }
            event = events.recv() => {
                match event {
                    Ok(event) if local.session.borrow().as_ref().is_some_and(|session| session.id == event.session) => {
                        published.send_session(&mut upstream, local).await?;
                        send_cloud(&mut upstream, None, CloudBody::ObsRawEvent(event.value)).await?;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!(skipped, "Cloud websocket fell behind local OBS events");
                    }
                    _ => {}
                }
            }
            Some((token, result)) = raw_replies.next() => {
                let token: String = token;
                if let Ok(value) = result {
                    send_cloud(&mut upstream, Some(&token), CloudBody::ObsRawResponse(value)).await?;
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
                if matches!(message, Message::Close(_)) {
                    return Ok(());
                }
                let Message::Text(text) = message else { continue; };
                let value: Value = serde_json::from_str(&text)?;
                let ack = value.get("identifyAck").or_else(|| value.get("identify_ack")).and_then(Value::as_bool);
                if ack == Some(true) {
                    link_state.send_replace(CloudLinkState::Connected);
                }
                if let Some(kind) = value.pointer("/requestError/kind").and_then(Value::as_str) {
                    request_error_kind = Some(kind.to_owned());
                }
                if ack == Some(false) {
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
                    && let Some(settings) = extract_obs_config(&value)
                {
                    local.credentials.send_if_modified(|current| {
                        if current.as_ref() == Some(&settings) {
                            return false;
                        }
                        *current = Some(settings);
                        true
                    });
                }
                if let Some(raw) = value.get("obsRawRequest").or_else(|| value.get("obs_raw_request"))
                    && let Some(session) = local.session.borrow().as_ref()
                {
                    let request = match raw {
                        Value::String(text) => serde_json::from_str::<Value>(text)?,
                        other => other.clone(),
                    };
                    let (reply, receiver) = oneshot::channel();
                    let token = value.get("token").and_then(Value::as_str);
                    if session.requests.try_send(RawRequest {
                        value: request,
                        reply: token.map(|_| reply),
                    }).is_ok() && let Some(token) = token {
                        let token = token.to_owned();
                        raw_replies.push(async move { (token, receiver.await) });
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
                                command_replies.push(async move {
                                    (token, receiver.await)
                                });
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
                if !identified && ack == Some(true) {
                    identified = true;
                    let current_state = *obs_state.borrow();
                    published.send_pc_state(&mut upstream, current_state, local, true).await?;
                }
            }
            Some((token, result)) = command_replies.next(), if !command_replies.is_empty() => {
                send_command_result(&mut upstream, &token, result.unwrap_or_else(|_| Err(COMMAND_UNAVAILABLE.into()))).await?;
            }
        }
    }
}

#[derive(Default)]
struct PublishedState {
    session: Option<uuid::Uuid>,
    pc: Option<(PcObsState, Option<String>)>,
}

impl PublishedState {
    async fn send_session(
        &mut self,
        socket: &mut Socket,
        local: &LocalObs,
    ) -> Result<(), RelayError> {
        let current = local.session.borrow().as_ref().map(|session| session.id);
        if self.session != current {
            if self.session.is_some() {
                send_cloud(socket, None, CloudBody::ObsWsConnected(false)).await?;
            }
            if current.is_some() {
                send_cloud(socket, None, CloudBody::ObsWsConnected(true)).await?;
            }
            self.session = current;
        }
        Ok(())
    }

    async fn send_pc_state(
        &mut self,
        socket: &mut Socket,
        process: PcObsState,
        local: &LocalObs,
        force: bool,
    ) -> Result<(), RelayError> {
        let obs = local.health.borrow().display(process);
        let version = local.telemetry.borrow().obs_version.clone();
        if force || !matches!(&self.pc, Some((state, known)) if obs == *state && version == *known)
        {
            send_cloud(
                socket,
                None,
                CloudBody::PcState {
                    obs,
                    obs_version: version.as_deref(),
                },
            )
            .await?;
            self.pc = Some((obs, version));
        }
        Ok(())
    }
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

    use super::obs::connect_obs;
    use super::{
        Arc, AtomicU64, ConnectLogUpload, Duration, LinkConfig, OutputStopOutcome, PcObsState,
        RelayError, changes_obs, collect_volume_meters, extract_obs_config, hash_base64,
        identify_payload, parse_watchdog, run_link, stop_active_streams, take_volume_meters,
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
            obs_health: tokio::sync::watch::channel(super::ObsHealth::Waiting).0,
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
            obs_health: tokio::sync::watch::channel(super::ObsHealth::Waiting).0,
            telemetry_tx: tokio::sync::watch::channel(super::ObsTelemetry::default()).0,
            effective_obs_tx: tokio::sync::watch::channel(None).0,
            obs_auth_failed: tokio::sync::watch::channel(false).0,
            commands: tokio::sync::mpsc::unbounded_channel().0,
            unsolicited_uploads: tokio::sync::mpsc::unbounded_channel().1,
            activity: Arc::new(AtomicU64::new(0)),
        }
    }

    #[test]
    fn cloud_backoff_resets_after_a_successful_identification() {
        let mut backoff = super::INITIAL_BACKOFF;
        let delay = super::retry_delay(&mut backoff, false);
        assert_eq!(delay, Duration::from_millis(500));
        for _ in 0..10 {
            super::retry_delay(&mut backoff, false);
        }
        assert_eq!(
            super::retry_delay(&mut backoff, false),
            Duration::from_secs(10)
        );
        assert_eq!(
            super::retry_delay(&mut backoff, true),
            Duration::from_millis(500)
        );
        assert_eq!(
            super::retry_delay(&mut backoff, false),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn idle_needs_fresh_output_samples_but_watchdog_keeps_the_last_stream_state() {
        let mut telemetry = super::ObsTelemetry {
            stream_active: true,
            last_program_scene: Some("Main".into()),
            ..Default::default()
        };
        assert!(telemetry.stream_active);
        assert_eq!(telemetry.last_program_scene.as_deref(), Some("Main"));
        assert_eq!(telemetry.current_outputs_active(), None);
        telemetry.record_fresh = true;
        assert_eq!(telemetry.current_outputs_active(), None);
        telemetry.stream_fresh = true;
        assert_eq!(telemetry.current_outputs_active(), Some(true));
        telemetry.stream_active = false;
        assert_eq!(telemetry.current_outputs_active(), Some(false));
    }

    async fn idle_connection(
        config: &mut LinkConfig,
        link: &tokio::sync::watch::Sender<super::CloudLinkState>,
    ) -> Result<(), super::RelayError> {
        let (_state_tx, mut state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
        let (_programs_tx, mut programs_rx) = tokio::sync::watch::channel(Vec::new());
        let (watchdog, _watchdog_rx) = tokio::sync::watch::channel(super::WatchdogMode::Off);
        let local = super::LocalObs::new(config);
        let local_process = state_rx.clone();
        tokio::select! {
            result = super::connect_once(config, &mut state_rx, &mut programs_rx, &watchdog, link, &local) => result,
            () = local.run(local_process) => unreachable!(),
        }
    }

    #[tokio::test]
    async fn obs_disconnect_does_not_mark_the_next_process_unresponsive() {
        use super::ObsHealth;

        let obs = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = idle_link_config(0);
        config.obs_port = Some(obs.local_addr().unwrap().port());
        let mut health = config.obs_health.subscribe();
        let (process, process_rx) = tokio::sync::watch::channel(PcObsState::Starting);
        let worker = tokio::spawn(async move {
            super::LocalObs::new(&config).run(process_rx).await;
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut socket = accept_async(obs.accept().await.unwrap().0).await.unwrap();
            socket
                .send(Message::text(
                    json!({"op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.6.0"}}).to_string(),
                ))
                .await
                .unwrap();
            socket.next().await.unwrap().unwrap();
            socket
                .send(Message::text(
                    json!({"op":2,"d":{"negotiatedRpcVersion":1}}).to_string(),
                ))
                .await
                .unwrap();
            health
                .wait_for(|health| *health == ObsHealth::Connected)
                .await
                .unwrap();
            socket.close(None).await.unwrap();
            health
                .wait_for(|health| *health == ObsHealth::Disconnected)
                .await
                .unwrap();
            let state = *process.borrow();
            assert_eq!(health.borrow().display(state), PcObsState::NotResponding);

            process.send_replace(PcObsState::NotRunning);
            health
                .wait_for(|health| *health == ObsHealth::Waiting)
                .await
                .unwrap();
            process.send_replace(PcObsState::Starting);
            let state = *process.borrow();
            assert_eq!(health.borrow().display(state), PcObsState::Starting);
        })
        .await
        .unwrap();
        worker.abort();
    }

    #[tokio::test]
    async fn rejected_local_password_is_reported_without_dropping_the_cloud_link() {
        use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};

        let obs = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cloud = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = idle_link_config(cloud.local_addr().unwrap().port());
        config.obs_port = Some(obs.local_addr().unwrap().port());
        let mut auth_failed = config.obs_auth_failed.subscribe();
        let mut health = config.obs_health.subscribe();
        let (link, _) = tokio::sync::watch::channel(super::CloudLinkState::Reconnecting);

        let server = async {
            let mut rejected = accept_async(obs.accept().await.unwrap().0).await.unwrap();
            rejected
                .send(Message::text(
                    json!({
                        "op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.5.1",
                            "authentication":{"salt":"salt","challenge":"challenge"}}
                    })
                    .to_string(),
                ))
                .await
                .unwrap();
            assert!(rejected.next().await.unwrap().unwrap().is_text());
            rejected
                .close(Some(CloseFrame {
                    code: CloseCode::Library(4009),
                    reason: "Authentication failed".into(),
                }))
                .await
                .unwrap();

            auth_failed.changed().await.unwrap();
            assert!(*auth_failed.borrow_and_update());
            let mut accepted = accept_async(obs.accept().await.unwrap().0).await.unwrap();
            accepted
                .send(Message::text(
                    json!({"op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.5.1"}}).to_string(),
                ))
                .await
                .unwrap();
            assert!(accepted.next().await.unwrap().unwrap().is_text());
            assert!(*auth_failed.borrow());
            assert_ne!(*health.borrow(), super::ObsHealth::Connected);
            accepted
                .send(Message::text(
                    json!({"op":2,"d":{"negotiatedRpcVersion":1}}).to_string(),
                ))
                .await
                .unwrap();
            health
                .wait_for(|state| *state == super::ObsHealth::Connected)
                .await
                .unwrap();
            assert!(!*auth_failed.borrow());

            let mut cloud = accept_async(cloud.accept().await.unwrap().0).await.unwrap();
            assert!(cloud.next().await.unwrap().unwrap().is_text());
            cloud
                .send(Message::text(json!({"identifyAck":false}).to_string()))
                .await
                .unwrap();
            while cloud.next().await.is_some_and(|message| message.is_ok()) {}
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(idle_connection(&mut config, &link), server)
        })
        .await
        .unwrap();
        assert!(
            matches!(result, Err(RelayError::InvalidToken)),
            "{result:?}"
        );
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

        let (_state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
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
            obs_health: tokio::sync::watch::channel(super::ObsHealth::Waiting).0,
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
            let local = super::LocalObs::new(&config);
            super::connect_once(
                &mut config,
                &mut state_rx,
                &mut programs_rx,
                &watchdog_tx,
                &tokio::sync::watch::channel(super::CloudLinkState::Reconnecting).0,
                &local,
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
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = idle_link_config(upstream.local_addr().unwrap().port());
        config.obs_port = Some(port);
        let (hello, wait_hello) = tokio::sync::oneshot::channel();
        let (version, wait_version) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            wait_hello.await.unwrap();
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
            wait_version.await.unwrap();
            socket
                .send(Message::Text(
                    json!({"op":7,"d":{"requestId":request_id,"requestType":"GetVersion","requestStatus":{"code":100,"result":true},"responseData":{"obsVersion":"31.0.0"}}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            while socket.next().await.is_some_and(|message| message.is_ok()) {}
        });
        let cloud = tokio::spawn(async move {
            let mut socket = accept_async(upstream.accept().await.unwrap().0)
                .await
                .unwrap();
            let identify: serde_json::Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(
                identify.pointer("/identify/obsVersion"),
                Some(&serde_json::Value::Null)
            );
            hello.send(()).unwrap();
            socket
                .send(Message::text(json!({"identifyAck":true}).to_string()))
                .await
                .unwrap();
            let mut version = Some(version);
            loop {
                let frame: serde_json::Value =
                    serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                        .unwrap();
                if frame["obsWsConnected"] == true
                    && let Some(version) = version.take()
                {
                    version.send(()).unwrap();
                }
                if frame["pcState"]["obsVersion"] == "31.0.0" {
                    assert_eq!(frame["pcState"]["obs"], "connected");
                    socket
                        .send(Message::text(json!({"identifyAck":false}).to_string()))
                        .await
                        .unwrap();
                    break;
                }
            }
        });
        let (_state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
        let (watchdog, _) = tokio::sync::watch::channel(super::WatchdogMode::Off);
        let result =
            tokio::time::timeout(Duration::from_secs(3), run_link(config, state_rx, watchdog))
                .await
                .unwrap();
        assert!(matches!(result, Err(RelayError::InvalidToken)));
        cloud.await.unwrap();
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

    #[tokio::test]
    async fn watchdog_retains_the_scene_from_a_raw_scene_list_without_an_event() {
        assert_raw_request_is_forwarded(RawRelayCase::SceneList).await;
    }

    #[tokio::test]
    async fn watchdog_refreshes_the_scene_after_a_successful_scene_switch_without_an_event() {
        assert_raw_request_is_forwarded(RawRelayCase::SceneSwitch).await;
    }

    #[derive(Clone, Copy)]
    enum RawRelayCase {
        Single,
        Batch,
        Reconnect,
        AbruptReconnect,
        SceneList,
        SceneSwitch,
    }

    async fn assert_raw_request_is_forwarded(case: RawRelayCase) {
        let batch = matches!(case, RawRelayCase::Batch);
        let abrupt = matches!(case, RawRelayCase::AbruptReconnect);
        let reconnect = abrupt || matches!(case, RawRelayCase::Reconnect);
        let scene_list = matches!(case, RawRelayCase::SceneList);
        let scene_switch = matches!(case, RawRelayCase::SceneSwitch);
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
            } else if scene_list {
                assert_eq!(request["d"]["requestType"], "GetSceneList");
                json!({"op":7,"d":{"requestId":"request-1","requestType":"GetSceneList","requestStatus":{"code":100,"result":true},"responseData":{"currentProgramSceneName":"BRB","scenes":[]}}})
            } else if scene_switch {
                assert_eq!(request["d"]["requestType"], "SetCurrentProgramScene");
                json!({"op":7,"d":{"requestId":"request-1","requestType":"SetCurrentProgramScene","requestStatus":{"code":100,"result":true}}})
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
            if scene_switch {
                let frame = socket.next().await.unwrap().unwrap();
                let refresh: serde_json::Value =
                    serde_json::from_str(frame.to_text().unwrap()).unwrap();
                assert_eq!(refresh["d"]["requestType"], "GetCurrentProgramScene");
                socket.send(Message::text(json!({"op":7,"d":{"requestId":refresh["d"]["requestId"],"requestType":"GetCurrentProgramScene","requestStatus":{"code":100,"result":true},"responseData":{"currentProgramSceneName":"BRB"}}}).to_string())).await.unwrap();
            }
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
            if scene_list || scene_switch {
                socket.close(None).await.unwrap();
            }
            while socket.next().await.is_some_and(|message| message.is_ok()) {}
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
            assert!(identify.pointer("/identify/obsVersion").is_some());
            let mut connected = false;
            let mut version = false;
            while !(connected && version) {
                let frame: serde_json::Value =
                    serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                        .unwrap();
                connected |= frame["obsWsConnected"] == true;
                if frame["pcState"]["obsVersion"] == "31.0.0" {
                    assert_eq!(frame["pcState"]["obs"], "connected");
                    version = true;
                }
            }
            let raw_request = if batch {
                json!({"op":8,"d":{"requestId":"request-1","requests":[{"requestType":"GetVersion"}]}})
            } else if scene_list {
                json!({"op":6,"d":{"requestType":"GetSceneList","requestId":"request-1"}})
            } else if scene_switch {
                json!({"op":6,"d":{"requestType":"SetCurrentProgramScene","requestId":"request-1","requestData":{"sceneUuid":"87947e62-0ba1-419f-9570-9d7b0f839701"}}})
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
            let mut saw_response = false;
            let mut disconnected = false;
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
                saw_response |= correlated;
                disconnected |= response["obsWsConnected"] == false;
                let finished = if reconnect {
                    assert!(
                        !correlated,
                        "reconnected OBS must not inherit stale cloud tokens"
                    );
                    response.pointer("/obsRawEvent/d/eventType") == Some(&json!("AfterStaleReply"))
                } else if scene_list || scene_switch {
                    saw_response && disconnected
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
                    } else if !reconnect && !scene_list && !scene_switch {
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

        let (_state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
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
                    obs_health: tokio::sync::watch::channel(super::ObsHealth::Waiting).0,
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
            Some(if scene_list || scene_switch {
                "BRB"
            } else {
                "Main"
            })
        );
        assert!(telemetry_rx.borrow().stream_active);
        if reconnect || scene_list || scene_switch {
            assert_eq!(telemetry_rx.borrow().current_outputs_active(), None);
        } else {
            assert_eq!(telemetry_rx.borrow().current_outputs_active(), Some(true));
        }
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
        let (_state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
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
                obs_health: tokio::sync::watch::channel(super::ObsHealth::Waiting).0,
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
        let (_state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
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
                    obs_health: tokio::sync::watch::channel(super::ObsHealth::Waiting).0,
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
        let (_state_tx, state_rx) = tokio::sync::watch::channel(PcObsState::Unknown);
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
                obs_health: tokio::sync::watch::channel(super::ObsHealth::Waiting).0,
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
