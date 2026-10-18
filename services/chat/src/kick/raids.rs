//! Kick raids, from Kick's site WebSocket ([`pusher`]), since Kick has no
//! webhook event for them yet (drop this once it has one).
//!
//! One instance at a time (the `kick-raids` lease) watches the Kick channels
//! where a linked switcher stops the stream after raiding out
//! ([`db::chat::raid_channels`]), re-read on every reconcile request and every
//! [`REFRESH`]. The socket wants Kick's own channel and chatroom IDs, which the
//! public API doesn't give, so they're looked up on Kick's site API
//! ([`Kick::site_channel`]) and kept in memory. Channels are spread over sockets
//! of [`CHANNELS_PER_SOCKET`], and a raid becomes a [`ChatEvent::Raid`] like
//! Twitch's.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::Duration,
};

use db::chat::ChatPlatform;
use futures_util::{SinkExt, StreamExt};
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{Instant, timeout},
};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::{
    Kick,
    pusher::{self, Frame, Raid},
};
use crate::{
    app::State,
    error::{Error, Result},
    events::{self, ChatEvent},
    kv::{self, HEARTBEAT, Lease},
};

/// How often the wanted channels are re-read (besides on reconcile requests).
const REFRESH: Duration = Duration::from_secs(60);
/// A channel whose IDs couldn't be found is tried again after this.
const RETRY_LOOKUP: Duration = Duration::from_secs(600);
/// Channels per socket, two topics each. Pusher has no documented limit, but
/// this is Kick's app and the chatroom topics carry all of their chat.
const CHANNELS_PER_SOCKET: usize = 50;
/// Pusher's answer to a ping, and its first frame, must come within this.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs until the task is dropped: takes the lease when it's free and watches
/// while it holds it.
pub async fn run(state: State) {
    let Some(kick) = state.kick.clone() else {
        return;
    };
    loop {
        match Lease::acquire(&state.buckets.locks, &lease_key(&state), &state.instance_id).await {
            Ok(Some(lease)) => {
                tracing::info!("Watching Kick raids");
                watch(&state, &kick, lease).await;
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "Couldn't take the Kick raids lease"),
        }
        tokio::time::sleep(HEARTBEAT).await;
    }
}

fn lease_key(state: &State) -> String {
    state.key(&["kick-raids"])
}

/// Lets go of the lease if this instance holds it (on shutdown, once [`run`]
/// has stopped), so another takes over at its next try instead of after the
/// lease lapses.
pub async fn release(state: &State) {
    let key = lease_key(state);
    if let Ok(Some(entry)) = state.buckets.locks.entry(&key).await
        && entry.value.as_ref() == state.instance_id.as_bytes()
    {
        kv::release(&state.buckets.locks, &key, entry.revision).await;
        tracing::info!("Handed the Kick raids over");
    }
}

/// Watches while the lease holds.
async fn watch(state: &State, kick: &Kick, mut lease: Lease) {
    let mut reconciles = match state
        .nats
        .subscribe(live::chat_reconcile_subject(&state.environment))
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::warn!(%error, "Couldn't subscribe to reconcile requests");
            lease.release().await;
            return;
        }
    };
    let (raids_tx, mut raids) = mpsc::channel(64);
    let mut watcher = Watcher {
        url: kick.config.pusher_url.clone(),
        raids: raids_tx,
        channels: HashMap::new(),
        failed: HashMap::new(),
        sockets: Vec::new(),
    };
    let mut renew = tokio::time::interval(HEARTBEAT);
    renew.tick().await;
    let mut refresh = tokio::time::interval(REFRESH);
    loop {
        tokio::select! {
            _ = renew.tick() => {
                if !lease.renew().await {
                    tracing::warn!("Lost the Kick raids lease");
                    return;
                }
            }
            _ = refresh.tick() => watcher.refresh(state, kick).await,
            Some(_) = reconciles.next() => watcher.refresh(state, kick).await,
            Some(raid) = raids.recv() => watcher.raided(state, raid).await,
        }
    }
}

/// A Kick channel we listen to, by its broadcaster's user ID.
struct Channel {
    slug: String,
    topics: [String; 2],
}

struct Watcher {
    url: String,
    raids: mpsc::Sender<Raid>,
    channels: HashMap<String, Channel>,
    /// Channels whose lookup failed, and when.
    failed: HashMap<String, Instant>,
    sockets: Vec<Socket>,
}

impl Watcher {
    async fn refresh(&mut self, state: &State, kick: &Kick) {
        let wanted = match db::chat::raid_channels(&state.db, ChatPlatform::Kick).await {
            Ok(wanted) => wanted,
            Err(error) => {
                tracing::warn!(%error, "Couldn't read the Kick channels to watch for raids");
                return;
            }
        };
        self.channels.retain(|id, _| wanted.contains(id));
        self.failed.retain(|id, _| wanted.contains(id));
        for id in &wanted {
            if self.channels.contains_key(id)
                || self
                    .failed
                    .get(id)
                    .is_some_and(|at| at.elapsed() < RETRY_LOOKUP)
            {
                continue;
            }
            match look_up(kick, id).await {
                Ok(channel) => {
                    tracing::info!(
                        channel = id,
                        slug = channel.slug,
                        topics = ?channel.topics,
                        "Watching a Kick channel for raids"
                    );
                    self.failed.remove(id);
                    self.channels.insert(id.clone(), channel);
                }
                Err(error) => {
                    tracing::warn!(%error, channel = id, "Couldn't find a Kick channel's IDs for its raids");
                    self.failed.insert(id.clone(), Instant::now());
                }
            }
        }

        let wanted: BTreeSet<String> = self.channels.keys().cloned().collect();
        let mut plan: Vec<BTreeSet<String>> = self
            .sockets
            .iter()
            .map(|socket| socket.channels.keys().cloned().collect())
            .collect();
        assign(&mut plan, &wanted, CHANNELS_PER_SOCKET);
        for (index, channels) in plan.into_iter().enumerate() {
            let channels: BTreeMap<String, [String; 2]> = channels
                .into_iter()
                .map(|id| {
                    let topics = match self.sockets.get(index).and_then(|s| s.channels.get(&id)) {
                        Some(topics) => topics.clone(),
                        None => self.channels[&id].topics.clone(),
                    };
                    (id, topics)
                })
                .collect();
            let Some(socket) = self.sockets.get_mut(index) else {
                self.sockets
                    .push(Socket::spawn(&self.url, channels, &self.raids));
                continue;
            };
            for (id, topics) in &socket.channels {
                if !channels.contains_key(id) {
                    for topic in topics {
                        let _ = socket.commands.send(Command::Unsubscribe(topic.clone()));
                    }
                }
            }
            for (id, topics) in &channels {
                if !socket.channels.contains_key(id) {
                    for topic in topics {
                        let _ = socket.commands.send(Command::Subscribe(topic.clone()));
                    }
                }
            }
            socket.channels = channels;
        }
        // An emptied socket closes (dropping it aborts its task).
        self.sockets.retain(|socket| !socket.channels.is_empty());
    }

    async fn raided(&self, state: &State, raid: Raid) {
        tracing::info!(
            topic = raid.topic,
            from = raid.from_slug,
            to = raid.to_slug,
            "A Kick channel raided out"
        );
        let from = self
            .channels
            .iter()
            .find(|(_, channel)| channel.topics.contains(&raid.topic))
            .or_else(|| {
                self.channels
                    .iter()
                    .find(|(_, channel)| channel.slug.eq_ignore_ascii_case(&raid.from_slug))
            });
        let Some((from_id, _)) = from else {
            tracing::debug!(topic = raid.topic, "A raid from a channel we don't watch");
            return;
        };
        // Both of a channel's topics may bring the same raid.
        let key = kv::key(&["kick-raid", from_id, &raid.to_slug]);
        if !kv::first_time(&state.buckets.seen, &key)
            .await
            .unwrap_or(true)
        {
            return;
        }
        let event = ChatEvent::Raid {
            platform: ChatPlatform::Kick,
            from_id: from_id.clone(),
            to_name: raid.to_name,
            to_login: raid.to_slug,
        };
        events::publish(state, &event).await;
    }
}

/// Finds the channel and chatroom IDs of the channel of `broadcaster_user_id`.
async fn look_up(kick: &Kick, broadcaster_user_id: &str) -> Result<Channel> {
    let slug = kick
        .slug_of(broadcaster_user_id)
        .await?
        .ok_or_else(|| Error::Other("Kick has no such channel".into()))?;
    let site = kick.site_channel(&slug).await?;
    if site.user_id.to_string() != broadcaster_user_id {
        return Err(Error::Other(format!(
            "Kick's site has {slug} as user {}",
            site.user_id
        )));
    }
    Ok(Channel {
        topics: pusher::topics(site.id, site.chatroom.id),
        slug,
    })
}

/// Spreads the `wanted` channels over sockets of at most `per_socket`: channels
/// stay where they are, gone ones leave, new ones go to the first socket with
/// room, then to new sockets. Positions are kept; an emptied socket stays empty.
fn assign(sockets: &mut Vec<BTreeSet<String>>, wanted: &BTreeSet<String>, per_socket: usize) {
    for socket in sockets.iter_mut() {
        socket.retain(|channel| wanted.contains(channel));
    }
    let placed: BTreeSet<String> = sockets.iter().flatten().cloned().collect();
    for channel in wanted.difference(&placed) {
        match sockets.iter_mut().find(|socket| socket.len() < per_socket) {
            Some(socket) => {
                socket.insert(channel.clone());
            }
            None => sockets.push(BTreeSet::from([channel.clone()])),
        }
    }
}

enum Command {
    Subscribe(String),
    Unsubscribe(String),
}

/// One Pusher connection, run by its own task until dropped.
struct Socket {
    /// Its channels and their topics.
    channels: BTreeMap<String, [String; 2]>,
    commands: mpsc::UnboundedSender<Command>,
    task: JoinHandle<()>,
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Socket {
    fn spawn(
        url: &str,
        channels: BTreeMap<String, [String; 2]>,
        raids: &mpsc::Sender<Raid>,
    ) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let task = tokio::spawn(connection(
            url.to_owned(),
            channels.values().flatten().cloned().collect(),
            receiver,
            raids.clone(),
        ));
        Self {
            channels,
            commands,
            task,
        }
    }
}

/// Keeps a connection with `topics` subscribed, reconnecting with a backoff.
async fn connection(
    url: String,
    mut topics: BTreeSet<String>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    raids: mpsc::Sender<Raid>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        match session(&url, &mut topics, &mut commands, &raids).await {
            Ok(Ended::Closed) => return,
            Ok(Ended::Dropped) => {
                tracing::info!("Kick's site WebSocket closed; reconnecting");
                backoff = Duration::from_secs(1);
            }
            Err(error) => {
                tracing::warn!(%error, "Kick's site WebSocket failed");
                backoff = (backoff * 2).min(Duration::from_secs(32));
            }
        }
        tokio::time::sleep(backoff).await;
    }
}

enum Ended {
    /// Nothing wants it anymore.
    Closed,
    /// Kick closed it.
    Dropped,
}

async fn session(
    url: &str,
    topics: &mut BTreeSet<String>,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    raids: &mpsc::Sender<Raid>,
) -> Result<Ended> {
    let (mut socket, _) = timeout(ANSWER_TIMEOUT, tokio_tungstenite::connect_async(url))
        .await
        .map_err(|_| Error::Other("timed out connecting".into()))??;
    let activity_timeout = loop {
        let frame = timeout(ANSWER_TIMEOUT, socket.next())
            .await
            .map_err(|_| Error::Other("no connection_established".into()))?
            .ok_or_else(|| Error::Other("closed before connection_established".into()))??;
        if let WsMessage::Text(text) = frame
            && let Frame::Connected { activity_timeout } = pusher::decode(&text)
        {
            break activity_timeout;
        }
    };
    // What changed while we were away, then everything.
    while let Ok(command) = commands.try_recv() {
        apply(topics, &command);
    }
    for topic in topics.iter() {
        socket.send(pusher::subscribe(topic).into()).await?;
    }
    tracing::debug!(topics = topics.len(), "Connected to Kick's site WebSocket");

    let idle = tokio::time::sleep(activity_timeout);
    tokio::pin!(idle);
    let mut pinged = false;
    loop {
        tokio::select! {
            frame = socket.next() => {
                let frame = match frame {
                    None => return Ok(Ended::Dropped),
                    Some(frame) => frame?,
                };
                idle.as_mut().reset(Instant::now() + activity_timeout);
                pinged = false;
                let text = match frame {
                    WsMessage::Text(text) => text,
                    WsMessage::Ping(payload) => {
                        socket.send(WsMessage::Pong(payload)).await?;
                        continue;
                    }
                    WsMessage::Close(frame) => {
                        tracing::info!(?frame, "Kick closed its site WebSocket");
                        return Ok(Ended::Dropped);
                    }
                    _ => continue,
                };
                match pusher::decode(&text) {
                    Frame::Raid(raid) => {
                        if raids.send(raid).await.is_err() {
                            return Ok(Ended::Closed);
                        }
                    }
                    Frame::Subscribed { topic } => tracing::debug!(topic, "Subscribed on Kick"),
                    Frame::Error(message) => tracing::warn!(message, "Kick's site WebSocket refused something"),
                    Frame::Connected { .. } | Frame::Pong | Frame::Other => {}
                }
            }
            command = commands.recv() => {
                let Some(command) = command else {
                    let _ = socket.close(None).await;
                    return Ok(Ended::Closed);
                };
                apply(topics, &command);
                let request = match &command {
                    Command::Subscribe(topic) => pusher::subscribe(topic),
                    Command::Unsubscribe(topic) => pusher::unsubscribe(topic),
                };
                socket.send(request.into()).await?;
            }
            () = &mut idle => {
                if pinged {
                    return Err(Error::Other("no answer to a ping".into()));
                }
                socket.send(pusher::ping().into()).await?;
                pinged = true;
                idle.as_mut().reset(Instant::now() + ANSWER_TIMEOUT);
            }
        }
    }
}

fn apply(topics: &mut BTreeSet<String>, command: &Command) {
    match command {
        Command::Subscribe(topic) => topics.insert(topic.clone()),
        Command::Unsubscribe(topic) => topics.remove(topic),
    };
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::assign;

    fn set(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn channels_fill_sockets_and_overflow_to_new_ones() {
        let mut sockets = Vec::new();
        assign(&mut sockets, &set(&["1", "2", "3"]), 2);
        assert_eq!(sockets, [set(&["1", "2"]), set(&["3"])]);

        // New ones fill the room left, then a new socket.
        assign(&mut sockets, &set(&["1", "2", "3", "4", "5"]), 2);
        assert_eq!(sockets, [set(&["1", "2"]), set(&["3", "4"]), set(&["5"])]);
    }

    #[test]
    fn channels_stay_where_they_are_and_gone_ones_free_room() {
        let mut sockets = vec![set(&["1", "2"]), set(&["3", "4"])];
        assign(&mut sockets, &set(&["2", "3", "4", "9"]), 2);
        assert_eq!(sockets, [set(&["2", "9"]), set(&["3", "4"])]);

        // Emptied sockets keep their place (the caller closes them).
        assign(&mut sockets, &set(&["3"]), 2);
        assert_eq!(sockets, [set(&[]), set(&["3"])]);
    }
}
