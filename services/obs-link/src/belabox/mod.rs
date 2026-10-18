//! The link to a switcher's BELABOX through BELABOX Cloud, for switchers with a
//! remote key. It runs with the browser source's connection (not while the
//! switcher is paused): a client keeping a remote connection open (like
//! belabot), a publisher writing its [`BelaboxStatus`] to the KV bucket, and a
//! responder carrying out [`BelaboxCommand`]s from the API and the chat service.
//! The switcher uses its [`Handle`] to start and stop BELABOX with the OBS stream.

mod protocol;

use std::time::{Duration, Instant};

use async_nats::jetstream::kv;
use futures_util::{SinkExt, StreamExt};
use live::{
    BelaboxAction, BelaboxChange, BelaboxCommand, BelaboxDetailsReply, BelaboxLink, BelaboxRefusal,
    BelaboxReply, BelaboxStatus, EventDetail,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::timeout,
};
use tokio_tungstenite::tungstenite::Message;
use tracing::Instrument;
use uuid::Uuid;

use self::protocol::{Change, State};
use crate::status;

/// belabot's keepalive interval.
const KEEPALIVE: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const BACKOFF_MIN: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(32);
/// A change the BELABOX reports within this long of our command is "by" whoever
/// sent it.
const INTENT_TTL: Duration = Duration::from_secs(15);

/// What a link needs.
pub struct Deps {
    pub switcher_id: Uuid,
    /// BELABOX Cloud's remote endpoint.
    pub url: String,
    /// The decrypted remote key.
    pub key: String,
    pub db: db::DbPool,
    pub nats: async_nats::Client,
    pub kv: kv::Store,
}

enum Request {
    Command(BelaboxCommand, oneshot::Sender<BelaboxReply>),
    Details(oneshot::Sender<BelaboxDetailsReply>),
}

impl Request {
    /// Answers that the link can't do it now.
    fn refuse(self, refusal: BelaboxRefusal) {
        match self {
            Self::Command(_, reply) => {
                let _ = reply.send(Err(refusal));
            }
            Self::Details(reply) => {
                let _ = reply.send(Err(refusal));
            }
        }
    }
}

/// Sends commands to a running link and reads its status.
#[derive(Clone)]
pub struct Handle {
    commands: mpsc::Sender<Request>,
    status: watch::Receiver<BelaboxStatus>,
}

impl Handle {
    pub async fn send(&self, command: BelaboxCommand) -> BelaboxReply {
        let (reply, answer) = oneshot::channel();
        if self
            .commands
            .send(Request::Command(command, reply))
            .await
            .is_err()
        {
            return Err(BelaboxRefusal::Connecting);
        }
        answer.await.unwrap_or(Err(BelaboxRefusal::Connecting))
    }

    pub async fn details(&self) -> BelaboxDetailsReply {
        let (reply, answer) = oneshot::channel();
        if self.commands.send(Request::Details(reply)).await.is_err() {
            return Err(BelaboxRefusal::Connecting);
        }
        answer.await.unwrap_or(Err(BelaboxRefusal::Connecting))
    }

    pub fn streaming(&self) -> bool {
        self.status.borrow().streaming
    }
}

/// A running link; dropping it stops it (use [`Link::stop`] to also clear its
/// status).
pub struct Link {
    handle: Handle,
    tasks: Vec<JoinHandle<()>>,
    switcher_id: Uuid,
    kv: kv::Store,
}

impl Link {
    pub fn start(deps: Deps) -> Self {
        let (status_tx, status_rx) = watch::channel(BelaboxStatus::new(BelaboxLink::Connecting));
        let (commands, commands_rx) = mpsc::channel(16);
        let handle = Handle {
            commands,
            status: status_rx.clone(),
        };
        let switcher_id = deps.switcher_id;
        let kv = deps.kv.clone();

        let tasks = vec![
            crate::spawn(status::publish(
                deps.kv.clone(),
                switcher_id,
                live::BELABOX_HEARTBEAT,
                status_rx,
            )),
            crate::spawn(answer_requests(
                deps.nats.clone(),
                switcher_id,
                handle.clone(),
            )),
            crate::spawn(run(deps, status_tx, commands_rx)),
        ];

        Self {
            handle,
            tasks,
            switcher_id,
            kv,
        }
    }

    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }

    /// Stops the link; `clear` also deletes its status (not when another
    /// connection took the switcher over).
    pub async fn stop(self, clear: bool) {
        for task in &self.tasks {
            task.abort();
        }
        if clear && let Err(error) = self.kv.delete(live::status_key(self.switcher_id)).await {
            tracing::warn!(%error, "Failed to clear the BELABOX status");
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Carries out commands sent over NATS and answers requests for the details.
async fn answer_requests(nats: async_nats::Client, switcher_id: Uuid, handle: Handle) {
    let (mut requests, mut details) = match tokio::try_join!(
        nats.subscribe(live::belabox_request_subject(switcher_id)),
        nats.subscribe(live::belabox_details_subject(switcher_id)),
    ) {
        Ok(subscriptions) => subscriptions,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to BELABOX requests");
            return;
        }
    };

    loop {
        tokio::select! {
            Some(message) = requests.next() => {
                let Some(reply) = message.reply else {
                    continue;
                };
                let command = match serde_json::from_slice::<BelaboxCommand>(&message.payload) {
                    Ok(command) => command,
                    Err(error) => {
                        tracing::warn!(%error, "Ignoring a malformed BELABOX request");
                        continue;
                    }
                };
                let span = telemetry::follow_span!(message.headers.as_ref(), "belabox.command");
                async { respond(&nats, reply, &handle.send(command).await).await }
                    .instrument(span)
                    .await;
            }
            Some(message) = details.next() => {
                if let Some(reply) = message.reply {
                    let span = telemetry::follow_span!(message.headers.as_ref(), "belabox.details");
                    async { respond(&nats, reply, &handle.details().await).await }
                        .instrument(span)
                        .await;
                }
            }
            else => return,
        }
    }
}

async fn respond<T: serde::Serialize>(
    nats: &async_nats::Client,
    reply: async_nats::Subject,
    answer: &T,
) {
    match serde_json::to_vec(answer) {
        Ok(payload) => {
            if let Err(error) = nats.publish(reply, payload.into()).await {
                tracing::warn!(%error, "Failed to answer a BELABOX request");
            }
        }
        Err(error) => tracing::warn!(%error, "Failed to serialize a BELABOX answer"),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Intent {
    Stream(bool),
    Interface(String, bool),
}

struct Client {
    deps: Deps,
    state: State,
    status: watch::Sender<BelaboxStatus>,
    /// Commands we sent, to say who caused the change BELABOX then reports.
    intents: Vec<(Intent, String, Instant)>,
}

/// Keeps a connection to BELABOX Cloud open until the link stops, or until
/// BELABOX Cloud refuses the key (then it waits for the settings to change).
async fn run(
    deps: Deps,
    status: watch::Sender<BelaboxStatus>,
    mut commands: mpsc::Receiver<Request>,
) {
    let mut client = Client {
        deps,
        state: State::default(),
        status,
        intents: Vec::new(),
    };
    let mut backoff = BACKOFF_MIN;

    loop {
        client.publish();
        match timeout(
            CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async(client.deps.url.as_str()),
        )
        .await
        {
            Ok(Ok((socket, _))) => {
                if client.session(socket, &mut commands).await {
                    backoff = BACKOFF_MIN;
                }
            }
            Ok(Err(error)) => tracing::warn!(%error, "Couldn't connect to BELABOX Cloud"),
            Err(_) => tracing::warn!("Timed out connecting to BELABOX Cloud"),
        }
        client.state.disconnected();
        client.publish();

        if client.state.invalid_key() {
            tracing::info!(
                switcher_id = %client.deps.switcher_id,
                "BELABOX Cloud refused the remote key; waiting for a new one"
            );
            while let Some(request) = commands.recv().await {
                request.refuse(BelaboxRefusal::InvalidKey);
            }
            return;
        }

        let pause = tokio::time::sleep(backoff);
        tokio::pin!(pause);
        loop {
            tokio::select! {
                _ = &mut pause => break,
                Some(request) = commands.recv() => request.refuse(BelaboxRefusal::Connecting),
            }
        }
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

impl Client {
    /// Runs one connection; `true` if BELABOX Cloud accepted the key.
    async fn session(
        &mut self,
        socket: tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        commands: &mut mpsc::Receiver<Request>,
    ) -> bool {
        let (mut sink, mut stream) = socket.split();
        self.state.connected();
        let auth = protocol::auth(&self.deps.key);
        if let Err(error) = sink.send(Message::Text(auth.into())).await {
            tracing::warn!(%error, "Couldn't authenticate with BELABOX Cloud");
            return false;
        }

        let mut keepalive = tokio::time::interval(KEEPALIVE);
        keepalive.reset();
        // Notifications expire even when nothing arrives.
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut accepted = false;

        loop {
            tokio::select! {
                message = stream.next() => match message {
                    Some(Ok(Message::Text(text))) => {
                        let changes = self.state.apply(&text, Instant::now());
                        accepted |= changes.contains(&Change::Authenticated(true));
                        for change in changes {
                            self.on_change(change).await;
                        }
                        self.publish();
                        if self.state.invalid_key() {
                            let _ = sink.close().await;
                            return accepted;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::info!(%error, "BELABOX Cloud connection lost");
                        return accepted;
                    }
                    None => {
                        tracing::info!("BELABOX Cloud closed the connection");
                        return accepted;
                    }
                },
                _ = keepalive.tick() => {
                    if sink.send(Message::Text(protocol::keepalive().into())).await.is_err() {
                        return accepted;
                    }
                }
                _ = tick.tick() => self.publish(),
                Some(request) = commands.recv() => match request {
                    Request::Details(reply) => {
                        let _ = reply.send(self.state.details());
                    }
                    Request::Command(command, reply) => {
                        let answer = match self.state.prepare(&command.action) {
                            Ok(frame) => {
                                if let Some(frame) = frame
                                    && sink.send(Message::Text(frame.into())).await.is_err()
                                {
                                    let _ = reply.send(Err(BelaboxRefusal::Connecting));
                                    return accepted;
                                }
                                self.sent(command).await;
                                Ok(())
                            }
                            Err(refusal) => Err(refusal),
                        };
                        let _ = reply.send(answer);
                    }
                },
            }
        }
    }

    /// Updates the status viewers see, if it changed.
    fn publish(&mut self) {
        let next = self.state.status(Instant::now());
        self.status.send_if_modified(|current| {
            let next = BelaboxStatus {
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

    /// Remembers what a command was for, so the change it causes is logged "by"
    /// its sender.
    async fn sent(&mut self, command: BelaboxCommand) {
        let intent = match command.action {
            BelaboxAction::Start => Intent::Stream(true),
            BelaboxAction::Stop => Intent::Stream(false),
            BelaboxAction::Interface { name, enabled } => Intent::Interface(name, enabled),
            // Nothing the BELABOX reports back as a change; log that it was done.
            BelaboxAction::WifiConnect { .. }
            | BelaboxAction::WifiDisconnect { .. }
            | BelaboxAction::WifiNew { .. }
            | BelaboxAction::WifiForget { .. } => {
                self.configured(BelaboxChange::Wifi, command.by).await;
                return;
            }
            BelaboxAction::Hotspot { .. } | BelaboxAction::HotspotConfig { .. } => {
                self.configured(BelaboxChange::Hotspot, command.by).await;
                return;
            }
            BelaboxAction::ModemConfig { .. } => {
                self.configured(BelaboxChange::Modem, command.by).await;
                return;
            }
            BelaboxAction::Settings { .. } => {
                self.configured(BelaboxChange::StreamSettings, command.by)
                    .await;
                return;
            }
            // Scans change nothing worth logging.
            BelaboxAction::WifiScan | BelaboxAction::ModemScan { .. } => return,
            // BELABOX doesn't echo it to us, so it's done once sent.
            BelaboxAction::Bitrate { max_br } => {
                self.state.bitrate_sent(max_br);
                self.publish();
                self.record(EventDetail::BelaboxBitrateChanged {
                    max_br,
                    by: Some(command.by),
                })
                .await;
                return;
            }
        };
        self.intents.push((intent, command.by, Instant::now()));
    }

    async fn configured(&self, what: BelaboxChange, by: String) {
        self.record(EventDetail::BelaboxConfigured { what, by: Some(by) })
            .await;
    }

    fn take_intent(&mut self, intent: &Intent) -> Option<String> {
        self.intents.retain(|(_, _, at)| at.elapsed() < INTENT_TTL);
        let i = self.intents.iter().position(|(i, _, _)| i == intent)?;
        Some(self.intents.remove(i).1)
    }

    async fn on_change(&mut self, change: Change) {
        let detail = match change {
            Change::Authenticated(true) => {
                tracing::debug!(switcher_id = %self.deps.switcher_id, "Connected to BELABOX Cloud");
                return;
            }
            Change::Authenticated(false) => return,
            Change::EncoderOnline(true) => EventDetail::BelaboxOnline,
            Change::EncoderOnline(false) => EventDetail::BelaboxOffline,
            Change::Streaming(active) => {
                let by = self.take_intent(&Intent::Stream(active));
                if active {
                    EventDetail::BelaboxStreamStarted { by }
                } else {
                    EventDetail::BelaboxStreamStopped { by }
                }
            }
            Change::InterfaceToggled { name, enabled } => {
                let by = self.take_intent(&Intent::Interface(name.clone(), enabled));
                EventDetail::BelaboxInterfaceToggled { name, enabled, by }
            }
            Change::Error(message) => EventDetail::BelaboxError { message },
        };
        self.record(detail).await;
    }

    async fn record(&self, detail: EventDetail) {
        if let Err(error) = live::record_event(
            &self.deps.db,
            &self.deps.nats,
            self.deps.switcher_id,
            detail,
        )
        .await
        {
            tracing::warn!(%error, "Failed to record a BELABOX event");
        }
    }
}
