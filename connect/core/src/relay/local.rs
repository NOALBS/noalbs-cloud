use std::{
    collections::HashMap,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use obs_protocol::{GetCurrentProgramScene, GetVersion, OutputStatus, RequestKind};
use serde_json::Value;
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    time::{self, MissedTickBehavior},
};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::{
    LinkConfig, ObsHealth, ObsTelemetry, PcObsState, RelayError, Socket, changes_obs,
    collect_volume_meters,
    obs::{PendingCall, REQUEST_TIMEOUT, RawRequests, connect_obs, decode_response},
    take_volume_meters,
};

pub(super) type Credentials = Option<(u16, String)>;

pub(super) struct LocalObs {
    pub(super) credentials: watch::Sender<Credentials>,
    pub(super) events: broadcast::Sender<ObsEvent>,
    pub(super) session: watch::Sender<Option<ObsSession>>,
    pub(super) health: watch::Sender<ObsHealth>,
    pub(super) telemetry: watch::Sender<ObsTelemetry>,
    auth_failed: watch::Sender<bool>,
    effective: watch::Sender<Credentials>,
    activity: Arc<AtomicU64>,
}

#[derive(Clone)]
pub(super) struct ObsSession {
    pub(super) id: Uuid,
    pub(super) requests: mpsc::Sender<RawRequest>,
}

pub(super) struct RawRequest {
    pub(super) value: Value,
    pub(super) reply: Option<oneshot::Sender<Value>>,
}

#[derive(Clone)]
pub(super) struct ObsEvent {
    pub(super) session: Uuid,
    pub(super) value: Value,
}

impl LocalObs {
    pub(super) fn new(config: &LinkConfig) -> Self {
        let initial = if config.use_dashboard_obs_settings {
            None
        } else {
            config
                .obs_port
                .map(|port| (port, config.obs_password.clone()))
        };
        Self {
            credentials: watch::channel(initial).0,
            events: broadcast::channel(256).0,
            session: watch::channel(None).0,
            health: config.obs_health.clone(),
            telemetry: config.telemetry_tx.clone(),
            auth_failed: config.obs_auth_failed.clone(),
            effective: config.effective_obs_tx.clone(),
            activity: config.activity.clone(),
        }
    }

    pub(super) async fn run(&self, process: watch::Receiver<PcObsState>) {
        tokio::join!(
            self.run_socket(&process),
            self.watch_process(process.clone())
        );
    }

    async fn watch_process(&self, mut process: watch::Receiver<PcObsState>) {
        while process.changed().await.is_ok() {
            if matches!(
                *process.borrow_and_update(),
                PcObsState::NotRunning | PcObsState::Crashed
            ) {
                self.health.send_if_modified(|health| {
                    if *health != ObsHealth::Disconnected {
                        return false;
                    }
                    *health = ObsHealth::Waiting;
                    true
                });
            }
        }
    }

    async fn run_socket(&self, process: &watch::Receiver<PcObsState>) {
        let mut credentials = self.credentials.subscribe();
        let mut delay = Duration::from_millis(500);
        loop {
            let settings = credentials.borrow_and_update().clone();
            self.effective.send_if_modified(|current| {
                if *current == settings {
                    return false;
                }
                *current = settings.clone();
                true
            });
            let Some((port, password)) = settings else {
                let _ = credentials.changed().await;
                continue;
            };

            let error = tokio::select! {
                biased;
                _ = credentials.changed() => None,
                result = self.connect_and_serve(port, &password, &mut delay) => Some(result),
            };
            self.telemetry.send_if_modified(|sample| {
                let changed = sample.stream_fresh || sample.record_fresh;
                sample.stream_fresh = false;
                sample.record_fresh = false;
                changed
            });
            let disconnected = self.session.send_replace(None).is_some();
            if let Some(error) = error {
                if disconnected {
                    self.set_health(match *process.borrow() {
                        PcObsState::NotRunning | PcObsState::Crashed => ObsHealth::Waiting,
                        _ => ObsHealth::Disconnected,
                    });
                }
                if matches!(error, RelayError::ObsAuthentication) {
                    self.set_auth_failed(true);
                }
                tracing::debug!(%error, "Local OBS websocket unavailable");
                tokio::select! {
                    _ = credentials.changed() => {},
                    _ = time::sleep(delay) => {
                        delay = (delay * 2).min(Duration::from_secs(10));
                        continue;
                    }
                }
            }
            self.set_health(ObsHealth::Waiting);
            self.set_auth_failed(false);
            delay = Duration::from_millis(500);
        }
    }

    fn set_health(&self, health: ObsHealth) {
        self.health
            .send_if_modified(|current| std::mem::replace(current, health) != health);
    }

    fn set_auth_failed(&self, failed: bool) {
        self.auth_failed
            .send_if_modified(|current| std::mem::replace(current, failed) != failed);
    }

    async fn connect_and_serve(
        &self,
        port: u16,
        password: &str,
        delay: &mut Duration,
    ) -> RelayError {
        match time::timeout(REQUEST_TIMEOUT, connect_obs(port, password)).await {
            Ok(Ok(socket)) => {
                *delay = Duration::from_millis(500);
                self.set_auth_failed(false);
                match self.serve(socket).await {
                    Err(error) => error,
                    Ok(()) => RelayError::ObsClosed,
                }
            }
            Ok(Err(error)) => error,
            Err(_) => RelayError::ObsTimeout,
        }
    }

    async fn serve(&self, mut socket: Socket) -> Result<(), RelayError> {
        let (requests, mut raw_rx) = mpsc::channel::<RawRequest>(256);
        let id = Uuid::new_v4();
        self.session.send_replace(Some(ObsSession { id, requests }));
        self.set_health(ObsHealth::Connected);
        self.activity.fetch_add(1, Ordering::Relaxed);

        let mut samples = Vec::with_capacity(5);
        for (sample, kind) in [
            (Sample::Version, RequestKind::GetVersion),
            (Sample::Scene, RequestKind::GetCurrentProgramScene),
            (Sample::Stream, RequestKind::GetStreamStatus),
            (Sample::Record, RequestKind::GetRecordStatus),
        ] {
            samples.push((sample, PendingCall::send(&mut socket, kind).await?));
        }
        let mut missed_stats = 0_u8;
        let mut stats_tick =
            time::interval_at(time::Instant::now() + REQUEST_TIMEOUT, REQUEST_TIMEOUT);
        stats_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut meter_tick = time::interval(Duration::from_millis(200));
        meter_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut meters = HashMap::new();
        let mut raw = RawRequests::<oneshot::Sender<Value>>::default();
        loop {
            tokio::select! {
                _ = meter_tick.tick() => {
                    raw.expire();
                    samples.retain(|(sample, call)| *sample == Sample::Stats || !call.expired());
                    if let Some(meters) = take_volume_meters(&mut meters) {
                        self.event(id, serde_json::to_value(meters)?);
                    }
                }
                _ = stats_tick.tick() => {
                    if let Some(index) = samples.iter().position(|(sample, _)| *sample == Sample::Stats) {
                        if !samples[index].1.expired() {
                            continue;
                        }
                        samples.swap_remove(index);
                        missed_stats += 1;
                    }
                    if missed_stats >= 3 {
                        return Err(RelayError::ObsTimeout);
                    }
                    samples.push((Sample::Stats, PendingCall::send(&mut socket, RequestKind::GetStats).await?));
                }
                Some(request) = raw_rx.recv() => {
                    if request.reply.as_ref().is_some_and(oneshot::Sender::is_closed) {
                        continue;
                    }
                    if raw.track(&request.value, request.reply) {
                        time::timeout(REQUEST_TIMEOUT, socket.send(Message::text(request.value.to_string())))
                            .await.map_err(|_| RelayError::ObsTimeout)??;
                        if changes_obs(&request.value) {
                            self.activity.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                message = socket.next() => {
                    let text = match message.ok_or(RelayError::ObsClosed)?? {
                        Message::Text(text) => text,
                        Message::Close(_) => return Err(RelayError::ObsClosed),
                        _ => continue,
                    };
                    let value: Value = serde_json::from_str(&text)?;
                    match value.get("op").and_then(Value::as_u64) {
                        Some(7 | 9) => {
                            if let Some(index) = samples.iter().position(|(_, call)| call.matches(&value)) {
                                let (sample, call) = samples.swap_remove(index);
                                let result = call.complete(&value).and_then(|data| sample.apply(data, &self.telemetry));
                                if sample == Sample::Stats {
                                    if matches!(result, Err(RelayError::ObsTimeout)) {
                                        missed_stats += 1;
                                        if missed_stats >= 3 {
                                            return Err(RelayError::ObsTimeout);
                                        }
                                    } else {
                                        missed_stats = 0;
                                    }
                                }
                                if let Err(error) = result {
                                    tracing::debug!(?sample, %error, "OBS sample failed");
                                }
                            } else if let Some(reply) = raw.resolve(&value) {
                                let _ = reply.send(value);
                            }
                        }
                        Some(5) => {
                            match value.pointer("/d/eventType").and_then(Value::as_str) {
                                Some("InputSettingsChanged") => continue,
                                Some("InputVolumeMeters") => {
                                    collect_volume_meters(&mut meters, &value);
                                    continue;
                                }
                                Some("StreamStateChanged") => self.output_event(&value, Sample::Stream),
                                Some("RecordStateChanged") => self.output_event(&value, Sample::Record),
                                Some("CurrentProgramSceneChanged") => {
                                    if let Some(scene) = value.pointer("/d/eventData/sceneName").and_then(Value::as_str) {
                                        self.telemetry.send_modify(|telemetry| telemetry.last_program_scene = Some(scene.to_owned()));
                                    }
                                }
                                _ => {}
                            }
                            self.activity.fetch_add(1, Ordering::Relaxed);
                            self.event(id, value);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn event(&self, session: Uuid, value: Value) {
        let _ = self.events.send(ObsEvent { session, value });
    }

    fn output_event(&self, value: &Value, sample: Sample) {
        if let Some(active) = value
            .pointer("/d/eventData/outputActive")
            .and_then(Value::as_bool)
        {
            self.telemetry
                .send_modify(|telemetry| sample.set_output(telemetry, active));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sample {
    Version,
    Scene,
    Stream,
    Record,
    Stats,
}

impl Sample {
    fn apply(
        self,
        data: Option<Value>,
        telemetry: &watch::Sender<ObsTelemetry>,
    ) -> Result<(), RelayError> {
        match self {
            Self::Version => {
                let version: GetVersion = decode_response(data)?;
                telemetry.send_if_modified(|current| {
                    if current.obs_version.as_ref() == Some(&version.obs_version) {
                        return false;
                    }
                    current.obs_version = Some(version.obs_version);
                    true
                });
            }
            Self::Scene => {
                let scene: GetCurrentProgramScene = decode_response(data)?;
                telemetry.send_modify(|current| {
                    current.last_program_scene = Some(scene.current_program_scene_name)
                });
            }
            Self::Stream | Self::Record => {
                let status: OutputStatus = decode_response(data)?;
                telemetry.send_modify(|current| self.set_output(current, status.output_active));
            }
            Self::Stats => {}
        }
        Ok(())
    }

    fn set_output(self, telemetry: &mut ObsTelemetry, active: bool) {
        if self == Self::Stream {
            telemetry.stream_active = active;
            telemetry.stream_fresh = true;
        } else {
            telemetry.record_active = active;
            telemetry.record_fresh = true;
        }
    }
}
