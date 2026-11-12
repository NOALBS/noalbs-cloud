//! The WebSocket server that browser sources connect to, and one
//! [`Connection`] per browser source.
//!
//! A browser source identifies with its switcher's connect token. It then gets
//! the OBS websocket settings, connects to OBS, and relays requests and events.
//! While OBS is connected, the switcher task runs (see [`crate::switcher`]).

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use async_nats::jetstream;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{MAX_CONNECT_LOG_RAW_BYTES, crypto::DataKey, shutdown};
use flate2::read::GzDecoder;
use futures_util::{SinkExt, StreamExt};
use live::{EventDetail, PcObsState, PcStatus, SwitcherStatus, Takeover};
use obs_actions::types::{Event, ObsMessage, ObsRequest, Reidentify};
use opentelemetry::KeyValue;
use serde::{Deserialize, Serialize};
use std::io::Read;
use tokio::{
    net,
    sync::{Notify, mpsc, oneshot},
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
const CONNECT_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const UNSOLICITED_LOG_UPLOADS_PER_HOUR: usize = 10;
const REQUESTED_LOG_UPLOADS_PER_HOUR: usize = 30;
const MAX_DECOMPRESSED_LOG_BYTES: u64 = MAX_CONNECT_LOG_RAW_BYTES;
/// Delay before restarting a switcher that stopped with an error.
const SWITCHER_RESTART_DELAY: Duration = Duration::from_secs(5);
/// How long a client has to finish the WebSocket handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a browser source has to identify (it does so as soon as it connects).
const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(10);
/// How often to send a keepalive to a browser source.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
/// The largest message we take from a client. Replies go on over NATS, which
/// takes 1 MB at most anyway; this only has to fit the biggest OBS answer.
const MAX_MESSAGE_SIZE: usize = 4 << 20;

/// How often to try NATS at startup, [`NATS_CONNECT_RETRY_DELAY`] apart.
const NATS_CONNECT_ATTEMPTS: u32 = 5;
const NATS_CONNECT_RETRY_DELAY: Duration = Duration::from_secs(1);
/// What a connection reads at once. Nearly every message is far smaller, and
/// the default (128 KiB) is kept by every open connection.
const READ_BUFFER_SIZE: usize = 16 << 10;

/// Everything connections share.
struct Shared {
    nats: async_nats::Client,
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
    let nats_connected = Arc::new(Notify::new());
    let nats = connect_nats(&config, nats_connected.clone()).await?;
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

    let stores_keeper = tokio::spawn(keep_stores(js, nats_connected));

    let shared = Arc::new(Shared {
        nats,
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

    stores_keeper.abort();
    Ok(())
}

/// Connects to NATS. A pod that just started can be refused for a moment (its
/// network policy isn't in place yet), so a few tries before giving up.
/// `connected` hears of every (re)connection, see [`keep_stores`].
async fn connect_nats(config: &Config, connected: Arc<Notify>) -> Result<async_nats::Client> {
    let mut attempt = 1;
    loop {
        let connected = connected.clone();
        let options =
            live::connect_options(config.nats_user.as_deref(), config.nats_password.as_deref())
                .event_callback(move |event| {
                    let connected = connected.clone();
                    async move {
                        if matches!(event, async_nats::Event::Connected) {
                            connected.notify_one();
                        }
                    }
                });
        match options.connect(&config.nats_url).await {
            Ok(nats) => return Ok(nats),
            Err(error) if attempt < NATS_CONNECT_ATTEMPTS => {
                tracing::warn!(%error, attempt, "Couldn't connect to NATS; trying again");
                attempt += 1;
                tokio::time::sleep(NATS_CONNECT_RETRY_DELAY).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// The buckets and the stats stream live in NATS's memory, so a NATS that
/// restarted has none of them: writing a status fails, and every switcher shows
/// as offline, until they exist again. Makes them again whenever we're back.
async fn keep_stores(js: jetstream::Context, connected: Arc<Notify>) {
    loop {
        connected.notified().await;
        let mut delay = Duration::from_secs(1);
        // JetStream may take a moment longer than the connection.
        while let Err(error) = live::Stores::open(&js).await {
            tracing::warn!(%error, "Couldn't open the live stores after connecting to NATS; trying again");
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(30));
        }
        tracing::debug!("Live stores are in place");
    }
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
    // Requests are small and sent one by one: don't hold one back to fill a packet.
    if let Err(error) = stream.set_nodelay(true) {
        tracing::debug!(%error, "Couldn't turn off Nagle's algorithm");
    }
    let config = WebSocketConfig::default()
        .read_buffer_size(READ_BUFFER_SIZE)
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
    let counted = metrics::connection(kind);
    info!("New connection");

    if let Some(switcher_id) = moblin {
        let deps = moblin::Deps {
            db: shared.db.clone(),
            nats: shared.nats.clone(),
            kv: shared.stores.moblin.clone(),
            attempts: shared.stores.moblin_attempts.clone(),
            data_key: shared.data_key.clone(),
        };
        moblin::serve(websocket, switcher_id, deps, shutdown_handle).await;
        return;
    }

    let (inner_comm, inner_comm_rx) = mpsc::unbounded_channel();
    let (control_tx, control_rx) = mpsc::unbounded_channel();
    let mut connection = Connection {
        counted,
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
    /// The API saved settings or renamed a Connect device for this switcher.
    ConfigChanged(live::ConfigChange),
    /// Another connection identified for the same switcher.
    TakenOver,
    /// Someone watches the audio levels.
    AudioWanted,
    /// A reconnect request, optionally limited to one Connect device.
    Reconnect { device_id: Option<Uuid> },
    ConnectCommand {
        payload: Vec<u8>,
        reply: async_nats::Subject,
    },
}

enum Flow {
    Continue,
    Close,
}

fn reset_obs_status(status: &mut SwitcherStatus) {
    let (auto_switch, paused, pc) = (status.auto_switch, status.paused, status.pc.clone());
    *status = SwitcherStatus::connected(auto_switch);
    status.paused = paused;
    status.pc = pc;
}

fn set_pc_device_name(status: &mut SwitcherStatus, name: &str) -> bool {
    let Some(pc) = &mut status.pc else {
        return false;
    };
    if pc.device_name.as_deref() == Some(name) {
        return false;
    }
    pc.device_name = Some(name.to_owned());
    true
}

fn should_close_for_reconnect(
    requested_device_id: Option<Uuid>,
    session_device_id: Option<Uuid>,
) -> bool {
    requested_device_id.is_none() || requested_device_id == session_device_id
}

fn reconnect_device_id(payload: &[u8]) -> Option<Uuid> {
    serde_json::from_slice::<live::Reconnect>(payload)
        .unwrap_or(live::Reconnect { device_id: None })
        .device_id
}

fn parse_config_change(payload: &[u8]) -> live::ConfigChange {
    serde_json::from_slice(payload).unwrap_or_default()
}

fn should_refresh_device_name(renamed_device_id: Uuid, session_device_id: Option<Uuid>) -> bool {
    Some(renamed_device_id) == session_device_id
}

fn connect_command_failure(
    is_connect: bool,
    supports_control: bool,
    supports_logs: bool,
    supports_programs: bool,
    action: common::programs::ConnectCommandAction,
) -> Option<(live::ConnectObsCommandCode, &'static str)> {
    if !is_connect {
        Some((
            live::ConnectObsCommandCode::Offline,
            "This switcher connects through the browser source, which can't control OBS",
        ))
    } else if action.is_program() && !supports_programs {
        Some((
            live::ConnectObsCommandCode::Unsupported,
            "Update NOALBS Cloud Connect to manage programs from the dashboard",
        ))
    } else if action.is_log_upload() && !supports_logs {
        Some((
            live::ConnectObsCommandCode::Unsupported,
            "Update NOALBS Cloud Connect to upload OBS logs",
        ))
    } else if action.is_obs_control() && !supports_control {
        Some((
            live::ConnectObsCommandCode::Unsupported,
            "Update NOALBS Cloud Connect to control OBS from the dashboard",
        ))
    } else {
        None
    }
}

fn expired_connect_commands(
    pending: &mut HashMap<String, PendingConnectCommand>,
) -> Vec<async_nats::Subject> {
    let tokens = pending
        .iter()
        .filter(|(_, command)| command.sent.elapsed() >= CONNECT_COMMAND_TIMEOUT)
        .map(|(token, _)| token.clone())
        .collect::<Vec<_>>();
    tokens
        .into_iter()
        .filter_map(|token| pending.remove(&token).map(|command| command.reply))
        .collect()
}

fn allow_unsolicited_log_upload(uploads: &mut VecDeque<Instant>, now: Instant) -> bool {
    uploads.retain(|uploaded| now.saturating_duration_since(*uploaded) < Duration::from_secs(3600));
    if uploads.len() >= UNSOLICITED_LOG_UPLOADS_PER_HOUR {
        return false;
    }
    uploads.push_back(now);
    true
}

fn allow_requested_log_upload(uploads: &mut VecDeque<Instant>, now: Instant) -> bool {
    uploads.retain(|uploaded| now.saturating_duration_since(*uploaded) < Duration::from_secs(3600));
    if uploads.len() >= REQUESTED_LOG_UPLOADS_PER_HOUR {
        return false;
    }
    uploads.push_back(now);
    true
}

fn decode_log_upload(upload: &live::ConnectLogUpload) -> Result<Vec<u8>> {
    if upload.content_gzip_base64.len() > 3 * 1024 * 1024 {
        return Err(Error::InvalidLogUpload("too large"));
    }
    let compressed = STANDARD
        .decode(&upload.content_gzip_base64)
        .map_err(|_| Error::InvalidLogUpload("bad base64"))?;
    let mut content = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .take(MAX_DECOMPRESSED_LOG_BYTES + 1)
        .read_to_end(&mut content)
        .map_err(|_| Error::InvalidLogUpload("bad gzip"))?;
    if content.len() as u64 > MAX_DECOMPRESSED_LOG_BYTES {
        return Err(Error::InvalidLogUpload("decompressed too large"));
    }
    Ok(content)
}

fn invalid_log_upload_reply(reason: &'static str) -> live::ConnectObsCommandReply {
    live::ConnectObsCommandReply {
        ok: false,
        code: Some(live::ConnectObsCommandCode::Refused),
        error: Some(format!("Invalid log upload: {reason}")),
        log_id: None,
    }
}

/// A browser source that identified as a switcher.
struct Session {
    switcher_id: Uuid,
    token_hash: [u8; 32],
    device_id: Option<Uuid>,
    device_token_hash: Option<String>,
    /// The OBS settings last sent, to notice changes.
    obs_settings: (i32, Option<Vec<u8>>),
    is_connect: bool,
    connect_capabilities: Vec<String>,
    pending_connect_commands: HashMap<String, PendingConnectCommand>,
    unsolicited_log_uploads: VecDeque<Instant>,
    requested_log_uploads: VecDeque<Instant>,
    status: StatusHandle,
    attribution: SharedAttribution,
    /// See `switcher::Deps::paused_by_scene_control`.
    paused_by_scene_control: Arc<AtomicBool>,
    /// See `switcher::Deps::handover`.
    handover: Arc<std::sync::Mutex<Option<(Uuid, live::FeedState)>>>,
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

struct PendingConnectCommand {
    reply: async_nats::Subject,
    sent: Instant,
}

struct RunningSwitcher {
    task: JoinHandle<()>,
    events: mpsc::UnboundedSender<obs_actions::types::Event>,
}

struct Connection {
    counted: metrics::Gauge,
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
        let mut keepalive = tokio::time::interval_at(
            tokio::time::Instant::now() + KEEPALIVE_INTERVAL,
            KEEPALIVE_INTERVAL,
        );
        let identify_deadline = tokio::time::sleep(IDENTIFY_TIMEOUT);
        tokio::pin!(identify_deadline);
        let mut last_received = Instant::now();

        loop {
            tokio::select! {
                _ = self.shutdown_handle.changed() => {
                    tracing::info!("Shutdown signal received. Closing WebSocket connection.");
                    self.websocket.close(None).await?;
                    return Ok(());
                }
                message = self.websocket.next() => {
                    last_received = Instant::now();
                    match message {
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
                    }
                },
                _ = tokio::time::sleep_until((last_received + Duration::from_secs(60)).into()),
                    if self.session.as_ref().is_some_and(|session| {
                        expects_keepalive(session.is_connect, &session.connect_capabilities)
                    }) => {
                    tracing::info!("WebSocket keepalive timed out; closing connection");
                    return Ok(());
                }
                _ = keepalive.tick(), if self.session.as_ref().is_some_and(|session| !session.is_connect) => {
                    self.websocket.send(Message::Ping(Vec::new().into())).await?;
                    self.send_request(RequestKind::Keepalive(()), None).await?;
                }
                Some(message) = inner_comm_rx.recv() => self.forward_to_browser(message).await?,
                Some(control) = control_rx.recv() => {
                    if let Flow::Close = self.handle_control(control).await? {
                        return self.close().await;
                    }
                }
                _ = prune.tick() => {
                    self.prune_requests().await?;
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
    async fn prune_requests(&mut self) -> Result<()> {
        self.requests
            .retain(|_, pending| pending.sent.elapsed() < REQUEST_TIMEOUT);
        let expired = if let Some(session) = self.session.as_mut() {
            expired_connect_commands(&mut session.pending_connect_commands)
        } else {
            Vec::new()
        };
        for subject in expired {
            telemetry::nats::publish(
                &self.shared.nats,
                subject,
                serde_json::to_vec(&live::ConnectObsCommandReply {
                    ok: false,
                    code: Some(live::ConnectObsCommandCode::Timeout),
                    error: Some("Connect did not respond in time".into()),
                    log_id: None,
                })?,
            )
            .await?;
        }
        Ok(())
    }

    async fn store_log_upload(
        &mut self,
        upload: live::ConnectLogUpload,
        unsolicited: bool,
    ) -> Result<Uuid> {
        let session = self.session.as_mut().ok_or(Error::SwitcherGone)?;
        if unsolicited {
            if !session.is_connect
                || !session
                    .connect_capabilities
                    .iter()
                    .any(|capability| capability == "obs_logs")
            {
                return Err(Error::SwitcherGone);
            }
            if !allow_unsolicited_log_upload(&mut session.unsolicited_log_uploads, Instant::now()) {
                tracing::warn!("Dropping excess unsolicited Connect log upload");
                return Err(Error::SwitcherGone);
            }
        }
        let switcher_id = session.switcher_id;
        let device_id = session.device_id;
        let content = decode_log_upload(&upload)?;
        let id = db::connect_logs::insert(
            &self.shared.db,
            switcher_id,
            device_id,
            upload.kind,
            &upload.file_name,
            upload.modified_at,
            upload.truncated,
            &content,
        )
        .await?;
        if unsolicited {
            live::record_event(
                &self.shared.db,
                &self.shared.nats,
                switcher_id,
                EventDetail::ConnectLogUploaded {
                    kind: upload.kind,
                    log_id: id,
                    by: None,
                },
            )
            .await?;
        }
        Ok(id)
    }

    async fn handle_message(&mut self, message: &str) -> Result<Flow> {
        let response = serde_json::from_str::<Response>(message)?;

        match response.response {
            ResponseKind::Identify(identify) => return self.identify(&identify).await,
            ResponseKind::ObsWsConnected(connected) => self.obs_connection_changed(connected).await,
            ResponseKind::PcState { obs } => {
                if let Some(session) = self.session.as_ref() {
                    session.status.update(|status| {
                        if let Some(pc) = &mut status.pc {
                            pc.obs = obs;
                        }
                    });
                }
            }
            ResponseKind::ProgramsState(programs) => {
                if let Some(session) = self.session.as_ref() {
                    session.status.update(|status| {
                        if let Some(pc) = &mut status.pc {
                            pc.programs = programs;
                        }
                    });
                }
            }
            ResponseKind::ConnectCommandResult { ok, error, log } => {
                if let Some(token) = response.token
                    && let Some(pending) = self
                        .session
                        .as_mut()
                        .and_then(|session| session.pending_connect_commands.remove(&token))
                {
                    let reply = if ok {
                        let upload_result = match log {
                            Some(upload) => self.store_log_upload(upload, false).await.map(Some),
                            None => Ok(None),
                        };
                        match upload_result {
                            Ok(log_id) => live::ConnectObsCommandReply {
                                ok: true,
                                code: None,
                                error: None,
                                log_id,
                            },
                            Err(Error::InvalidLogUpload(reason)) => {
                                tracing::warn!(reason, "Connect returned an invalid log upload");
                                invalid_log_upload_reply(reason)
                            }
                            Err(error) => return Err(error),
                        }
                    } else {
                        live::ConnectObsCommandReply {
                            ok: false,
                            code: Some(live::ConnectObsCommandCode::Refused),
                            error: Some(
                                error.unwrap_or_else(|| "Connect refused the command".into()),
                            ),
                            log_id: None,
                        }
                    };
                    telemetry::nats::publish(
                        &self.shared.nats,
                        pending.reply,
                        serde_json::to_vec(&reply)?,
                    )
                    .await?;
                }
            }
            ResponseKind::LogUpload(upload) => {
                if let Err(error) = self.store_log_upload(upload, true).await {
                    tracing::warn!(%error, "Dropping unsolicited Connect log upload");
                }
            }
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

    async fn identify(&mut self, identify: &Identify) -> Result<Flow> {
        if self.session.is_some() {
            tracing::warn!("Browser source identified twice; ignoring");
            return Ok(Flow::Continue);
        }

        let token_hash = common::tokens::hash(&identify.token);
        let device_token_hash = common::tokens::hash_text(&identify.token);
        let (switcher, device, device_hash) = if let Some(switcher) =
            db::switchers::by_connect_token_hash(&self.shared.db, &token_hash).await?
        {
            (switcher, None, None)
        } else if let Some((device, switcher)) =
            db::connect::by_token_hash(&self.shared.db, &device_token_hash).await?
        {
            let os = identify
                .os
                .as_deref()
                .filter(|os| matches!(*os, "windows" | "linux" | "macos"))
                .unwrap_or(&device.os);
            if !db::connect::touch(&self.shared.db, device.id, os, &identify.version).await? {
                tracing::info!("Connect device was revoked during authentication");
                return self.refuse(RequestErrorKind::AuthenticationFailed).await;
            }
            (switcher, Some(device), Some(device_token_hash))
        } else {
            tracing::info!("Browser source used an unknown connect token");
            return self.refuse(RequestErrorKind::AuthenticationFailed).await;
        };
        let switcher_id = switcher.id;
        Span::current().record("switcher.id", tracing::field::display(switcher_id));
        tracing::info!("Browser source identified");

        self.send_request(RequestKind::IdentifyAck(true), None)
            .await?;
        let is_connect = identify.client.as_deref() == Some("connect");
        let client = if is_connect {
            "connect"
        } else {
            "browser_source"
        };
        Span::current().record("connection.kind", client);
        if is_connect {
            self.counted = metrics::connection(client);
        }
        self.send_request(
            RequestKind::Settings(self.settings_for(&switcher, is_connect)),
            None,
        )
        .await?;
        if !is_connect {
            self.send_request(RequestKind::Keepalive(()), None).await?;
        }
        db::switchers::touch_last_seen(&self.shared.db, switcher_id).await?;
        let paused = db::switchers::is_paused(&self.shared.db, switcher_id).await?;
        if paused {
            tracing::info!(%switcher_id, "Switcher is over its plan's limit; not switching");
        }

        let mut initial = SwitcherStatus::connected(switcher.bitrate_switcher_enabled);
        initial.paused = paused;
        if is_connect {
            initial.pc = Some(PcStatus {
                device_id: device.as_ref().map(|device| device.id),
                device_name: device.as_ref().map(|device| device.name.clone()),
                obs: PcObsState::Unknown,
                obs_version: identify.obs_version.clone(),
                programs: Vec::new(),
            });
        }
        let status = StatusHandle::new(initial);
        let attribution = SharedAttribution::default();
        let tasks = vec![
            crate::spawn(metrics::track_switcher(status.subscribe(), client)),
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
            crate::spawn(forward_connect_commands(
                self.shared.nats.clone(),
                switcher_id,
                self.control_tx.clone(),
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
            device_id: device.as_ref().map(|device| device.id),
            device_token_hash: device_hash,
            obs_settings: (switcher.port, switcher.password.clone()),
            is_connect,
            connect_capabilities: identify.capabilities.clone(),
            pending_connect_commands: HashMap::new(),
            unsolicited_log_uploads: VecDeque::new(),
            requested_log_uploads: VecDeque::new(),
            status,
            attribution,
            paused_by_scene_control: Arc::default(),
            handover: Arc::default(),
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

    fn settings_for(&self, switcher: &db::switchers::Switcher, is_connect: bool) -> Settings {
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
            connect: is_connect.then_some(ConnectSettings {
                watchdog: switcher.connect_watchdog,
            }),
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
                    reset_obs_status(status);
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
            reqwest: self.shared.reqwest.clone(),
            allow_private_stats_urls: self.shared.allow_private_stats_urls,
            status: session.status.clone(),
            inner_comm: self.inner_comm.clone(),
            attribution: session.attribution.clone(),
            paused_by_scene_control: session.paused_by_scene_control.clone(),
            handover: session.handover.clone(),
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
            Control::ConnectCommand { payload, reply } => {
                let (is_connect, supports_control, supports_logs, supports_programs) = self
                    .session
                    .as_ref()
                    .map(|session| {
                        (
                            session.is_connect,
                            session
                                .connect_capabilities
                                .iter()
                                .any(|value| value == "obs_control"),
                            session
                                .connect_capabilities
                                .iter()
                                .any(|value| value == "obs_logs"),
                            session
                                .connect_capabilities
                                .iter()
                                .any(|value| value == "programs"),
                        )
                    })
                    .unwrap_or_default();
                let request: live::ConnectObsCommandRequest = match serde_json::from_slice(&payload)
                {
                    Ok(request) => request,
                    Err(_) => {
                        let body = live::ConnectObsCommandReply {
                            ok: false,
                            code: Some(live::ConnectObsCommandCode::Refused),
                            error: Some("Invalid OBS command".into()),
                            log_id: None,
                        };
                        telemetry::nats::publish(
                            &self.shared.nats,
                            reply,
                            serde_json::to_vec(&body)?,
                        )
                        .await?;
                        return Ok(Flow::Continue);
                    }
                };
                if let Some((code, error)) = connect_command_failure(
                    is_connect,
                    supports_control,
                    supports_logs,
                    supports_programs,
                    request.action.clone(),
                ) {
                    telemetry::nats::publish(
                        &self.shared.nats,
                        reply,
                        serde_json::to_vec(&live::ConnectObsCommandReply {
                            ok: false,
                            code: Some(code),
                            error: Some(error.into()),
                            log_id: None,
                        })?,
                    )
                    .await?;
                    return Ok(Flow::Continue);
                }
                if request.action == common::programs::ConnectCommandAction::UploadLog {
                    let allowed = self.session.as_mut().is_some_and(|session| {
                        allow_requested_log_upload(
                            &mut session.requested_log_uploads,
                            Instant::now(),
                        )
                    });
                    if !allowed {
                        telemetry::nats::publish(
                            &self.shared.nats,
                            reply,
                            serde_json::to_vec(&live::ConnectObsCommandReply {
                                ok: false,
                                code: Some(live::ConnectObsCommandCode::Refused),
                                error: Some("Too many log requests; try again later".into()),
                                log_id: None,
                            })?,
                        )
                        .await?;
                        return Ok(Flow::Continue);
                    }
                }
                let token = Uuid::new_v4().to_string();
                self.send_request(
                    RequestKind::ConnectCommand(request.action),
                    Some(token.clone()),
                )
                .await?;
                if let Some(session) = self.session.as_mut() {
                    session.pending_connect_commands.insert(
                        token,
                        PendingConnectCommand {
                            reply,
                            sent: Instant::now(),
                        },
                    );
                }
                Ok(Flow::Continue)
            }
            Control::TakenOver => {
                tracing::info!("A newer browser source took over this switcher");
                self.taken_over = true;
                self.refuse(RequestErrorKind::AlreadyConnected).await
            }
            Control::ConfigChanged(change) => match change.renamed_device_id {
                Some(device_id) => {
                    let session_device_id =
                        self.session.as_ref().and_then(|session| session.device_id);
                    if should_refresh_device_name(device_id, session_device_id) {
                        self.refresh_device_name().await?;
                    }
                    Ok(Flow::Continue)
                }
                None => self.reload().await,
            },
            Control::Reconnect { device_id } => {
                let session_device_id = self.session.as_ref().and_then(|session| session.device_id);
                if should_close_for_reconnect(device_id, session_device_id) {
                    if let Some(device_id) = device_id {
                        tracing::info!(%device_id, "Closing the revoked Connect device connection");
                    } else {
                        tracing::info!("Closing so the browser source reconnects (asked by staff)");
                    }
                    Ok(Flow::Close)
                } else {
                    tracing::debug!(
                        ?device_id,
                        "Ignoring reconnect for a different Connect device"
                    );
                    Ok(Flow::Continue)
                }
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
        let is_connect = session.is_connect;
        let (switcher, device) = if let Some(token_hash) = session.device_token_hash.as_deref() {
            match db::connect::by_token_hash(&self.shared.db, token_hash)
                .await?
                .filter(|(device, switcher)| {
                    Some(device.id) == session.device_id && switcher.id == switcher_id
                }) {
                Some((device, switcher)) => (Some(switcher), Some(device)),
                None => (None, None),
            }
        } else {
            (
                db::switchers::get(&self.shared.db, switcher_id)
                    .await?
                    .filter(|switcher| switcher.connect_token_hash == session.token_hash),
                None,
            )
        };

        // Deleted or revoked, or its link was replaced: this session no longer works.
        let Some(switcher) = switcher else {
            tracing::info!(%switcher_id, "Switcher deleted or its link replaced; disconnecting");
            return self.refuse(RequestErrorKind::AuthenticationFailed).await;
        };

        if let Some(device) = device {
            session.status.update(|status| {
                set_pc_device_name(status, &device.name);
            });
        }

        let obs_settings = (switcher.port, switcher.password.clone());
        if obs_settings != session.obs_settings || is_connect {
            self.send_request(
                RequestKind::Settings(self.settings_for(&switcher, is_connect)),
                None,
            )
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
                        status.active_scene_rule_id = None;
                    });
                }
            } else if obs_connected {
                self.start_switcher();
            }
        }

        Ok(Flow::Continue)
    }

    async fn refresh_device_name(&self) -> Result<()> {
        let Some(session) = self.session.as_ref() else {
            return Ok(());
        };
        let Some(token_hash) = session.device_token_hash.as_deref() else {
            return Ok(());
        };
        let Some((device, switcher)) =
            db::connect::by_token_hash(&self.shared.db, token_hash).await?
        else {
            return Ok(());
        };
        if Some(device.id) != session.device_id || switcher.id != session.switcher_id {
            return Ok(());
        }

        session.status.update(|status| {
            set_pc_device_name(status, &device.name);
        });
        Ok(())
    }

    async fn cleanup(&mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        for (_, pending) in session.pending_connect_commands.drain() {
            let reply = live::ConnectObsCommandReply {
                ok: false,
                code: Some(live::ConnectObsCommandCode::Offline),
                error: Some("Connect computer went offline".into()),
                log_id: None,
            };
            if let Ok(payload) = serde_json::to_vec(&reply)
                && let Err(error) =
                    telemetry::nats::publish(&self.shared.nats, pending.reply, payload).await
            {
                tracing::warn!(%error, "Failed to reply to a pending Connect command");
            }
        }
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

fn expects_keepalive(is_connect: bool, capabilities: &[String]) -> bool {
    !is_connect || capabilities.iter().any(|cap| cap == "keepalive")
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

async fn forward_connect_commands(
    nats: async_nats::Client,
    switcher_id: Uuid,
    control: mpsc::UnboundedSender<Control>,
) {
    let mut requests = match nats
        .subscribe(live::connect_request_subject(switcher_id))
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to Connect OBS commands");
            return;
        }
    };

    while let Some(message) = requests.next().await {
        let Some(reply) = message.reply else {
            continue;
        };
        if control
            .send(Control::ConnectCommand {
                payload: message.payload.to_vec(),
                reply,
            })
            .is_err()
        {
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
            Some(message) = config.next() => {
                Control::ConfigChanged(parse_config_change(&message.payload))
            }
            Some(message) = takeover.next() => {
                match serde_json::from_slice::<Takeover>(&message.payload) {
                    Ok(other) if other.connection_id != connection_id => Control::TakenOver,
                    _ => continue,
                }
            }
            Some(_) = audio_demand.next() => Control::AudioWanted,
            Some(message) = reconnect.next() => Control::Reconnect {
                device_id: reconnect_device_id(&message.payload),
            },
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
    Keepalive(()),
    RequestError(RequestError),
    Settings(Settings),
    ObsRawRequest(String),
    ConnectCommand(common::programs::ConnectCommandAction),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect: Option<ConnectSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectSettings {
    pub watchdog: db::connect::Watchdog,
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
    PcState {
        obs: PcObsState,
    },
    ProgramsState(Vec<common::programs::ProgramStatus>),
    ObsRawResponse(serde_json::Value),
    ConnectCommandResult {
        ok: bool,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        log: Option<live::ConnectLogUpload>,
    },
    LogUpload(live::ConnectLogUpload),
    ObsRawEvent(serde_json::Value),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identify {
    pub token: String,
    pub version: String,
    #[serde(default)]
    pub client: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub os: Option<String>,
    #[serde(default)]
    pub obs_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        io::Write,
        time::{Duration, Instant},
    };

    use base64::{Engine, engine::general_purpose::STANDARD};
    use flate2::{Compression, write::GzEncoder};
    use live::{PcObsState, PcStatus, SwitcherStatus};
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        CONNECT_COMMAND_TIMEOUT, ConnectSettings, Identify, MAX_DECOMPRESSED_LOG_BYTES,
        ObsSettings, PendingConnectCommand, REQUESTED_LOG_UPLOADS_PER_HOUR, Request, RequestKind,
        Response, ResponseKind, Settings, UNSOLICITED_LOG_UPLOADS_PER_HOUR,
        allow_requested_log_upload, allow_unsolicited_log_upload, connect_command_failure,
        decode_log_upload, expired_connect_commands, invalid_log_upload_reply, parse_config_change,
        reconnect_device_id, reset_obs_status, set_pc_device_name, should_close_for_reconnect,
        should_refresh_device_name,
    };

    #[test]
    fn programs_state_response_matches_connect_wire_shape() {
        let response = serde_json::from_value::<Response>(json!({"programsState": []})).unwrap();

        assert!(matches!(
            response.response,
            ResponseKind::ProgramsState(programs) if programs.is_empty()
        ));
    }

    #[test]
    fn browser_sources_and_capable_connect_sessions_require_keepalive() {
        assert!(super::expects_keepalive(true, &["keepalive".into()]));
        assert!(!super::expects_keepalive(true, &[]));
        assert!(!super::expects_keepalive(true, &["obs_control".into()]));
        assert!(super::expects_keepalive(false, &["keepalive".into()]));
        assert!(super::expects_keepalive(false, &[]));
    }

    #[test]
    fn keepalive_request_matches_wire_shape() {
        let request = Request {
            token: None,
            request: RequestKind::Keepalive(()),
        };

        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"keepalive":null}"#
        );
    }

    #[test]
    fn browser_settings_wire_shape_is_unchanged_and_connect_gets_watchdog() {
        let browser = serde_json::to_value(Settings {
            obs: ObsSettings {
                port: 4455,
                password: None,
            },
            connect: None,
        })
        .unwrap();
        assert_eq!(browser, json!({"obs": {"port": 4455}}));

        let connect = serde_json::to_value(Settings {
            obs: ObsSettings {
                port: 4455,
                password: None,
            },
            connect: Some(ConnectSettings {
                watchdog: db::connect::Watchdog::RestartResume,
            }),
        })
        .unwrap();
        assert_eq!(
            connect,
            json!({"obs": {"port": 4455}, "connect": {"watchdog": "restart_resume"}})
        );
    }

    #[test]
    fn reconnects_only_close_the_target_device_session() {
        let device_id = Uuid::from_u128(1);
        let other_device_id = Uuid::from_u128(2);

        assert!(should_close_for_reconnect(None, None));
        assert!(should_close_for_reconnect(None, Some(other_device_id)));
        assert!(should_close_for_reconnect(Some(device_id), Some(device_id)));
        assert!(!should_close_for_reconnect(
            Some(device_id),
            Some(other_device_id)
        ));
        assert!(!should_close_for_reconnect(Some(device_id), None));
    }

    #[test]
    fn empty_or_invalid_config_payload_means_settings_changed() {
        assert_eq!(parse_config_change(b""), live::ConfigChange::default());
        assert_eq!(
            parse_config_change(b"not-json"),
            live::ConfigChange::default()
        );
    }

    #[test]
    fn config_change_payload_identifies_a_renamed_device() {
        let device_id = Uuid::from_u128(1);
        let payload = serde_json::to_vec(&live::ConfigChange {
            renamed_device_id: Some(device_id),
        })
        .unwrap();

        assert_eq!(
            parse_config_change(&payload),
            live::ConfigChange {
                renamed_device_id: Some(device_id),
            }
        );
    }

    #[test]
    fn only_the_matching_connect_device_is_refreshed() {
        let device_id = Uuid::from_u128(1);
        let other_device_id = Uuid::from_u128(2);

        assert!(should_refresh_device_name(device_id, Some(device_id)));
        assert!(!should_refresh_device_name(
            device_id,
            Some(other_device_id)
        ));
        assert!(!should_refresh_device_name(device_id, None));
    }

    #[test]
    fn empty_or_invalid_reconnect_payload_targets_any_session() {
        assert_eq!(reconnect_device_id(b""), None);
        assert_eq!(reconnect_device_id(b"not-json"), None);

        let device_id = Uuid::from_u128(1);
        let payload = serde_json::to_vec(&live::Reconnect {
            device_id: Some(device_id),
        })
        .unwrap();
        assert_eq!(reconnect_device_id(&payload), Some(device_id));
    }

    #[test]
    fn old_browser_source_identify_messages_still_parse() {
        let response: Response = serde_json::from_value(json!({
            "identify": { "token": "browser-token", "version": "1.0" }
        }))
        .unwrap();
        let ResponseKind::Identify(identify) = response.response else {
            panic!("identify message expected");
        };
        assert_eq!(identify.token, "browser-token");
        assert_eq!(identify.version, "1.0");
        assert_eq!(identify.client, None);
        assert!(identify.capabilities.is_empty());
        assert_eq!(identify.os, None);
        assert_eq!(identify.obs_version, None);
    }

    #[test]
    fn connect_identify_fields_are_camel_case_and_pc_state_parses() {
        let identify: Identify = serde_json::from_value(json!({
            "token": "device-token",
            "version": "2.0",
            "client": "connect",
            "capabilities": ["obs"],
            "os": "linux",
            "obsVersion": "31.0"
        }))
        .unwrap();
        assert_eq!(identify.obs_version.as_deref(), Some("31.0"));
        let response: Response = serde_json::from_value(json!({
            "pcState": { "obs": "crashed" }
        }))
        .unwrap();
        assert!(matches!(
            response.response,
            ResponseKind::PcState {
                obs: PcObsState::Crashed
            }
        ));
    }

    #[test]
    fn resetting_obs_status_preserves_connect_pc_status() {
        let mut status = SwitcherStatus::connected(true);
        status.paused = true;
        status.pc = Some(PcStatus {
            device_id: Some(Uuid::nil()),
            device_name: Some("Studio PC".into()),
            obs: PcObsState::NotResponding,
            obs_version: Some("31.0".into()),
            programs: Vec::new(),
        });

        reset_obs_status(&mut status);

        assert!(status.auto_switch);
        assert!(status.paused);
        assert_eq!(
            status.pc,
            Some(PcStatus {
                device_id: Some(Uuid::nil()),
                device_name: Some("Studio PC".into()),
                obs: PcObsState::NotResponding,
                obs_version: Some("31.0".into()),
                programs: Vec::new(),
            })
        );
    }

    #[test]
    fn set_pc_device_name_updates_the_connect_status() {
        let mut status = SwitcherStatus::connected(true);
        status.pc = Some(PcStatus {
            device_id: Some(Uuid::nil()),
            device_name: Some("Old PC name".into()),
            obs: PcObsState::Unknown,
            obs_version: None,
            programs: Vec::new(),
        });

        assert!(set_pc_device_name(&mut status, "New PC name"));
        assert_eq!(
            status.pc.as_ref().and_then(|pc| pc.device_name.as_deref()),
            Some("New PC name")
        );
    }

    #[test]
    fn set_pc_device_name_returns_false_when_the_name_is_unchanged() {
        let mut status = SwitcherStatus::connected(true);
        status.pc = Some(PcStatus {
            device_id: Some(Uuid::nil()),
            device_name: Some("Studio PC".into()),
            obs: PcObsState::Unknown,
            obs_version: None,
            programs: Vec::new(),
        });
        let before = status.clone();

        assert!(!set_pc_device_name(&mut status, "Studio PC"));
        assert_eq!(status, before);
    }

    #[test]
    fn set_pc_device_name_is_a_noop_without_a_connect_pc() {
        let mut status = SwitcherStatus::connected(true);
        let before = status.clone();

        assert!(!set_pc_device_name(&mut status, "Studio PC"));
        assert_eq!(status, before);
    }

    #[test]
    fn browser_source_commands_are_offline_and_old_connects_are_unsupported() {
        use common::programs::ConnectCommandAction as Action;

        let browser = connect_command_failure(false, false, false, false, Action::Start).unwrap();
        assert_eq!(browser.0, live::ConnectObsCommandCode::Offline);
        assert_eq!(
            browser.1,
            "This switcher connects through the browser source, which can't control OBS"
        );
        let old_connect =
            connect_command_failure(true, false, false, false, Action::Start).unwrap();
        assert_eq!(old_connect.0, live::ConnectObsCommandCode::Unsupported);
        assert_eq!(
            old_connect.1,
            "Update NOALBS Cloud Connect to control OBS from the dashboard"
        );
        assert!(connect_command_failure(true, true, false, false, Action::Restart).is_none());
        assert_eq!(
            connect_command_failure(true, true, false, false, Action::UploadLog)
                .unwrap()
                .0,
            live::ConnectObsCommandCode::Unsupported
        );
        assert!(connect_command_failure(true, false, true, false, Action::UploadLog).is_none());
        assert_eq!(
            connect_command_failure(
                true,
                true,
                true,
                false,
                Action::ProgramStart { id: Uuid::nil() }
            )
            .unwrap()
            .0,
            live::ConnectObsCommandCode::Unsupported
        );
        assert!(
            connect_command_failure(
                true,
                true,
                true,
                true,
                Action::ProgramStart { id: Uuid::nil() }
            )
            .is_none()
        );
        assert_eq!(
            connect_command_failure(
                false,
                false,
                false,
                true,
                Action::ProgramStart { id: Uuid::nil() }
            )
            .unwrap()
            .0,
            live::ConnectObsCommandCode::Offline
        );
    }

    #[test]
    fn unsolicited_log_uploads_are_limited_to_ten_per_hour() {
        let now = Instant::now();
        let mut uploads = VecDeque::new();
        for index in 0..UNSOLICITED_LOG_UPLOADS_PER_HOUR {
            assert!(allow_unsolicited_log_upload(
                &mut uploads,
                now + Duration::from_secs(index as u64)
            ));
        }
        assert!(!allow_unsolicited_log_upload(
            &mut uploads,
            now + Duration::from_secs(20)
        ));
        assert!(allow_unsolicited_log_upload(
            &mut uploads,
            now + Duration::from_secs(3601)
        ));
    }

    #[test]
    fn requested_log_uploads_are_limited_to_thirty_per_hour() {
        let now = Instant::now();
        let mut uploads = VecDeque::new();
        for index in 0..REQUESTED_LOG_UPLOADS_PER_HOUR {
            assert!(allow_requested_log_upload(
                &mut uploads,
                now + Duration::from_secs(index as u64)
            ));
        }
        assert!(!allow_requested_log_upload(
            &mut uploads,
            now + Duration::from_secs(40)
        ));
        assert!(allow_requested_log_upload(
            &mut uploads,
            now + Duration::from_secs(3601)
        ));
    }

    #[test]
    fn connect_log_upload_is_decoded_before_storage() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"OBS log header\n").unwrap();
        let upload = connect_log_upload(STANDARD.encode(encoder.finish().unwrap()));
        assert_eq!(decode_log_upload(&upload).unwrap(), b"OBS log header\n");
    }

    #[test]
    fn connect_log_upload_accepts_the_shared_raw_limit() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&vec![0; MAX_DECOMPRESSED_LOG_BYTES as usize])
            .unwrap();
        let upload = connect_log_upload(STANDARD.encode(encoder.finish().unwrap()));
        assert_eq!(
            decode_log_upload(&upload).unwrap().len(),
            MAX_DECOMPRESSED_LOG_BYTES as usize
        );
    }

    #[test]
    fn connect_log_upload_rejects_oversized_base64() {
        let upload = connect_log_upload("A".repeat(3 * 1024 * 1024 + 1));
        assert_eq!(
            decode_log_upload(&upload).unwrap_err().to_string(),
            "Invalid Connect log upload: too large"
        );
    }

    #[test]
    fn connect_log_upload_rejects_invalid_base64() {
        let upload = connect_log_upload("not base64!".into());
        assert_eq!(
            decode_log_upload(&upload).unwrap_err().to_string(),
            "Invalid Connect log upload: bad base64"
        );
    }

    #[test]
    fn connect_log_upload_rejects_invalid_gzip() {
        let upload = connect_log_upload(STANDARD.encode(b"not a gzip stream"));
        assert_eq!(
            decode_log_upload(&upload).unwrap_err().to_string(),
            "Invalid Connect log upload: bad gzip"
        );
    }

    #[test]
    fn connect_log_upload_rejects_excessive_decompressed_content() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&vec![0; MAX_DECOMPRESSED_LOG_BYTES as usize + 1])
            .unwrap();
        let upload = connect_log_upload(STANDARD.encode(encoder.finish().unwrap()));
        assert_eq!(
            decode_log_upload(&upload).unwrap_err().to_string(),
            "Invalid Connect log upload: decompressed too large"
        );
    }

    #[test]
    fn invalid_log_upload_reply_refuses_the_requested_upload() {
        let reply = invalid_log_upload_reply("bad gzip");
        assert!(!reply.ok);
        assert_eq!(reply.code, Some(live::ConnectObsCommandCode::Refused));
        assert_eq!(reply.error.as_deref(), Some("Invalid log upload: bad gzip"));
        assert_eq!(reply.log_id, None);
    }

    fn connect_log_upload(content_gzip_base64: String) -> live::ConnectLogUpload {
        live::ConnectLogUpload {
            kind: db::connect_logs::ConnectLogKind::ObsLog,
            file_name: "latest.txt".into(),
            modified_at: None,
            truncated: false,
            content_gzip_base64,
        }
    }

    #[test]
    fn supported_connect_command_uses_the_expected_websocket_shapes() {
        let request = Request {
            token: Some("command-token".into()),
            request: RequestKind::ConnectCommand(common::programs::ConnectCommandAction::Restart),
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "token": "command-token",
                "connectCommand": { "action": "restart" }
            })
        );
        let response: Response = serde_json::from_value(json!({
            "token": "command-token",
            "connectCommandResult": { "ok": false, "error": "OBS is busy" }
        }))
        .unwrap();
        assert_eq!(response.token.as_deref(), Some("command-token"));
        assert!(matches!(
            response.response,
            ResponseKind::ConnectCommandResult {
                ok: false,
                error: Some(error),
                ..
            } if error == "OBS is busy"
        ));
    }

    #[test]
    fn program_connect_command_keeps_its_fields_in_the_wire_shape() {
        let id = Uuid::nil();
        let request = Request {
            token: Some("command-token".into()),
            request: RequestKind::ConnectCommand(
                common::programs::ConnectCommandAction::ProgramStart { id },
            ),
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "token": "command-token",
                "connectCommand": { "action": "program_start", "id": id }
            })
        );
    }

    #[test]
    fn pending_connect_commands_expire_after_thirty_seconds() {
        let mut pending = HashMap::from([(
            "token".into(),
            PendingConnectCommand {
                reply: async_nats::Subject::from("inbox.reply"),
                sent: Instant::now() - CONNECT_COMMAND_TIMEOUT - Duration::from_secs(1),
            },
        )]);
        assert_eq!(
            expired_connect_commands(&mut pending),
            [async_nats::Subject::from("inbox.reply")]
        );
        assert!(pending.is_empty());
    }
}
