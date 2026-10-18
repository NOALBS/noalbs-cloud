//! Moblin's remote control: a switcher's Moblin, set up as a remote control
//! *streamer* with the Assistant URL `…/moblin/<switcher ID>`, connects here and
//! proves it knows the switcher's Moblin password. We act as its *assistant*:
//! we poll its status and settings, publish a [`MoblinStatus`] to the KV bucket,
//! and carry out [`MoblinCommand`]s from NATS (the API, the chat service, and
//! the switcher starting or stopping Moblin with the OBS stream).
//!
//! Unlike the BELABOX link, this runs whether or not the browser source is
//! connected, possibly on another obs-link replica, so everything reaches it
//! over NATS. The newest Moblin for a switcher wins (a [`Takeover`]).

mod protocol;

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use async_nats::jetstream::kv;
use common::{crypto::DataKey, shutdown};
use futures_util::{SinkExt, StreamExt};
use live::{
    EventDetail, MoblinAction, MoblinCommand, MoblinRefusal, MoblinReply, MoblinStatus, Takeover,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
use tracing::Instrument;
use uuid::Uuid;

use self::protocol::{Change, Incoming, Request, State};
use crate::status;

/// How long Moblin has to identify after connecting.
const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(10);
/// Wait before refusing a wrong password, to slow down guessing.
const REFUSE_DELAY: Duration = Duration::from_secs(1);
/// Log a wrong password at most this often per switcher.
const WRONG_PASSWORD_LOG_INTERVAL: Duration = Duration::from_secs(600);
const STATUS_POLL: Duration = Duration::from_secs(2);
/// Moblin doesn't tell us when its scenes or presets change.
const SETTINGS_POLL: Duration = Duration::from_secs(30);
const PING_INTERVAL: Duration = Duration::from_secs(30);
/// Moblin pings every 30 s; nothing for this long means the phone is gone.
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// A change Moblin reports within this long of our command is "by" whoever
/// sent it.
const INTENT_TTL: Duration = Duration::from_secs(15);

/// When a wrong password was last logged, per switcher (this replica only).
static WRONG_PASSWORD_LOGGED: LazyLock<Mutex<HashMap<Uuid, Instant>>> =
    LazyLock::new(Mutex::default);

/// What a Moblin connection needs.
#[derive(Clone)]
pub struct Deps {
    pub db: db::DbPool,
    pub nats: async_nats::Client,
    pub kv: kv::Store,
    pub data_key: DataKey,
}

type Socket = WebSocketStream<TcpStream>;
type CommandRequest = (MoblinCommand, oneshot::Sender<MoblinReply>);

/// The switcher ID in a Moblin Assistant URL's path (`/moblin/<id>`), if the
/// path is one.
pub fn switcher_in_path(path: &str) -> Option<Option<Uuid>> {
    let rest = path.strip_prefix("/moblin")?;
    if !(rest.is_empty() || rest.starts_with('/')) {
        return None;
    }
    let id = rest.trim_matches('/');
    Some(Uuid::parse_str(id).ok())
}

/// Runs one Moblin connection until it closes.
pub async fn serve(
    mut socket: Socket,
    switcher_id: Option<Uuid>,
    deps: Deps,
    shutdown: shutdown::ShutdownHandler,
) {
    let Some(session) = identify(&mut socket, switcher_id, &deps).await else {
        let _ = socket.close(None).await;
        return;
    };
    let switcher_id = session.switcher_id;
    tracing::Span::current().record("switcher.id", tracing::field::display(switcher_id));
    tracing::info!("Moblin identified");

    let mut connection = Connection::start(socket, session, deps, shutdown).await;
    if let Err(error) = connection.run().await {
        tracing::info!(%error, "Moblin connection ended");
    }
    connection.cleanup().await;
}

struct Identified {
    switcher_id: Uuid,
    /// The encrypted password Moblin proved it knows, to notice a new one.
    password: Vec<u8>,
    paused: bool,
}

/// Sends the challenge and checks Moblin's answer; `None` if it didn't identify
/// (it was told so).
async fn identify(
    socket: &mut Socket,
    switcher_id: Option<Uuid>,
    deps: &Deps,
) -> Option<Identified> {
    let (challenge, salt) = (common::tokens::generate(), common::tokens::generate());
    socket
        .send(Message::Text(protocol::hello(&challenge, &salt).into()))
        .await
        .ok()?;

    let authentication = tokio::time::timeout(IDENTIFY_TIMEOUT, async {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Text(text) = message
                && let Some(Incoming::Identify { authentication }) = protocol::parse(&text)
            {
                return Some(authentication);
            }
        }
        None
    })
    .await
    .ok()
    .flatten()?;

    let switcher = match switcher_id {
        Some(id) => match db::switchers::get(&deps.db, id).await {
            Ok(switcher) => switcher,
            Err(error) => {
                tracing::warn!(%error, "Couldn't look up a Moblin's switcher");
                return None;
            }
        },
        None => None,
    };
    let known = switcher.as_ref().and_then(|switcher| {
        let encrypted = switcher.moblin_password.clone()?;
        let password = deps.data_key.decrypt(&encrypted).ok()?;
        Some((String::from_utf8(password).ok()?, encrypted))
    });

    let Some((_, encrypted)) = known.filter(|(password, _)| {
        protocol::hash_password(&challenge, &salt, password) == authentication
    }) else {
        tracing::info!(
            ?switcher_id,
            "Moblin sent a wrong password or an unknown switcher"
        );
        if let Some(switcher) = switcher.filter(|s| s.moblin_password.is_some()) {
            log_wrong_password(deps, switcher.id).await;
        }
        tokio::time::sleep(REFUSE_DELAY).await;
        let _ = socket
            .send(Message::Text(protocol::identified(false).into()))
            .await;
        return None;
    };
    let switcher_id = switcher.map(|s| s.id)?;

    let paused = match db::switchers::is_paused(&deps.db, switcher_id).await {
        Ok(paused) => paused,
        Err(error) => {
            tracing::warn!(%error, "Couldn't check whether a switcher is paused");
            return None;
        }
    };
    socket
        .send(Message::Text(protocol::identified(true).into()))
        .await
        .ok()?;

    Some(Identified {
        switcher_id,
        password: encrypted,
        paused,
    })
}

async fn log_wrong_password(deps: &Deps, switcher_id: Uuid) {
    {
        let mut logged = WRONG_PASSWORD_LOGGED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        logged.retain(|_, at| at.elapsed() < WRONG_PASSWORD_LOG_INTERVAL);
        if logged.contains_key(&switcher_id) {
            return;
        }
        logged.insert(switcher_id, Instant::now());
    }
    record(deps, switcher_id, EventDetail::MoblinWrongPassword).await;
}

async fn record(deps: &Deps, switcher_id: Uuid, detail: EventDetail) {
    if let Err(error) = live::record_event(&deps.db, &deps.nats, switcher_id, detail).await {
        tracing::warn!(%error, "Failed to record a Moblin event");
    }
}

/// News for a connection from NATS.
enum Control {
    ConfigChanged,
    TakenOver,
}

/// What a request we sent to Moblin was for.
enum Pending {
    Status,
    Settings,
    Command {
        reply: oneshot::Sender<MoblinReply>,
        /// What Moblin should then report, and who asked.
        expect: Option<Change>,
        by: String,
        /// Read the settings again once Moblin says it's done.
        refresh_settings: bool,
    },
}

struct Connection {
    id: Uuid,
    socket: Socket,
    session: Identified,
    deps: Deps,
    shutdown: shutdown::ShutdownHandler,
    state: State,
    status: watch::Sender<MoblinStatus>,
    commands: mpsc::Receiver<CommandRequest>,
    control: mpsc::UnboundedReceiver<Control>,
    tasks: Vec<JoinHandle<()>>,
    next_id: u64,
    pending: HashMap<u64, (Pending, Instant)>,
    /// Commands carried out, to say who caused the change Moblin then reports.
    intents: Vec<(Change, String, Instant)>,
    last_heard: Instant,
    taken_over: bool,
}

impl Connection {
    async fn start(
        socket: Socket,
        session: Identified,
        deps: Deps,
        shutdown: shutdown::ShutdownHandler,
    ) -> Self {
        let id = Uuid::new_v4();
        let switcher_id = session.switcher_id;
        let initial = MoblinStatus {
            updated_at: chrono::Utc::now(),
            paused: session.paused,
            ..MoblinStatus::default()
        };
        let (status, status_rx) = watch::channel(initial);
        let (commands_tx, commands) = mpsc::channel(16);
        let (control_tx, control) = mpsc::unbounded_channel();

        // Older connections for this switcher close when they see this.
        match serde_json::to_vec(&Takeover { connection_id: id }) {
            Ok(payload) => {
                if let Err(error) = deps
                    .nats
                    .publish(live::moblin_takeover_subject(switcher_id), payload.into())
                    .await
                {
                    tracing::warn!(%error, "Couldn't announce a Moblin takeover");
                }
            }
            Err(error) => tracing::warn!(%error, "Couldn't serialize a takeover"),
        }

        let tasks = vec![
            crate::spawn(status::publish(
                deps.kv.clone(),
                switcher_id,
                live::MOBLIN_HEARTBEAT,
                status_rx,
            )),
            crate::spawn(answer_requests(deps.nats.clone(), switcher_id, commands_tx)),
            crate::spawn(listen_for_control(
                deps.nats.clone(),
                switcher_id,
                id,
                control_tx,
            )),
        ];
        record(&deps, switcher_id, EventDetail::MoblinConnected).await;

        Self {
            id,
            socket,
            session,
            deps,
            shutdown,
            state: State::default(),
            status,
            commands,
            control,
            tasks,
            next_id: 0,
            pending: HashMap::new(),
            intents: Vec::new(),
            last_heard: Instant::now(),
            taken_over: false,
        }
    }

    fn switcher_id(&self) -> Uuid {
        self.session.switcher_id
    }

    async fn run(&mut self) -> Result<(), tokio_tungstenite::tungstenite::Error> {
        self.send_request(Request::GetSettings, Pending::Settings)
            .await?;
        self.send_request(Request::GetStatus, Pending::Status)
            .await?;

        let mut status_poll = tokio::time::interval(STATUS_POLL);
        let mut settings_poll = tokio::time::interval(SETTINGS_POLL);
        let mut ping = tokio::time::interval(PING_INTERVAL);
        status_poll.reset();
        settings_poll.reset();
        ping.reset();

        loop {
            tokio::select! {
                _ = self.shutdown.changed() => {
                    self.socket.close(None).await?;
                    return Ok(());
                }
                message = self.socket.next() => match message {
                    Some(Ok(Message::Text(text))) => {
                        self.last_heard = Instant::now();
                        self.handle_message(&text).await?;
                    }
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(_)) => self.last_heard = Instant::now(),
                    Some(Err(error)) => return Err(error),
                },
                Some((command, reply)) = self.commands.recv() => self.handle_command(command, reply).await?,
                Some(control) = self.control.recv() => {
                    if self.handle_control(control).await {
                        self.socket.close(None).await?;
                        return Ok(());
                    }
                }
                _ = status_poll.tick() => {
                    self.prune();
                    if self.last_heard.elapsed() > IDLE_TIMEOUT {
                        tracing::info!(switcher_id = %self.switcher_id(), "Moblin went quiet; closing");
                        self.socket.close(None).await?;
                        return Ok(());
                    }
                    self.send_request(Request::GetStatus, Pending::Status).await?;
                }
                _ = settings_poll.tick() => {
                    self.send_request(Request::GetSettings, Pending::Settings).await?;
                }
                _ = ping.tick() => self.socket.send(Message::Ping(Default::default())).await?,
            }
        }
    }

    async fn send_request(
        &mut self,
        request: Request,
        pending: Pending,
    ) -> Result<(), tokio_tungstenite::tungstenite::Error> {
        self.next_id += 1;
        let id = self.next_id;
        self.pending.insert(id, (pending, Instant::now()));
        self.socket
            .send(Message::Text(request.to_message(id).into()))
            .await
    }

    /// Drops requests Moblin never answered.
    fn prune(&mut self) {
        let expired = self
            .pending
            .iter()
            .filter(|(_, (_, sent))| sent.elapsed() > REQUEST_TIMEOUT)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in expired {
            if let Some((Pending::Command { reply, .. }, _)) = self.pending.remove(&id) {
                let _ = reply.send(Err(MoblinRefusal::Timeout));
            }
        }
        self.intents.retain(|(_, _, at)| at.elapsed() < INTENT_TTL);
    }

    async fn handle_message(
        &mut self,
        text: &str,
    ) -> Result<(), tokio_tungstenite::tungstenite::Error> {
        let Some(message) = protocol::parse(text) else {
            tracing::debug!("Ignoring a message from Moblin we don't understand");
            return Ok(());
        };
        match message {
            Incoming::Ping => {
                self.socket
                    .send(Message::Text(protocol::pong().into()))
                    .await?;
            }
            Incoming::Event(data) => {
                let changes = self.state.apply_event(&data);
                self.on_changes(changes).await;
            }
            Incoming::Response { id, ok, data } => match self.pending.remove(&id) {
                Some((Pending::Status, _)) => {
                    if let Some(data) = data {
                        let changes = self.state.apply_status(&data);
                        self.on_changes(changes).await;
                    }
                }
                Some((Pending::Settings, _)) => {
                    if let Some(data) = data {
                        let changes = self.state.apply_settings(&data);
                        self.on_changes(changes).await;
                    }
                }
                Some((
                    Pending::Command {
                        reply,
                        expect,
                        by,
                        refresh_settings,
                    },
                    _,
                )) => {
                    if ok {
                        if let Some(expect) = expect {
                            self.intents.push((expect, by, Instant::now()));
                        }
                        let _ = reply.send(Ok(()));
                        // Moblin doesn't push settings changes: read them back.
                        if refresh_settings {
                            self.send_request(Request::GetSettings, Pending::Settings)
                                .await?;
                        }
                    } else {
                        let _ = reply.send(Err(MoblinRefusal::Failed));
                    }
                }
                None => {}
            },
            Incoming::Identify { .. } => {
                tracing::debug!("Moblin identified twice; ignoring");
            }
            Incoming::Other(_) => {}
        }
        Ok(())
    }

    async fn handle_command(
        &mut self,
        command: MoblinCommand,
        reply: oneshot::Sender<MoblinReply>,
    ) -> Result<(), tokio_tungstenite::tungstenite::Error> {
        if self.session.paused {
            let _ = reply.send(Err(MoblinRefusal::Paused));
            return Ok(());
        }
        let request = match self.state.prepare(&command.action) {
            Ok(request) => request,
            Err(refusal) => {
                let _ = reply.send(Err(refusal));
                return Ok(());
            }
        };
        let expect = self.state.expected_change(&command.action);
        let refresh_settings = matches!(
            command.action,
            MoblinAction::Connection { .. } | MoblinAction::ConnectionPriorities { .. }
        );
        self.send_request(
            request,
            Pending::Command {
                reply,
                expect,
                by: command.by,
                refresh_settings,
            },
        )
        .await
    }

    fn take_intent(&mut self, change: &Change) -> Option<String> {
        self.intents.retain(|(_, _, at)| at.elapsed() < INTENT_TTL);
        let i = self.intents.iter().position(|(c, _, _)| c == change)?;
        Some(self.intents.remove(i).1)
    }

    async fn on_changes(&mut self, changes: Vec<Change>) {
        self.publish();
        for change in changes {
            let by = self.take_intent(&change);
            let detail = match change {
                Change::Streaming(true) => EventDetail::MoblinStreamStarted { by },
                Change::Streaming(false) => EventDetail::MoblinStreamStopped { by },
                Change::Recording(true) => EventDetail::MoblinRecordingStarted { by },
                Change::Recording(false) => EventDetail::MoblinRecordingStopped { by },
                Change::Muted(muted) => EventDetail::MoblinMuteChanged { muted, by },
                Change::Scene(scene) => EventDetail::MoblinSceneChanged { scene, by },
                Change::BitratePreset(kbps) => EventDetail::MoblinBitrateChanged { kbps, by },
                Change::ZoomPreset(name) => EventDetail::MoblinZoomPresetChanged { name, by },
                Change::Torch(on) => EventDetail::MoblinTorchChanged { on, by },
                Change::StealthMode(on) => EventDetail::MoblinStealthModeChanged { on, by },
                Change::ConnectionPriorities(enabled) => {
                    EventDetail::MoblinConnectionPrioritiesChanged { enabled, by }
                }
                Change::Connection {
                    name,
                    enabled,
                    priority,
                } => EventDetail::MoblinConnectionChanged {
                    name,
                    enabled,
                    priority,
                    by,
                },
            };
            record(&self.deps, self.switcher_id(), detail).await;
        }
    }

    /// Updates the status viewers see, if it changed.
    fn publish(&self) {
        let next = MoblinStatus {
            paused: self.session.paused,
            ..self.state.status()
        };
        self.status.send_if_modified(|current| {
            let next = MoblinStatus {
                updated_at: current.updated_at,
                ..next
            };
            let changed = *current != next;
            if changed {
                *current = next;
            }
            changed
        });
    }

    /// `true` if the connection should close.
    async fn handle_control(&mut self, control: Control) -> bool {
        match control {
            Control::TakenOver => {
                tracing::info!(switcher_id = %self.switcher_id(), "A newer Moblin took over");
                self.taken_over = true;
                true
            }
            Control::ConfigChanged => self.reload().await,
        }
    }

    /// Applies settings saved in the dashboard; `true` if Moblin must identify
    /// again (the switcher is gone or has a new password).
    async fn reload(&mut self) -> bool {
        let switcher_id = self.switcher_id();
        let switcher = match db::switchers::get(&self.deps.db, switcher_id).await {
            Ok(switcher) => switcher,
            Err(error) => {
                tracing::warn!(%error, "Couldn't reload a Moblin's switcher");
                return false;
            }
        };
        let Some(_) =
            switcher.filter(|s| s.moblin_password.as_ref() == Some(&self.session.password))
        else {
            tracing::info!(%switcher_id, "Moblin password changed or switcher deleted; disconnecting");
            return true;
        };
        match db::switchers::is_paused(&self.deps.db, switcher_id).await {
            Ok(paused) => {
                self.session.paused = paused;
                self.publish();
            }
            Err(error) => tracing::warn!(%error, "Couldn't check whether a switcher is paused"),
        }
        false
    }

    async fn cleanup(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        for (_, (pending, _)) in self.pending.drain() {
            if let Pending::Command { reply, .. } = pending {
                let _ = reply.send(Err(MoblinRefusal::NotConnected));
            }
        }
        if self.taken_over {
            return;
        }
        let key = live::status_key(self.switcher_id());
        if let Err(error) = self.deps.kv.delete(&key).await {
            tracing::warn!(%error, "Failed to clear the Moblin status");
        }
        record(
            &self.deps,
            self.switcher_id(),
            EventDetail::MoblinDisconnected,
        )
        .await;
        tracing::debug!(connection_id = %self.id, "Moblin connection cleaned up");
    }
}

/// Passes commands sent over NATS to the connection, and its answers back.
async fn answer_requests(
    nats: async_nats::Client,
    switcher_id: Uuid,
    commands: mpsc::Sender<CommandRequest>,
) {
    let mut requests = match nats
        .subscribe(live::moblin_request_subject(switcher_id))
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to Moblin requests");
            return;
        }
    };

    while let Some(message) = requests.next().await {
        let Some(reply) = message.reply else {
            continue;
        };
        let command = match serde_json::from_slice::<MoblinCommand>(&message.payload) {
            Ok(command) => command,
            Err(error) => {
                tracing::warn!(%error, "Ignoring a malformed Moblin request");
                continue;
            }
        };
        // Answered in the background, so a slow phone doesn't hold up others.
        let (answer_tx, answer) = oneshot::channel();
        if commands.send((command, answer_tx)).await.is_err() {
            return;
        }
        let nats = nats.clone();
        // Ends with the answer, so it shows how long the phone took.
        let span = telemetry::follow_span!(message.headers.as_ref(), "moblin.command");
        crate::spawn(
            async move {
                let answer: MoblinReply = answer.await.unwrap_or(Err(MoblinRefusal::NotConnected));
                match serde_json::to_vec(&answer) {
                    Ok(payload) => {
                        if let Err(error) = nats.publish(reply, payload.into()).await {
                            tracing::warn!(%error, "Failed to answer a Moblin request");
                        }
                    }
                    Err(error) => tracing::warn!(%error, "Failed to serialize a Moblin answer"),
                }
            }
            .instrument(span),
        );
    }
}

/// Listens for config changes and takeovers for this switcher's Moblin.
async fn listen_for_control(
    nats: async_nats::Client,
    switcher_id: Uuid,
    connection_id: Uuid,
    control: mpsc::UnboundedSender<Control>,
) {
    let subscriptions = tokio::try_join!(
        nats.subscribe(live::config_subject(switcher_id)),
        nats.subscribe(live::moblin_takeover_subject(switcher_id)),
    );
    let (mut config, mut takeover) = match subscriptions {
        Ok(subscriptions) => subscriptions,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to Moblin control messages");
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
            else => return,
        };
        if control.send(message).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::switcher_in_path;
    use uuid::Uuid;

    #[test]
    fn reads_the_switcher_from_the_path() {
        let id = Uuid::new_v4();
        assert_eq!(switcher_in_path(&format!("/moblin/{id}")), Some(Some(id)));
        assert_eq!(switcher_in_path(&format!("/moblin/{id}/")), Some(Some(id)));
        assert_eq!(
            switcher_in_path(&format!("/moblin/{}", id.to_string().to_uppercase())),
            Some(Some(id))
        );
        assert_eq!(switcher_in_path("/moblin/nope"), Some(None));
        assert_eq!(switcher_in_path("/link"), None);
        assert_eq!(switcher_in_path("/moblinx"), None);
        assert_eq!(switcher_in_path("/"), None);
    }
}
