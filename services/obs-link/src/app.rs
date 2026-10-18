//! The WebSocket server that browser sources connect to, and one
//! [`Connection`] per browser source.
//!
//! A browser source identifies with its switcher's connect token. It then gets
//! the OBS websocket settings, connects to OBS, and relays requests and events.
//! While OBS is connected, the switcher task runs (see [`crate::switcher`]).

use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use async_nats::jetstream;
use common::{crypto::DataKey, shutdown};
use futures_util::{SinkExt, StreamExt};
use live::{EventDetail, SwitcherStatus, Takeover};
use obs_actions::types::{Event, ObsMessage, ObsRequest, Reidentify};
use opentelemetry::KeyValue;
use serde::{Deserialize, Serialize};
use tokio::{
    net,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_tungstenite::tungstenite::{
    Error as WebSocketError, Message, error::ProtocolError,
    handshake::server::Request as Handshake, protocol::WebSocketConfig,
};
use tracing::{Instrument, Span, info};
use uuid::Uuid;

use crate::{
    attribution::SharedAttribution,
    audio, belabox,
    config::Config,
    error::{Error, Result},
    metrics, moblin,
    status::{self, StatusHandle},
    switcher,
};

const APP_USER_AGENT: &str = concat!("NOALBS-Cloud/", env!("CARGO_PKG_VERSION"));
/// Requests to the browser source not answered within this are dropped.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Delay before restarting a switcher that stopped with an error.
const SWITCHER_RESTART_DELAY: Duration = Duration::from_secs(5);
/// How long a client has to finish the WebSocket handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a browser source has to identify (it does so as soon as it connects).
const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(10);
/// The largest message we take from a client. Replies go on over NATS, which
/// takes 1 MB at most anyway; this only has to fit the biggest OBS answer.
const MAX_MESSAGE_SIZE: usize = 4 << 20;

/// Everything connections share.
struct Shared {
    nats: async_nats::Client,
    js: jetstream::Context,
    stores: live::Stores,
    belabox_url: String,
    db: db::DbPool,
    reqwest: reqwest::Client,
    allow_private_stats_urls: bool,
    data_key: DataKey,
}

pub async fn run(config: Config, mut shutdown_handle: shutdown::ShutdownHandler) -> Result<()> {
    tracing::info!(host = %config.host, port = config.port, "Running obs-link");

    let listener = net::TcpListener::bind(format!("{}:{}", config.host, config.port)).await?;
    let nats = async_nats::connect(&config.nats_url).await?;
    let js = jetstream::new(nats.clone());
    let stores = live::Stores::open(&js)
        .await
        .map_err(|error| Error::Nats(error.to_string()))?;
    let db = db::create_pool_with_options(&db::DbConfig {
        database_url: &config.postgres_url,
        max_connections: config.postgres_max_connections,
    })
    .await?;
    if config.allow_private_stats_urls {
        tracing::warn!("Private stats URLs are allowed; never enable this in production");
    }
    let reqwest =
        crate::stream_servers::http_client(APP_USER_AGENT, config.allow_private_stats_urls)?;

    let shared = Arc::new(Shared {
        nats,
        js,
        stores,
        belabox_url: config.belabox_url.clone(),
        db,
        reqwest,
        allow_private_stats_urls: config.allow_private_stats_urls,
        data_key: DataKey::from_hex(&config.data_encryption_key)?,
    });

    loop {
        tokio::select! {
            Ok((stream, addr)) = listener.accept() => {
                let span = tracing::info_span!(
                    target: telemetry::SCOPE,
                    "connection",
                    connection.kind = tracing::field::Empty,
                    switcher.id = tracing::field::Empty,
                );
                let connection = handle_connection(stream, addr, shared.clone(), shutdown_handle.clone());
                tokio::spawn(connection.instrument(span));
            }
            _ = shutdown_handle.changed() => {
                info!("Shutdown signal received. Stopping listener.");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_connection(
    stream: net::TcpStream,
    addr: std::net::SocketAddr,
    shared: Arc<Shared>,
    shutdown_handle: shutdown::ShutdownHandler,
) {
    // The path says who connects: a Moblin at `/moblin/<switcher ID>`, else a
    // browser source (`/link`, or anything older URLs use).
    let mut path = String::new();
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_SIZE))
        .max_frame_size(Some(MAX_MESSAGE_SIZE));
    let handshake = tokio_tungstenite::accept_hdr_async_with_config(
        stream,
        |request: &Handshake, response| {
            path = request.uri().path().to_owned();
            Ok(response)
        },
        Some(config),
    );
    let websocket = match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake).await {
        Ok(Ok(websocket)) => websocket,
        // Closed before sending a request: the kubelet's TCP probes, load balancer
        // checks and port scanners. Every probe would otherwise log an error.
        Ok(Err(WebSocketError::Protocol(ProtocolError::HandshakeIncomplete))) => {
            tracing::debug!(%addr, "Connection closed before a handshake");
            return;
        }
        Ok(Err(error)) => {
            tracing::info!(%error, "WebSocket handshake failed");
            return;
        }
        Err(_) => {
            tracing::info!("WebSocket handshake timed out");
            return;
        }
    };

    let moblin = moblin::switcher_in_path(&path);
    let kind = if moblin.is_some() {
        "moblin"
    } else {
        "browser_source"
    };
    Span::current().record("connection.kind", kind);
    let _counted = metrics::connection(kind);
    info!("New connection");

    if let Some(switcher_id) = moblin {
        let deps = moblin::Deps {
            db: shared.db.clone(),
            nats: shared.nats.clone(),
            kv: shared.stores.moblin.clone(),
            data_key: shared.data_key.clone(),
        };
        moblin::serve(websocket, switcher_id, deps, shutdown_handle).await;
        return;
    }

    let (inner_comm, inner_comm_rx) = mpsc::unbounded_channel();
    let (control_tx, control_rx) = mpsc::unbounded_channel();
    let mut connection = Connection {
        id: Uuid::new_v4(),
        websocket,
        shared,
        shutdown_handle,
        inner_comm,
        control_tx,
        requests: HashMap::new(),
        session: None,
        taken_over: false,
    };

    if let Err(error) = connection.run(inner_comm_rx, control_rx).await {
        tracing::warn!(%error, "Connection ended with an error");
    }
    connection.cleanup().await;
}

/// A message for the browser source, from the switcher or the API.
pub struct InnerCommMessage {
    pub message: String,
    pub reply: InnerReply,
    /// Lasts until OBS answers: the request's part of the sender's trace (none
    /// for untraced requests).
    pub span: Span,
}

/// A request the browser source hasn't answered yet.
struct Pending {
    reply: InnerReply,
    sent: Instant,
    span: Span,
}

pub enum InnerReply {
    Nats(Option<async_nats::Subject>),
    Chan(oneshot::Sender<serde_json::Value>),
}

/// News for a connection from NATS.
enum Control {
    /// The API saved settings for this switcher.
    ConfigChanged,
    /// Another connection identified for the same switcher.
    TakenOver,
    /// Someone watches the audio levels.
    AudioWanted,
    /// Staff asked for a fresh connection: close, and the browser source
    /// reconnects.
    Reconnect,
}

enum Flow {
    Continue,
    Close,
}

/// A browser source that identified as a switcher.
struct Session {
    switcher_id: Uuid,
    token_hash: [u8; 32],
    /// The OBS settings last sent, to notice changes.
    obs_settings: (i32, Option<Vec<u8>>),
    status: StatusHandle,
    attribution: SharedAttribution,
    /// See `switcher::Deps::paused_by_scene_control`.
    paused_by_scene_control: Arc<AtomicBool>,
    obs_connected: bool,
    /// Over the plan's switcher limit: stays connected, but no switcher runs.
    paused: bool,
    /// Status publisher, request forwarder and control listener.
    tasks: Vec<JoinHandle<()>>,
    switcher: Option<RunningSwitcher>,
    /// The BELABOX remote key last used (encrypted), to notice changes.
    belabox_key: Option<Vec<u8>>,
    /// The BELABOX Cloud link, for switchers with a remote key (not while paused).
    belabox: Option<belabox::Link>,
    audio: audio::Meters,
}

struct RunningSwitcher {
    task: JoinHandle<()>,
    events: mpsc::UnboundedSender<obs_actions::types::Event>,
}

struct Connection {
    id: Uuid,
    websocket: tokio_tungstenite::WebSocketStream<net::TcpStream>,
    shared: Arc<Shared>,
    shutdown_handle: shutdown::ShutdownHandler,
    inner_comm: mpsc::UnboundedSender<InnerCommMessage>,
    control_tx: mpsc::UnboundedSender<Control>,
    requests: HashMap<String, Pending>,
    session: Option<Session>,
    /// Closed because a newer connection took over: leave its status alone.
    taken_over: bool,
}

impl Connection {
    async fn run(
        &mut self,
        mut inner_comm_rx: mpsc::UnboundedReceiver<InnerCommMessage>,
        mut control_rx: mpsc::UnboundedReceiver<Control>,
    ) -> Result<()> {
        let mut prune = tokio::time::interval(Duration::from_secs(10));
        let identify_deadline = tokio::time::sleep(IDENTIFY_TIMEOUT);
        tokio::pin!(identify_deadline);

        loop {
            tokio::select! {
                _ = self.shutdown_handle.changed() => {
                    tracing::info!("Shutdown signal received. Closing WebSocket connection.");
                    self.websocket.close(None).await?;
                    return Ok(());
                }
                message = self.websocket.next() => match message {
                    Some(Ok(Message::Text(text))) => match self.handle_message(&text).await {
                        Ok(Flow::Continue) => {}
                        Ok(Flow::Close) => return self.close().await,
                        Err(error) => tracing::warn!(%error, "Error handling message"),
                    },
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("WebSocket connection closed by client");
                        return Ok(());
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(error.into()),
                },
                Some(message) = inner_comm_rx.recv() => self.forward_to_browser(message).await?,
                Some(control) = control_rx.recv() => {
                    if let Flow::Close = self.handle_control(control).await? {
                        return self.close().await;
                    }
                }
                _ = prune.tick() => {
                    self.prune_requests();
                    self.resubscribe_audio().await?;
                }
                _ = &mut identify_deadline, if self.session.is_none() => {
                    tracing::info!("Browser source didn't identify in time");
                    return self.close().await;
                }
            }
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.websocket.close(None).await?;
        Ok(())
    }

    async fn send_request(&mut self, request: RequestKind, token: Option<String>) -> Result<()> {
        let message = serde_json::to_string(&Request { token, request })?;
        self.websocket.send(Message::Text(message.into())).await?;
        Ok(())
    }

    /// Tells the browser source it may not connect, so it stops reconnecting.
    async fn refuse(&mut self, kind: RequestErrorKind) -> Result<Flow> {
        self.send_request(RequestKind::RequestError(RequestError { kind }), None)
            .await?;
        self.send_request(RequestKind::IdentifyAck(false), None)
            .await?;
        Ok(Flow::Close)
    }

    async fn forward_to_browser(&mut self, message: InnerCommMessage) -> Result<()> {
        let token = match message.reply {
            InnerReply::Nats(None) => None,
            reply => {
                let token = Uuid::new_v4().to_string();
                let pending = Pending {
                    reply,
                    sent: Instant::now(),
                    span: message.span,
                };
                self.requests.insert(token.clone(), pending);
                Some(token)
            }
        };
        self.send_request(RequestKind::ObsRawRequest(message.message), token)
            .await
    }

    /// Asks OBS for audio levels while someone watches, and stops once nobody does.
    async fn resubscribe_audio(&mut self) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let change = session
            .audio
            .resubscribe(Instant::now(), session.obs_connected);
        match change {
            Some(levels) => self.reidentify(levels).await,
            None => Ok(()),
        }
    }

    /// Changes the OBS session's event subscriptions (the browser source relays it).
    async fn reidentify(&mut self, levels: bool) -> Result<()> {
        let request = serde_json::to_string(&ObsRequest::Reidentify(Reidentify {
            event_subscriptions: audio::subscriptions(levels),
        }))?;
        self.send_request(RequestKind::ObsRawRequest(request), None)
            .await
    }

    /// Relays OBS's audio events to the dashboard's mixers.
    async fn on_audio_event(&mut self, event: &Event) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let update = match event {
            Event::InputVolumeMeters(meters) => session.audio.frame(meters, Instant::now()),
            Event::InputMuteStateChanged(change) => Some(live::AudioUpdate::Muted {
                input: change.input_name.clone(),
                muted: change.input_muted,
            }),
            Event::InputVolumeChanged(change) => Some(live::AudioUpdate::Volume {
                input: change.input_name.clone(),
                volume_db: change.input_volume_db,
            }),
            _ => None,
        };
        if let Some(update) = update {
            let payload = serde_json::to_vec(&update)?;
            self.shared
                .nats
                .publish(live::audio_subject(session.switcher_id), payload.into())
                .await?;
        }
        Ok(())
    }

    /// Drops requests the browser source never answered (e.g. OBS went away).
    fn prune_requests(&mut self) {
        self.requests
            .retain(|_, pending| pending.sent.elapsed() < REQUEST_TIMEOUT);
    }

    async fn handle_message(&mut self, message: &str) -> Result<Flow> {
        let response = serde_json::from_str::<Response>(message)?;

        match response.response {
            ResponseKind::Identify(identify) => return self.identify(&identify.token).await,
            ResponseKind::ObsWsConnected(connected) => self.obs_connection_changed(connected).await,
            ResponseKind::ObsRawResponse(obs_response) => {
                if let Some(token) = response.token
                    && let Some(pending) = self.requests.remove(&token)
                {
                    let source = match pending.reply {
                        InnerReply::Nats(_) => "nats",
                        InnerReply::Chan(_) => "switcher",
                    };
                    metrics::OBS_REQUEST_DURATION.record(
                        pending.sent.elapsed().as_secs_f64(),
                        &[KeyValue::new("source", source)],
                    );
                    match pending.reply {
                        InnerReply::Nats(Some(subject)) => {
                            let payload = serde_json::to_string(&obs_response)?;
                            self.shared
                                .nats
                                .publish(subject, payload.into())
                                .instrument(pending.span)
                                .await?;
                        }
                        InnerReply::Chan(sender) => {
                            let _ = sender.send(obs_response);
                        }
                        InnerReply::Nats(None) => {}
                    }
                }
            }
            ResponseKind::ObsRawEvent(event) => {
                let ObsMessage::Event(event) = serde_json::from_value::<ObsMessage>(event)? else {
                    return Ok(Flow::Continue);
                };
                self.on_audio_event(&event).await?;
                if matches!(event, Event::InputVolumeMeters(_)) {
                    return Ok(Flow::Continue);
                }
                if let Some(running) = self.session.as_ref().and_then(|s| s.switcher.as_ref()) {
                    let _ = running.events.send(event);
                }
            }
            ResponseKind::Error(error) => {
                tracing::warn!(%error, "Browser source reported an error");
            }
        }

        Ok(Flow::Continue)
    }

    async fn identify(&mut self, token: &str) -> Result<Flow> {
        if self.session.is_some() {
            tracing::warn!("Browser source identified twice; ignoring");
            return Ok(Flow::Continue);
        }

        let token_hash = common::tokens::hash(token);
        let Some(switcher) =
            db::switchers::by_connect_token_hash(&self.shared.db, &token_hash).await?
        else {
            tracing::info!("Browser source used an unknown connect token");
            return self.refuse(RequestErrorKind::AuthenticationFailed).await;
        };
        let switcher_id = switcher.id;
        Span::current().record("switcher.id", tracing::field::display(switcher_id));
        tracing::info!("Browser source identified");

        self.send_request(RequestKind::IdentifyAck(true), None)
            .await?;
        self.send_request(RequestKind::Settings(self.settings_for(&switcher)), None)
            .await?;
        db::switchers::touch_last_seen(&self.shared.db, switcher_id).await?;
        let paused = db::switchers::is_paused(&self.shared.db, switcher_id).await?;
        if paused {
            tracing::info!(%switcher_id, "Switcher is over its plan's limit; not switching");
        }

        let mut initial = SwitcherStatus::connected(switcher.bitrate_switcher_enabled);
        initial.paused = paused;
        let status = StatusHandle::new(initial);
        let attribution = SharedAttribution::default();
        let tasks = vec![
            crate::spawn(metrics::track_switcher(status.subscribe())),
            crate::spawn(status::publish(
                self.shared.stores.status.clone(),
                switcher_id,
                live::STATUS_HEARTBEAT,
                status.subscribe(),
            )),
            crate::spawn(forward_obs_requests(
                self.shared.nats.clone(),
                switcher_id,
                self.inner_comm.clone(),
            )),
            crate::spawn(collect_intents(
                self.shared.nats.clone(),
                switcher_id,
                attribution.clone(),
            )),
            crate::spawn(listen_for_control(
                self.shared.nats.clone(),
                switcher_id,
                self.id,
                self.control_tx.clone(),
            )),
        ];

        // Older connections for this switcher close when they see this.
        let takeover = serde_json::to_vec(&Takeover {
            connection_id: self.id,
        })?;
        self.shared
            .nats
            .publish(live::takeover_subject(switcher_id), takeover.into())
            .await?;

        let belabox = if paused {
            None
        } else {
            start_belabox(&self.shared, &switcher)
        };

        self.session = Some(Session {
            switcher_id,
            token_hash,
            obs_settings: (switcher.port, switcher.password.clone()),
            status,
            attribution,
            paused_by_scene_control: Arc::default(),
            obs_connected: false,
            paused,
            tasks,
            switcher: None,
            belabox_key: switcher.belabox_remote_key.clone(),
            belabox,
            audio: audio::Meters::default(),
        });

        Ok(Flow::Continue)
    }

    fn settings_for(&self, switcher: &db::switchers::Switcher) -> Settings {
        let password = switcher.password.as_deref().and_then(|encrypted| {
            match self.shared.data_key.decrypt(encrypted) {
                Ok(bytes) => String::from_utf8(bytes).ok(),
                Err(error) => {
                    tracing::warn!(%error, switcher_id = %switcher.id, "Couldn't decrypt the OBS password");
                    None
                }
            }
        });

        Settings {
            obs: ObsSettings {
                port: u32::try_from(switcher.port).unwrap_or(4455),
                password,
            },
        }
    }

    async fn obs_connection_changed(&mut self, connected: bool) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session.obs_connected == connected {
            return;
        }
        session.obs_connected = connected;
        // A new OBS session starts with the default events, without levels.
        session.audio.obs_reconnected();
        let switcher_id = session.switcher_id;

        if connected {
            session.status.update(|status| status.obs_connected = true);
            if !session.paused {
                self.start_switcher();
            }
        } else {
            self.stop_switcher();
            if let Some(session) = self.session.as_ref() {
                session.status.update(|status| {
                    let (auto_switch, paused) = (status.auto_switch, status.paused);
                    *status = SwitcherStatus::connected(auto_switch);
                    status.paused = paused;
                });
            }
        }

        let detail = if connected {
            EventDetail::ObsConnected
        } else {
            EventDetail::ObsDisconnected
        };
        if let Err(error) =
            live::record_event(&self.shared.db, &self.shared.nats, switcher_id, detail).await
        {
            tracing::warn!(%error, "Failed to record OBS connection change");
        }
        if connected && let Err(error) = self.subscribe_events().await {
            tracing::warn!(%error, "Couldn't change OBS's event subscriptions");
        }
    }

    /// A new OBS session gets every default event: narrows them to the ones we
    /// use (with levels if someone watches). Old browser sources relay this too.
    async fn subscribe_events(&mut self) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let levels = session
            .audio
            .resubscribe(Instant::now(), session.obs_connected)
            .unwrap_or(false);
        self.reidentify(levels).await
    }

    fn start_switcher(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if let Some(running) = session.switcher.take() {
            running.task.abort();
        }

        let deps = switcher::Deps {
            switcher_id: session.switcher_id,
            db: self.shared.db.clone(),
            nats: self.shared.nats.clone(),
            js: self.shared.js.clone(),
            reqwest: self.shared.reqwest.clone(),
            allow_private_stats_urls: self.shared.allow_private_stats_urls,
            status: session.status.clone(),
            inner_comm: self.inner_comm.clone(),
            attribution: session.attribution.clone(),
            paused_by_scene_control: session.paused_by_scene_control.clone(),
            data_key: self.shared.data_key.clone(),
            belabox: session.belabox.as_ref().map(belabox::Link::handle),
        };
        let (events, mut events_rx) = mpsc::unbounded_channel();

        let task = crate::spawn(async move {
            loop {
                match switcher::run(deps.clone(), &mut events_rx).await {
                    // OBS went away; the connection stops us.
                    Ok(()) => return,
                    Err(Error::SwitcherGone) => return,
                    Err(error) => tracing::warn!(%error, "Switcher stopped; restarting"),
                }
                tokio::time::sleep(SWITCHER_RESTART_DELAY).await;
            }
        });

        session.switcher = Some(RunningSwitcher { task, events });
    }

    fn stop_switcher(&mut self) {
        if let Some(running) = self.session.as_mut().and_then(|s| s.switcher.take()) {
            running.task.abort();
        }
    }

    async fn handle_control(&mut self, control: Control) -> Result<Flow> {
        match control {
            Control::TakenOver => {
                tracing::info!("A newer browser source took over this switcher");
                self.taken_over = true;
                self.refuse(RequestErrorKind::AlreadyConnected).await
            }
            Control::ConfigChanged => self.reload().await,
            Control::Reconnect => {
                tracing::info!("Closing so the browser source reconnects (asked by staff)");
                Ok(Flow::Close)
            }
            Control::AudioWanted => {
                if let Some(session) = self.session.as_mut() {
                    let change = session.audio.demand(Instant::now(), session.obs_connected);
                    if let Some(levels) = change {
                        self.reidentify(levels).await?;
                    }
                }
                Ok(Flow::Continue)
            }
        }
    }

    /// Applies settings saved in the dashboard.
    async fn reload(&mut self) -> Result<Flow> {
        let Some(session) = self.session.as_ref() else {
            return Ok(Flow::Continue);
        };
        let switcher_id = session.switcher_id;
        let switcher = db::switchers::get(&self.shared.db, switcher_id).await?;

        // Deleted, or its connect token was replaced: this link no longer works.
        let Some(switcher) = switcher.filter(|s| s.connect_token_hash == session.token_hash) else {
            tracing::info!(%switcher_id, "Switcher deleted or its link replaced; disconnecting");
            return self.refuse(RequestErrorKind::AuthenticationFailed).await;
        };

        let obs_settings = (switcher.port, switcher.password.clone());
        if obs_settings != session.obs_settings {
            self.send_request(RequestKind::Settings(self.settings_for(&switcher)), None)
                .await?;
        }

        // A plan change may pause or resume this switcher.
        let paused = db::switchers::is_paused(&self.shared.db, switcher_id).await?;
        if let Some(session) = self.session.as_mut() {
            if paused != session.paused {
                tracing::info!(%switcher_id, paused, "Switcher paused state changed");
            }

            // A new or removed remote key, or a pause: restart the BELABOX link
            // (before the switcher, which takes its handle).
            let want_belabox = !paused && switcher.belabox_remote_key.is_some();
            if switcher.belabox_remote_key != session.belabox_key
                || want_belabox != session.belabox.is_some()
            {
                if let Some(link) = session.belabox.take() {
                    link.stop(true).await;
                }
                if want_belabox {
                    session.belabox = start_belabox(&self.shared, &switcher);
                }
                session.belabox_key = switcher.belabox_remote_key.clone();
            }
            session.obs_settings = obs_settings;
            session.paused = paused;
            let obs_connected = session.obs_connected;
            session.status.update(|status| {
                status.auto_switch = switcher.bitrate_switcher_enabled;
                status.paused = paused;
            });
            if paused {
                self.stop_switcher();
                if let Some(session) = self.session.as_ref() {
                    session.status.update(|status| {
                        status.state = live::FeedState::Idle;
                        status.active_server_id = None;
                    });
                }
            } else if obs_connected {
                self.start_switcher();
            }
        }

        Ok(Flow::Continue)
    }

    async fn cleanup(&mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        if let Some(running) = session.switcher.take() {
            running.task.abort();
        }
        for task in session.tasks {
            task.abort();
        }
        if let Some(link) = session.belabox.take() {
            link.stop(!self.taken_over).await;
        }

        if !self.taken_over {
            let key = live::status_key(session.switcher_id);
            if let Err(error) = self.shared.stores.status.delete(&key).await {
                tracing::warn!(%error, "Failed to clear switcher status");
            }
        }
        if let Err(error) =
            db::switchers::touch_last_seen(&self.shared.db, session.switcher_id).await
        {
            tracing::warn!(%error, "Failed to update last seen");
        }
    }
}

/// Starts the BELABOX Cloud link, if the switcher has a remote key.
fn start_belabox(shared: &Shared, switcher: &db::switchers::Switcher) -> Option<belabox::Link> {
    let encrypted = switcher.belabox_remote_key.as_deref()?;
    let key = shared
        .data_key
        .decrypt(encrypted)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let Some(key) = key else {
        tracing::warn!(switcher_id = %switcher.id, "Couldn't decrypt the BELABOX remote key");
        return None;
    };

    Some(belabox::Link::start(belabox::Deps {
        switcher_id: switcher.id,
        url: shared.belabox_url.clone(),
        key,
        db: shared.db.clone(),
        nats: shared.nats.clone(),
        kv: shared.stores.belabox.clone(),
    }))
}

/// Relays OBS requests from the API (via NATS) to the browser source.
async fn forward_obs_requests(
    nats: async_nats::Client,
    switcher_id: Uuid,
    inner_comm: mpsc::UnboundedSender<InnerCommMessage>,
) {
    let mut requests = match nats.subscribe(live::obs_request_subject(switcher_id)).await {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to OBS requests");
            return;
        }
    };

    while let Some(message) = requests.next().await {
        let request = InnerCommMessage {
            span: obs_request_span(&message),
            message: String::from_utf8_lossy(&message.payload).into_owned(),
            reply: InnerReply::Nats(message.reply),
        };
        if inner_comm.send(request).is_err() {
            return;
        }
    }
}

/// A span for a relayed OBS request, in the sender's trace. Untraced requests (the
/// API's polled routes) get none.
fn obs_request_span(message: &async_nats::Message) -> Span {
    let span = telemetry::follow_span!(
        message.headers.as_ref(),
        "obs.request",
        otel.name = tracing::field::Empty,
        obs.request.type = tracing::field::Empty,
    );
    if !span.is_disabled() {
        let request = serde_json::from_slice::<serde_json::Value>(&message.payload).ok();
        let request_type = request
            .as_ref()
            .and_then(|request| request.pointer("/d/requestType")?.as_str())
            .unwrap_or("request");
        span.record("otel.name", format!("OBS {request_type}"));
        span.record("obs.request.type", request_type);
    }
    span
}

/// Remembers dashboard actions, so the OBS changes they cause are attributed.
async fn collect_intents(
    nats: async_nats::Client,
    switcher_id: Uuid,
    attribution: SharedAttribution,
) {
    let mut intents = match nats.subscribe(live::intent_subject(switcher_id)).await {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to intents");
            return;
        }
    };

    while let Some(message) = intents.next().await {
        match serde_json::from_slice::<live::Intent>(&message.payload) {
            Ok(intent) => attribution
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .add_intent(intent),
            Err(error) => tracing::warn!(%error, "Ignoring a malformed intent"),
        }
    }
}

/// Listens for config changes, takeovers, reconnect requests and audio demand
/// for this switcher.
async fn listen_for_control(
    nats: async_nats::Client,
    switcher_id: Uuid,
    connection_id: Uuid,
    control: mpsc::UnboundedSender<Control>,
) {
    let subscriptions = tokio::try_join!(
        nats.subscribe(live::config_subject(switcher_id)),
        nats.subscribe(live::takeover_subject(switcher_id)),
        nats.subscribe(live::audio_demand_subject(switcher_id)),
        nats.subscribe(live::reconnect_subject(switcher_id)),
    );
    let (mut config, mut takeover, mut audio_demand, mut reconnect) = match subscriptions {
        Ok(subscriptions) => subscriptions,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to switcher control messages");
            return;
        }
    };

    loop {
        let message = tokio::select! {
            Some(_) = config.next() => Control::ConfigChanged,
            Some(message) = takeover.next() => {
                match serde_json::from_slice::<Takeover>(&message.payload) {
                    Ok(other) if other.connection_id != connection_id => Control::TakenOver,
                    _ => continue,
                }
            }
            Some(_) = audio_demand.next() => Control::AudioWanted,
            Some(_) = reconnect.next() => Control::Reconnect,
            else => return,
        };
        if control.send(message).is_err() {
            return;
        }
    }
}

#[derive(Debug, Serialize)]
struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(flatten)]
    pub request: RequestKind,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum RequestKind {
    IdentifyAck(bool),
    RequestError(RequestError),
    Settings(Settings),
    ObsRawRequest(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RequestError {
    pub kind: RequestErrorKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum RequestErrorKind {
    AuthenticationFailed,
    AlreadyConnected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    pub obs: ObsSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ObsSettings {
    pub port: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Response {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(flatten)]
    pub response: ResponseKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ResponseKind {
    Identify(Identify),
    ObsWsConnected(bool),
    ObsRawResponse(serde_json::Value),
    ObsRawEvent(serde_json::Value),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Identify {
    pub token: String,
    pub version: String,
}
