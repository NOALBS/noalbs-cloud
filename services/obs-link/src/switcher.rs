//! One switcher while its OBS is connected: polls the stream servers, decides
//! which scene should be on air (see [`crate::decide`]), switches OBS, and
//! publishes live status, stats and activity.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_nats::jetstream;
use chrono::Utc;
use db::stream_servers::{self, StreamServer, StreamServerKind};
use live::{
    BelaboxAction, BelaboxCommand, BelaboxRefusal, EventDetail, FeedState, Intent, IntentAction,
    MoblinAction, MoblinCommand, MoblinRefusal, MoblinReply, ObsFrames, SceneRef, ServerSample,
    StatsSample, SwitchReason,
};
use obs_actions::{ObsClient, types::Event};
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::{
    app,
    attribution::{Attribution, SceneChange, SharedAttribution},
    belabox,
    decide::{self, Memory, Observation, Reading, RoleScenes, Stop},
    error::{Error, Result},
    history::{Flush, Recorder},
    status::StatusHandle,
    stream_servers as ss,
};

/// Stats older than this don't count.
const STATS_FRESH_FOR: chrono::TimeDelta = chrono::TimeDelta::seconds(5);
/// After (re)starting, wait this long for every server's first reading before
/// publishing stats or a feed state, so a settings save doesn't flash "offline".
const WARM_UP: Duration = Duration::from_secs(3);
/// Re-read the scene list every this many ticks, in case names changed.
const SCENE_REFRESH_TICKS: u32 = 30;
/// How long starting or stopping Moblin with the stream may take.
const MOBLIN_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The triggers for switching based on stream server stats
#[derive(Debug, Clone)]
pub struct Triggers {
    pub low: Option<u32>,
    pub offline: Option<u32>,
    pub rtt: Option<u32>,
    pub rtt_offline: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchType {
    Normal,
    Low,
    Previous,
    Offline,
}

impl SwitchType {
    fn reading(&self) -> Reading {
        match self {
            Self::Normal => Reading::Live,
            Self::Low => Reading::Low,
            Self::Offline => Reading::Offline,
            Self::Previous => Reading::Previous,
        }
    }
}

fn reading_state(reading: Reading) -> FeedState {
    match reading {
        Reading::Live => FeedState::Live,
        Reading::Low => FeedState::Low,
        Reading::Offline | Reading::Previous => FeedState::Offline,
    }
}

/// What a running switcher needs from its connection.
#[derive(Clone)]
pub struct Deps {
    pub switcher_id: Uuid,
    pub db: db::DbPool,
    pub nats: async_nats::Client,
    pub js: jetstream::Context,
    pub reqwest: reqwest::Client,
    /// See `Config::allow_private_stats_urls`.
    pub allow_private_stats_urls: bool,
    pub status: StatusHandle,
    pub inner_comm: mpsc::UnboundedSender<app::InnerCommMessage>,
    /// Kept by the connection, so it survives switcher restarts.
    pub attribution: SharedAttribution,
    /// The recording's current pause was made by "only record on these scenes", so
    /// only we undo it. Kept by the connection like `attribution`: a settings save
    /// restarts the switcher, which must still be able to undo its own pause.
    pub paused_by_scene_control: Arc<AtomicBool>,
    /// Decrypts stream server secrets (e.g. a MediaMTX password).
    pub data_key: common::crypto::DataKey,
    /// The BELABOX Cloud link, when the switcher has a remote key.
    pub belabox: Option<belabox::Handle>,
}

struct Runtime {
    deps: Deps,
    obs: ObsClient<InnerCommSender>,
    settings: db::switchers::Switcher,
    servers: Vec<StreamServer>,
    config: decide::Config,
    memory: Memory,
    stats: HashMap<Uuid, ss::StreamServerStats>,
    scene_names: HashMap<Uuid, String>,
    current_scene: Option<SceneRef>,
    streaming: bool,
    recording: bool,
    recording_paused: bool,
    server_tasks: Vec<tokio::task::JoinHandle<()>>,
    started: Instant,
    /// Stream history, while streaming.
    recorder: Option<Recorder>,
}

impl Drop for Runtime {
    fn drop(&mut self) {
        for task in &self.server_tasks {
            task.abort();
        }
        // A restart or OBS going away mid-bucket: write what we have. The next
        // run's writes to the same bucket merge with it.
        if let Some(flush) = self.recorder.as_mut().and_then(Recorder::take)
            && let Ok(handle) = tokio::runtime::Handle::try_current()
        {
            let db = self.deps.db.clone();
            handle.spawn(async move { flush.write(&db).await });
        }
    }
}

/// Runs until OBS disconnects (`obs_events` closes) or an error occurs.
pub async fn run(deps: Deps, obs_events: &mut mpsc::UnboundedReceiver<Event>) -> Result<()> {
    let switcher_id = deps.switcher_id;
    let settings = db::switchers::get(&deps.db, switcher_id)
        .await?
        .ok_or(Error::SwitcherGone)?;
    let servers: Vec<StreamServer> = stream_servers::by_switcher_id(&deps.db, switcher_id)
        .await?
        .into_iter()
        .filter(|server| server.enabled)
        .collect();

    let mut runtime = Runtime {
        obs: ObsClient::new(InnerCommSender(deps.inner_comm.clone()), switcher_id),
        config: decide_config(&settings, &servers),
        memory: Memory::default(),
        stats: HashMap::new(),
        scene_names: HashMap::new(),
        current_scene: None,
        streaming: false,
        recording: false,
        recording_paused: false,
        server_tasks: Vec::new(),
        started: Instant::now(),
        recorder: None,
        settings,
        servers,
        deps,
    };

    runtime.sync_from_obs().await?;

    let (stats_tx, mut stats_rx) = mpsc::unbounded_channel();
    runtime.spawn_server_tasks(&stats_tx);

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut ticks: u32 = 0;

    loop {
        tokio::select! {
            Some(stats) = stats_rx.recv() => {
                runtime.stats.insert(stats.stream_server_id, stats);
            }
            event = obs_events.recv() => match event {
                Some(event) => runtime.on_obs_event(event).await,
                None => return Ok(()),
            },
            _ = tick.tick() => {
                ticks = ticks.wrapping_add(1);
                if ticks.is_multiple_of(SCENE_REFRESH_TICKS) {
                    match runtime.refresh_scenes().await {
                        Ok(()) => runtime.reconcile_scene_recording().await,
                        Err(error) => tracing::debug!(%error, "Couldn't refresh the scene list"),
                    }
                }
                runtime.on_tick().await;
            }
        }
    }
}

fn decide_config(settings: &db::switchers::Switcher, servers: &[StreamServer]) -> decide::Config {
    decide::Config {
        auto_switch: settings.bitrate_switcher_enabled,
        only_while_streaming: settings.only_switch_when_streaming,
        instant_recover: settings.instantly_switch_on_recover,
        retry_attempts: u32::try_from(settings.retry_attempts).unwrap_or(0),
        scenes: RoleScenes {
            live: settings.scene_live,
            low: settings.scene_low,
            offline: settings.scene_offline,
        },
        starting: settings.scene_starting,
        switch_to_live: settings.switch_to_live,
        // Already in priority order.
        servers: servers
            .iter()
            .map(|server| decide::Server {
                id: server.id,
                overrides: RoleScenes {
                    live: server.override_scene_live,
                    low: server.override_scene_low,
                    offline: server.override_scene_offline,
                },
                instant_degrade: ss::pushes_stats(server.kind),
                delay_live_recovery: ss::pushes_stats(server.kind),
            })
            .collect(),
        offline_timeout: timeout_duration(settings.offline_timeout),
        ending: settings.scene_ending,
        ending_timeout: timeout_duration(settings.ending_timeout),
    }
}

/// A timeout setting in minutes, if on.
fn timeout_minutes(setting: Option<i32>) -> Option<u32> {
    setting
        .and_then(|minutes| u32::try_from(minutes).ok())
        .filter(|minutes| *minutes > 0)
}

fn timeout_duration(setting: Option<i32>) -> Option<Duration> {
    timeout_minutes(setting).map(|minutes| Duration::from_secs(u64::from(minutes) * 60))
}

/// When an output started, from how long OBS says it has been running.
fn started_at(active: bool, duration_ms: u32) -> Option<chrono::DateTime<Utc>> {
    active.then(|| Utc::now() - chrono::TimeDelta::milliseconds(i64::from(duration_ms)))
}

impl Runtime {
    fn id(&self) -> Uuid {
        self.deps.switcher_id
    }

    /// Reads the current stream, recording and scene state from OBS.
    async fn sync_from_obs(&mut self) -> Result<()> {
        let stream = self.obs.stream_status().await?;
        let record = self.obs.recording_status().await?;
        self.refresh_scenes().await?;
        self.streaming = stream.output_active;
        self.recording = record.output_active;
        self.recording_paused = record.output_paused;

        // Pick up the stream OBS is running (after a restart or reconnect), or
        // end one that stopped while we weren't watching.
        match started_at(stream.output_active, stream.output_duration) {
            Some(started) => self.begin_history(started, true).await,
            None => {
                if let Err(error) = db::streams::end_open(&self.deps.db, self.id(), None).await {
                    tracing::warn!(%error, "Couldn't end the stream in history");
                }
            }
        }

        self.reconcile_scene_recording().await;

        let auto_switch = self.settings.bitrate_switcher_enabled;
        let current_scene = self.current_scene.clone();
        let stream_started_at = started_at(stream.output_active, stream.output_duration);
        let frames = match stream_started_at {
            Some(_) => self.frames().await,
            None => None,
        };
        self.deps.status.update(|status| {
            // Keep the counters from the start of this same stream (a restart of
            // the switcher mid-stream); otherwise count from now, like NOALBS.
            let same_stream = match (status.stream_started_at, stream_started_at) {
                (Some(before), Some(now)) => (now - before).abs() < chrono::TimeDelta::seconds(5),
                _ => false,
            };
            status.stream_frames = match stream_started_at {
                Some(_) if same_stream => status.stream_frames.or(frames),
                Some(_) => frames,
                None => None,
            };
            status.obs_connected = true;
            status.streaming = stream.output_active;
            status.stream_started_at = stream_started_at;
            status.recording = record.output_active;
            status.recording_paused = record.output_paused;
            status.recording_started_at = started_at(record.output_active, record.output_duration);
            status.current_scene = current_scene;
            status.auto_switch = auto_switch;
        });
        Ok(())
    }

    async fn refresh_scenes(&mut self) -> Result<()> {
        let list = self.obs.scene_list().await?;
        self.scene_names = list
            .scenes
            .iter()
            .filter_map(|scene| {
                scene
                    .scene_uuid
                    .map(|uuid| (uuid, scene.scene_name.clone()))
            })
            .collect();
        let current = SceneRef {
            uuid: list.current_program_scene_uuid,
            name: list.current_program_scene_name,
        };
        self.attribution().scene_seen(&current.name);
        if self.current_scene.as_ref() != Some(&current) {
            self.current_scene = Some(current.clone());
            self.deps
                .status
                .update(|status| status.current_scene = Some(current));
        }
        Ok(())
    }

    fn scene_name(&self, uuid: Uuid) -> String {
        self.scene_names
            .get(&uuid)
            .cloned()
            .unwrap_or_else(|| uuid.to_string())
    }

    fn spawn_server_tasks(&mut self, stats_tx: &mpsc::UnboundedSender<ss::StreamServerStats>) {
        let triggers = Arc::new(RwLock::new(Triggers {
            low: enabled_trigger(self.settings.trigger_low),
            offline: enabled_trigger(self.settings.trigger_offline),
            rtt: enabled_trigger(self.settings.trigger_rtt),
            rtt_offline: enabled_trigger(self.settings.trigger_rtt_offline),
        }));

        for server in &self.servers {
            if !ss::url_allowed(&server.stats_url, self.deps.allow_private_stats_urls) {
                // Not polled; it shows as offline. The URL stays out of the log.
                tracing::warn!(stream_server = %server.name, "A stats URL points at a private address; not polling it");
                continue;
            }
            let source = ss::Source {
                secrets: ss::Secrets::decrypt(server, &self.deps.data_key),
                server: server.clone(),
                reqwest_client: self.deps.reqwest.clone(),
                stats_tx: stats_tx.clone(),
                triggers: triggers.clone(),
                allow_private: self.deps.allow_private_stats_urls,
            };

            let task = match server.kind {
                StreamServerKind::Belabox => crate::spawn(ss::Belabox::run(source)),
                StreamServerKind::Mediamtx => crate::spawn(ss::Mediamtx::run(source)),
                StreamServerKind::Sls => crate::spawn(ss::SrtLiveServer::run(source)),
                StreamServerKind::Nginx => crate::spawn(ss::Nginx::run(source)),
                StreamServerKind::Nms => crate::spawn(ss::NodeMediaServer::run(source)),
                StreamServerKind::Nimble => crate::spawn(ss::Nimble::run(source)),
                StreamServerKind::Xiu => crate::spawn(ss::Xiu::run(source)),
                StreamServerKind::Openirl => crate::spawn(ss::OpenIrl::run(source)),
                StreamServerKind::Openrtmp => crate::spawn(ss::OpenRtmp::run(source)),
                StreamServerKind::Rist => crate::spawn(ss::Rist::run(source)),
                StreamServerKind::Irlhosting => crate::spawn(ss::Irlhosting::run(source)),
                StreamServerKind::IrlhostingWebsocket => {
                    crate::spawn(ss::IrlhostingWebsocket::run(source))
                }
            };
            self.server_tasks.push(task);
        }
    }

    /// Once a second: publish stats, decide, and switch if needed.
    async fn on_tick(&mut self) {
        let now = Utc::now();
        let fresh: HashMap<Uuid, &ss::StreamServerStats> = self
            .stats
            .iter()
            .filter(|(_, stats)| now - stats.timestamp < STATS_FRESH_FOR)
            .map(|(id, stats)| (*id, stats))
            .collect();
        if fresh.len() < self.servers.len() && self.started.elapsed() < WARM_UP {
            return;
        }
        let readings: HashMap<Uuid, Reading> = fresh
            .iter()
            .map(|(id, stats)| (*id, stats.switch_type.reading()))
            .collect();

        let sample = StatsSample {
            at: now,
            servers: self
                .servers
                .iter()
                .map(|server| match fresh.get(&server.id) {
                    Some(stats) => ServerSample {
                        server_id: server.id,
                        bitrate: stats.bitrate,
                        rtt: stats.rtt,
                        state: reading_state(stats.switch_type.reading()),
                    },
                    None => ServerSample {
                        server_id: server.id,
                        bitrate: 0,
                        rtt: None,
                        state: FeedState::Offline,
                    },
                })
                .collect(),
        };
        self.publish_stats(&sample).await;
        let raw: HashMap<Uuid, serde_json::Value> = if self.recorder.is_some() {
            fresh
                .iter()
                .filter(|(_, stats)| !stats.raw_stats.is_null())
                .map(|(id, stats)| (*id, stats.raw_stats.clone()))
                .collect()
        } else {
            HashMap::new()
        };

        let observation = Observation {
            now: Instant::now(),
            streaming: self.streaming,
            recording: self.recording,
            current_scene: self.current_scene.as_ref().and_then(|scene| scene.uuid),
            readings: &readings,
        };
        let outcome = decide::decide(&self.config, &observation, &mut self.memory);
        self.deps.status.update(|status| {
            status.state = outcome.state;
            status.active_server_id = outcome.active_server;
        });
        if self.streaming
            && let Some(flush) = self
                .recorder
                .as_mut()
                .and_then(|recorder| recorder.add(&sample, &raw, outcome.state))
        {
            self.write_history(flush);
        }

        if let Some(target) = outcome.switch_to {
            let cause = outcome.active_server.and_then(|id| {
                let server = self.servers.iter().find(|server| server.id == id)?;
                let numbers = sample.servers.iter().find(|s| s.server_id == id)?;
                Some((server.name.clone(), numbers.bitrate, numbers.rtt))
            });
            self.switch_to(target, cause).await;
        }
        if let Some(stop) = outcome.stop {
            self.stop_for_timeout(stop).await;
        }
    }

    /// The feed has been gone for the offline timeout, or the Ending scene on
    /// for the ending timeout: stop the stream (and, through
    /// [`Self::on_stream`], whatever follows it). Chat hears about it from the
    /// `OfflineTimeout` / `EndingTimeout` event.
    async fn stop_for_timeout(&mut self, stop: Stop) {
        let (minutes, by, event): (_, _, fn(u32) -> EventDetail) = match stop {
            Stop::OfflineTimeout => (
                timeout_minutes(self.settings.offline_timeout),
                "the offline timeout",
                |minutes| EventDetail::OfflineTimeout { minutes },
            ),
            Stop::EndingTimeout => (
                timeout_minutes(self.settings.ending_timeout),
                "the ending timeout",
                |minutes| EventDetail::EndingTimeout { minutes },
            ),
        };
        let Some(minutes) = minutes else {
            return;
        };
        tracing::info!(minutes, ?stop, "Timeout reached, stopping the stream");
        let action = IntentAction::Stream { active: false };
        self.attribution().add_intent(Intent {
            action: action.clone(),
            by: by.into(),
        });
        match self.obs.stream(false).await {
            Ok(()) => self.record(event(minutes)).await,
            Err(error) => {
                self.take_intent(&action);
                tracing::warn!(%error, ?stop, "Couldn't stop the stream for the timeout");
            }
        }
    }

    /// Starts recording history for the stream that started at `started_at`:
    /// the stored one when `resume` (OBS was already streaming), else a new one.
    async fn begin_history(&mut self, started_at: chrono::DateTime<Utc>, resume: bool) {
        if let Some(flush) = self
            .recorder
            .take()
            .and_then(|mut recorder| recorder.take())
        {
            self.write_history(flush);
        }
        let db = &self.deps.db;
        let stream = if resume {
            db::streams::resume(db, self.id(), started_at).await
        } else {
            db::streams::start(db, self.id(), started_at).await
        };
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(%error, "Couldn't start stream history");
                return;
            }
        };

        let sources: Vec<db::streams::Source> = self
            .servers
            .iter()
            .map(|server| db::streams::Source {
                server_id: server.id,
                name: server.name.clone(),
                kind: server.kind,
            })
            .collect();
        if let Err(error) = db::streams::upsert_sources(db, stream.id, &sources).await {
            tracing::warn!(%error, "Couldn't record the stream's servers");
        }
        self.recorder = Some(Recorder::new(stream.id));
    }

    /// Writes what's left of the recorded stream and ends it at `at`.
    fn end_history(&mut self, at: chrono::DateTime<Utc>) {
        let Some(mut recorder) = self.recorder.take() else {
            return;
        };
        let stream_id = recorder.stream_id();
        let flush = recorder.take();
        let db = self.deps.db.clone();
        // In order, so the last samples are in before it ends. By ID, so this
        // can't end a stream that starts meanwhile.
        crate::spawn(async move {
            if let Some(flush) = flush {
                flush.write(&db).await;
            }
            if let Err(error) = db::streams::end(&db, stream_id, at).await {
                tracing::warn!(%error, "Couldn't end the stream in history");
            }
        });
    }

    /// Writes finished buckets without holding up the tick.
    fn write_history(&self, flush: Flush) {
        let db = self.deps.db.clone();
        crate::spawn(async move { flush.write(&db).await });
    }

    async fn publish_stats(&self, sample: &StatsSample) {
        if sample.servers.is_empty() {
            return;
        }
        match serde_json::to_vec(sample) {
            Ok(payload) => {
                let subject = live::stats_subject(self.id());
                if let Err(error) = self.deps.js.publish(subject, payload.into()).await {
                    tracing::warn!(%error, "Failed to publish stats");
                }
            }
            Err(error) => tracing::warn!(%error, "Failed to serialize stats"),
        }
    }

    /// Switches OBS to `target` automatically and logs why.
    async fn switch_to(&mut self, target: Uuid, cause: Option<(String, u32, Option<f64>)>) {
        let from = self.attribution().scene();
        let switched = self.obs.switch_scene_uuid(target).await;
        crate::metrics::SCENE_SWITCHES.add(1, &[crate::metrics::outcome(&switched)]);
        match switched {
            Ok(()) => {
                let to = self.scene_name(target);
                self.attribution().own_switch(target, &to);
                let (server, bitrate, rtt) = match cause {
                    Some((server, bitrate, rtt)) => (Some(server), Some(bitrate), rtt),
                    None => (None, None, None),
                };
                self.record(EventDetail::SceneSwitched {
                    from,
                    to,
                    reason: SwitchReason::Auto,
                    by: None,
                    server,
                    bitrate,
                    rtt,
                })
                .await;
            }
            Err(error) => tracing::warn!(%error, %target, "Automatic scene switch failed"),
        }
    }

    /// Make recording match the scene allow-list without ever resuming a pause
    /// that was already present when NOALBS got involved.
    async fn reconcile_scene_recording(&mut self) {
        let scene = self.current_scene.as_ref().and_then(|scene| scene.uuid);
        match recording_scene_action(
            self.recording,
            self.recording_paused,
            self.paused_by_scene_control(),
            scene,
            &self.settings.recording_scene_ids,
        ) {
            Some(RecordingSceneAction::Pause) => self.set_scene_control_pause(true).await,
            Some(RecordingSceneAction::Resume) => self.set_scene_control_pause(false).await,
            Some(RecordingSceneAction::ClearOwnership) => self.set_paused_by_scene_control(false),
            None => {}
        }
    }

    fn paused_by_scene_control(&self) -> bool {
        self.deps.paused_by_scene_control.load(Ordering::Relaxed)
    }

    fn set_paused_by_scene_control(&self, owned: bool) {
        self.deps
            .paused_by_scene_control
            .store(owned, Ordering::Relaxed);
    }

    async fn set_scene_control_pause(&mut self, pause: bool) {
        let scene = self
            .current_scene
            .as_ref()
            .map(|scene| scene.name.clone())
            .unwrap_or_default();
        if let Err(error) = self.obs.pause_recording(pause).await {
            tracing::warn!(%error, "Couldn't change the recording state");
            let message = if pause {
                format!(
                    "Couldn't pause the recording on {scene}: {error}. Some recording formats can't be paused."
                )
            } else {
                format!("Couldn't resume the recording on {scene}: {error}")
            };
            self.record(EventDetail::RecordingSceneError { message })
                .await;
            return;
        }
        // OBS sends a matching event shortly afterwards. Update eagerly to
        // avoid issuing a duplicate command before it reaches us.
        self.recording_paused = pause;
        self.set_paused_by_scene_control(pause);
        self.deps
            .status
            .update(|status| status.recording_paused = pause);
        self.record(if pause {
            EventDetail::RecordingScenePaused { scene }
        } else {
            EventDetail::RecordingSceneResumed { scene }
        })
        .await;
    }

    async fn record(&self, detail: EventDetail) {
        if let Err(error) =
            live::record_event(&self.deps.db, &self.deps.nats, self.id(), detail).await
        {
            tracing::warn!(%error, "Failed to record switcher event");
        }
    }

    /// OBS's frame counters right now, to count a stream's dropped frames from.
    async fn frames(&self) -> Option<ObsFrames> {
        match self.obs.stats().await {
            Ok(stats) => Some(ObsFrames {
                render_total: stats.render_total_frames,
                render_skipped: stats.render_skipped_frames,
                output_total: stats.output_total_frames,
                output_skipped: stats.output_skipped_frames,
            }),
            Err(error) => {
                tracing::debug!(%error, "Couldn't read OBS's stats");
                None
            }
        }
    }

    fn attribution(&self) -> std::sync::MutexGuard<'_, Attribution> {
        self.deps
            .attribution
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Who asked for `action` from the dashboard, if someone did recently.
    fn take_intent(&self, action: &IntentAction) -> Option<String> {
        self.attribution().take_intent(action)
    }

    async fn on_obs_event(&mut self, event: Event) {
        match event {
            Event::StreamStateChanged(change) => match change.output_state.as_str() {
                "OBS_WEBSOCKET_OUTPUT_STARTED" => self.on_stream(true).await,
                "OBS_WEBSOCKET_OUTPUT_STOPPED" => self.on_stream(false).await,
                _ => {}
            },
            Event::RecordStateChanged(change) => match change.output_state.as_str() {
                "OBS_WEBSOCKET_OUTPUT_STARTED" => self.on_record(true).await,
                "OBS_WEBSOCKET_OUTPUT_STOPPED" => self.on_record(false).await,
                "OBS_WEBSOCKET_OUTPUT_PAUSED" => self.on_record_pause(true).await,
                "OBS_WEBSOCKET_OUTPUT_RESUMED" => self.on_record_pause(false).await,
                _ => {}
            },
            Event::CurrentProgramSceneChanged(change) => {
                self.on_scene(change.scene_uuid, change.scene_name).await;
            }
            // Audio is relayed by the connection (`audio.rs`).
            Event::InputMuteStateChanged(_)
            | Event::InputVolumeChanged(_)
            | Event::InputVolumeMeters(_)
            | Event::Unknown => {}
        }
    }

    async fn on_stream(&mut self, active: bool) {
        self.streaming = active;
        let frames = if active { self.frames().await } else { None };
        self.deps.status.update(|status| {
            status.streaming = active;
            status.stream_started_at = active.then(Utc::now);
            status.stream_frames = frames;
        });
        if active {
            self.begin_history(Utc::now(), false).await;
        } else {
            self.end_history(Utc::now());
        }
        let by = self.take_intent(&IntentAction::Stream { active });
        self.record(if active {
            EventDetail::StreamStarted { by }
        } else {
            EventDetail::StreamStopped { by }
        })
        .await;

        if self.settings.record_while_streaming
            && self.recording != active
            && let Err(error) = self.obs.record(active).await
        {
            tracing::warn!(%error, "Couldn't start or stop the recording with the stream");
        }

        let follow = if active {
            self.settings.belabox_start_with_stream
        } else {
            self.settings.belabox_stop_with_stream
        };
        if follow {
            self.sync_belabox(active);
        }

        let follow = if active {
            self.settings.moblin_start_with_stream
        } else {
            self.settings.moblin_stop_with_stream
        };
        if follow && self.settings.moblin_password.is_some() {
            self.sync_moblin(active);
        }

        if active
            && self.settings.switch_to_starting
            && let Some(starting) = self.settings.scene_starting
        {
            self.switch_to(starting, None).await;
        }
    }

    /// Starts or stops BELABOX with the OBS stream (in the background: BELABOX
    /// Cloud may be slow). Failures show in the activity feed.
    fn sync_belabox(&self, active: bool) {
        let Some(belabox) = self.deps.belabox.clone() else {
            return;
        };
        if belabox.streaming() == active {
            return;
        }
        let (db, nats, switcher_id) = (self.deps.db.clone(), self.deps.nats.clone(), self.id());

        crate::spawn(async move {
            let command = BelaboxCommand {
                action: if active {
                    BelaboxAction::Start
                } else {
                    BelaboxAction::Stop
                },
                by: "the OBS stream".into(),
            };
            let refusal = match belabox.send(command).await {
                Ok(()) | Err(BelaboxRefusal::AlreadyStreaming | BelaboxRefusal::NotStreaming) => {
                    return;
                }
                Err(refusal) => refusal,
            };
            let verb = if active { "start" } else { "stop" };
            let detail = EventDetail::BelaboxError {
                message: format!("Couldn't {verb} BELABOX with the OBS stream: {refusal}"),
            };
            if let Err(error) = live::record_event(&db, &nats, switcher_id, detail).await {
                tracing::warn!(%error, "Failed to record switcher event");
            }
        });
    }

    /// Starts or stops Moblin's stream with the OBS stream, through its
    /// connection (which may be on another obs-link). Failures show in the
    /// activity feed.
    fn sync_moblin(&self, active: bool) {
        let (db, nats, switcher_id) = (self.deps.db.clone(), self.deps.nats.clone(), self.id());

        crate::spawn(async move {
            let command = MoblinCommand {
                action: MoblinAction::Stream { active },
                by: "the OBS stream".into(),
            };
            let reply = match serde_json::to_vec(&command) {
                Ok(payload) => tokio::time::timeout(
                    MOBLIN_REQUEST_TIMEOUT,
                    nats.request(live::moblin_request_subject(switcher_id), payload.into()),
                )
                .await
                .map_err(|_| MoblinRefusal::Timeout)
                .and_then(|reply| reply.map_err(|_| MoblinRefusal::NotConnected))
                .and_then(|message| {
                    serde_json::from_slice::<MoblinReply>(&message.payload)
                        .unwrap_or(Err(MoblinRefusal::Failed))
                }),
                Err(_) => Err(MoblinRefusal::Failed),
            };
            let refusal = match reply {
                Ok(()) | Err(MoblinRefusal::AlreadyStreaming | MoblinRefusal::NotStreaming) => {
                    return;
                }
                Err(refusal) => refusal,
            };
            let verb = if active { "start" } else { "stop" };
            let detail = EventDetail::MoblinError {
                message: format!("Couldn't {verb} Moblin with the OBS stream: {refusal}"),
            };
            if let Err(error) = live::record_event(&db, &nats, switcher_id, detail).await {
                tracing::warn!(%error, "Failed to record switcher event");
            }
        });
    }

    async fn on_record(&mut self, active: bool) {
        self.recording = active;
        self.recording_paused &= active;
        if !active {
            self.set_paused_by_scene_control(false);
        }
        self.deps.status.update(|status| {
            status.recording = active;
            status.recording_paused &= active;
            status.recording_started_at = active.then(Utc::now);
        });
        let by = self.take_intent(&IntentAction::Record { active });
        self.record(if active {
            EventDetail::RecordingStarted { by }
        } else {
            EventDetail::RecordingStopped { by }
        })
        .await;

        if active {
            self.reconcile_scene_recording().await;
        }
    }

    async fn on_record_pause(&mut self, paused: bool) {
        self.recording_paused = paused;
        self.deps.status.update(|status| {
            status.recording_paused = paused;
        });
        if !paused {
            self.reconcile_scene_recording().await;
        }
    }

    async fn on_scene(&mut self, uuid: Option<Uuid>, name: String) {
        let scene = SceneRef {
            uuid,
            name: name.clone(),
        };
        if let Some(uuid) = uuid {
            self.scene_names.insert(uuid, name.clone());
        }
        self.current_scene = Some(scene.clone());
        self.deps
            .status
            .update(|status| status.current_scene = Some(scene));

        self.reconcile_scene_recording().await;

        let change = self.attribution().scene_changed(uuid, &name);
        let SceneChange::Changed { from, by } = change else {
            // A repeat, or our own switch coming back: nothing new to log.
            return;
        };
        self.record(EventDetail::SceneSwitched {
            from,
            to: name,
            reason: if by.is_some() {
                SwitchReason::Dashboard
            } else {
                SwitchReason::Obs
            },
            by,
            server: None,
            bitrate: None,
            rtt: None,
        })
        .await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingSceneAction {
    Pause,
    Resume,
    /// The pause we made is gone (resumed by someone else, or recording stopped).
    ClearOwnership,
}

/// Determines the one safe action for recording state. An empty allow-list is
/// off; a missing OBS scene UUID is necessarily outside a non-empty list. A
/// pause is only resumed when scene control made it (`owned`).
fn recording_scene_action(
    recording: bool,
    paused: bool,
    owned: bool,
    current_scene: Option<Uuid>,
    recording_scenes: &[Uuid],
) -> Option<RecordingSceneAction> {
    if !recording || (owned && !paused) {
        return owned.then_some(RecordingSceneAction::ClearOwnership);
    }
    let enabled_here = current_scene.is_some_and(|scene| recording_scenes.contains(&scene));
    if paused {
        (owned && (enabled_here || recording_scenes.is_empty()))
            .then_some(RecordingSceneAction::Resume)
    } else {
        (!recording_scenes.is_empty() && !enabled_here).then_some(RecordingSceneAction::Pause)
    }
}

/// Sends OBS requests over the browser source's connection.
pub struct InnerCommSender(pub mpsc::UnboundedSender<app::InnerCommMessage>);

#[async_trait::async_trait]
impl obs_actions::ClientTransport for InnerCommSender {
    async fn request(
        &self,
        payload: String,
        _switcher_id: uuid::Uuid,
    ) -> obs_actions::error::Result<obs_actions::types::ObsMessage> {
        let (tx, rx) = tokio::sync::oneshot::channel::<serde_json::Value>();
        let inner = app::InnerCommMessage {
            message: payload,
            reply: app::InnerReply::Chan(tx),
            span: tracing::Span::none(),
        };

        self.0
            .send(inner)
            .map_err(|_| obs_actions::error::Error::ObsRequestError("Connection closed".into()))?;

        let response = tokio::time::timeout(std::time::Duration::from_secs(5), rx)
            .await
            .map_err(|_| obs_actions::error::Error::RequestTimeout)?
            .map_err(|_| obs_actions::error::Error::RequestTimeout)?;

        let response = serde_json::from_value::<obs_actions::types::ObsMessage>(response)?;

        Ok(response)
    }
}

/// A trigger from the database, where `NULL` (and, defensively, 0) means off.
/// An RTT trigger of 0 would otherwise fire on every sample.
fn enabled_trigger(value: Option<i32>) -> Option<u32> {
    value
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::{RecordingSceneAction, enabled_trigger, recording_scene_action};
    use uuid::Uuid;

    #[test]
    fn null_zero_and_negative_triggers_are_off() {
        assert_eq!(enabled_trigger(None), None);
        assert_eq!(enabled_trigger(Some(0)), None);
        assert_eq!(enabled_trigger(Some(-5)), None);
        assert_eq!(enabled_trigger(Some(800)), Some(800));
    }

    #[test]
    fn recording_scene_control_only_undoes_its_own_pause() {
        let live = Uuid::from_u128(1);
        let brb = Uuid::from_u128(2);
        let selected = [live];

        assert_eq!(
            recording_scene_action(true, false, false, Some(brb), &selected),
            Some(RecordingSceneAction::Pause)
        );
        assert_eq!(
            recording_scene_action(true, true, true, Some(live), &selected),
            Some(RecordingSceneAction::Resume)
        );
        assert_eq!(
            recording_scene_action(true, true, false, Some(live), &selected),
            None
        );
        assert_eq!(
            recording_scene_action(true, true, true, Some(brb), &[]),
            Some(RecordingSceneAction::Resume)
        );
        // A settings save restarts the switcher, which still owns its pause:
        // adding the scene on air to the list resumes it.
        assert_eq!(
            recording_scene_action(true, true, true, Some(brb), &[live, brb]),
            Some(RecordingSceneAction::Resume)
        );
        assert_eq!(
            recording_scene_action(false, false, true, Some(live), &selected),
            Some(RecordingSceneAction::ClearOwnership)
        );
    }
}
