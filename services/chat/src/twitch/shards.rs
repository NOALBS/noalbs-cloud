//! Spreads the conduit's shards over the running instances.
//!
//! Every [`HEARTBEAT`] each instance renews its own heartbeat and its shard
//! claims, then aims for `ceil(shards / live instances)` shards: it claims free
//! ones (claims of a dead instance lapse after [`kv::CLAIM_TTL`]) or lets go of
//! extra ones so a newcomer can take them. For each shard it holds it runs one
//! EventSub WebSocket and points the shard at it.

use std::{collections::BTreeMap, time::Duration};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use tokio::{net::TcpStream, task::JoinHandle, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message as WsMessage};

use super::eventsub::{self, Message};
use crate::{
    app::State,
    error::{Error, Result},
    events, inbound,
    kv::{self, HEARTBEAT},
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Twitch closes a session that isn't used within 10 s of its welcome.
const WELCOME_TIMEOUT: Duration = Duration::from_secs(10);

struct Held {
    revision: u64,
    session: JoinHandle<()>,
}

/// How many shards this instance should hold.
pub fn target(shards: u32, live_instances: usize) -> u32 {
    shards.div_ceil(live_instances.max(1) as u32)
}

fn shard_key(state: &State, shard: u32) -> String {
    state.key(&["shard", &shard.to_string()])
}

/// Runs until the task is dropped: keeps this instance's share of the shards.
pub async fn run(state: State) {
    let Some(twitch) = state.twitch.clone() else {
        return;
    };
    let shards = twitch.config.shards;
    let mut held: BTreeMap<u32, Held> = BTreeMap::new();
    let mut tick = tokio::time::interval(HEARTBEAT);

    loop {
        tick.tick().await;
        if let Err(error) = state
            .buckets
            .instances
            .put(state.key(&[&state.instance_id]), Bytes::new())
            .await
        {
            tracing::warn!(%error, "Couldn't write the instance heartbeat");
            continue;
        }

        // Renew what we hold; drop what lapsed (another instance may have it now).
        let mut lost = Vec::new();
        for (shard, claim) in held.iter_mut() {
            match kv::renew(
                &state.buckets.shards,
                &shard_key(&state, *shard),
                &state.instance_id,
                claim.revision,
            )
            .await
            {
                Ok(Some(revision)) => claim.revision = revision,
                Ok(None) => lost.push(*shard),
                Err(error) => tracing::warn!(%error, shard, "Couldn't renew a shard claim"),
            }
            if claim.session.is_finished() {
                // The session gave up (e.g. no conduit yet): start another.
                claim.session = spawn_session(state.clone(), *shard);
            }
        }
        for shard in lost {
            tracing::warn!(shard, "Lost a shard claim");
            if let Some(claim) = held.remove(&shard) {
                claim.session.abort();
            }
        }

        tracing::debug!(held = held.len(), "Counting instances");
        let live = match live_instances(&state).await {
            Ok(live) => live,
            Err(error) => {
                tracing::warn!(%error, "Couldn't count the chat instances");
                continue;
            }
        };
        let want = target(shards, live);
        tracing::debug!(live, want, held = held.len(), "Shard tick");

        // Let go of extras, highest first, so newcomers can take them.
        while held.len() as u32 > want {
            let (shard, claim) = held.pop_last().expect("held is not empty");
            claim.session.abort();
            kv::release(
                &state.buckets.shards,
                &shard_key(&state, shard),
                claim.revision,
            )
            .await;
            tracing::info!(shard, "Released a shard to another instance");
        }
        // Claim free ones, starting after our instance's position so instances
        // starting together try different shards first.
        let offset = offset(&state.instance_id, shards);
        for step in 0..shards {
            if held.len() as u32 >= want {
                break;
            }
            let shard = (offset + step) % shards;
            if held.contains_key(&shard) {
                continue;
            }
            match claim_or_adopt(&state, shard).await {
                Ok(Some(revision)) => {
                    tracing::info!(shard, "Claimed a Twitch conduit shard");
                    held.insert(
                        shard,
                        Held {
                            revision,
                            session: spawn_session(state.clone(), shard),
                        },
                    );
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(%error, shard, "Couldn't claim a shard"),
            }
        }
    }
}

/// Claims a free shard, or takes back one still claimed under our own ID (this
/// instance restarted with a configured `instance_id` before its claims lapsed).
async fn claim_or_adopt(state: &State, shard: u32) -> Result<Option<u64>> {
    let key = shard_key(state, shard);
    if let Some(revision) = kv::claim(&state.buckets.shards, &key, &state.instance_id).await? {
        return Ok(Some(revision));
    }
    match state.buckets.shards.entry(&key).await {
        Ok(Some(entry)) if entry.value.as_ref() == state.instance_id.as_bytes() => {
            kv::renew(
                &state.buckets.shards,
                &key,
                &state.instance_id,
                entry.revision,
            )
            .await
        }
        _ => Ok(None),
    }
}

fn offset(instance_id: &str, shards: u32) -> u32 {
    instance_id.bytes().map(u32::from).sum::<u32>() % shards.max(1)
}

async fn live_instances(state: &State) -> Result<usize> {
    let keys = state.buckets.instances.keys().await.map_err(Error::nats)?;
    Ok(keys
        .filter(|key| std::future::ready(key.is_ok()))
        .count()
        .await)
}

fn spawn_session(state: State, shard: u32) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            match run_session(&state, shard).await {
                Ok(()) => backoff = Duration::from_secs(1),
                Err(error) => {
                    tracing::warn!(%error, shard, "Twitch shard session ended");
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
            tokio::time::sleep(backoff).await;
        }
    })
}

/// One WebSocket session for a shard, following Twitch's reconnects, until it
/// drops. Returns `Ok` when it ended normally (Twitch closed it).
async fn run_session(state: &State, shard: u32) -> Result<()> {
    let twitch = state.twitch.clone().expect("shards run only with Twitch");
    let conduit_id = match twitch.conduit_id().await {
        Some(id) => id,
        None => twitch.ensure_conduit(&state.db).await?,
    };

    let (mut socket, session_id, mut keepalive) = connect(&twitch.config.eventsub_url).await?;
    twitch
        .helix
        .assign_shard(&conduit_id, shard, &session_id)
        .await?;
    tracing::info!(shard, "Twitch shard connected");

    loop {
        let frame = match timeout(keepalive, socket.next()).await {
            Err(_) => return Err(Error::Other("no keepalive from Twitch".into())),
            Ok(None) => return Ok(()),
            Ok(Some(frame)) => frame?,
        };
        let text = match frame {
            WsMessage::Text(text) => text,
            WsMessage::Ping(payload) => {
                socket.send(WsMessage::Pong(payload)).await?;
                continue;
            }
            WsMessage::Close(frame) => {
                tracing::info!(shard, ?frame, "Twitch closed the shard session");
                return Ok(());
            }
            _ => continue,
        };
        match eventsub::decode(&text) {
            Ok(Message::Keepalive) | Ok(Message::Welcome { .. }) => {}
            Ok(Message::Reconnect { url }) => {
                // Connect to the new URL first; the old socket closes after its welcome.
                let (next, next_session, next_keepalive) = connect(&url).await?;
                if next_session != session_id {
                    twitch
                        .helix
                        .assign_shard(&conduit_id, shard, &next_session)
                        .await?;
                }
                socket = next;
                keepalive = next_keepalive;
                tracing::info!(shard, "Twitch moved the shard session");
            }
            Ok(Message::Chat {
                message_id,
                message,
            }) => {
                if seen_first(state, &message_id).await {
                    inbound::publish(state, &message).await;
                }
            }
            Ok(Message::Raid {
                message_id,
                from_id,
                to_name,
                to_login,
            }) => {
                if seen_first(state, &message_id).await {
                    let raid = events::ChatEvent::Raid {
                        platform: db::chat::ChatPlatform::Twitch,
                        from_id,
                        to_name,
                        to_login,
                    };
                    events::publish(state, &raid).await;
                }
            }
            Ok(Message::AuthRevoked {
                message_id,
                user_id,
            }) => {
                if seen_first(state, &message_id).await {
                    let revoked = events::ChatEvent::Revoked {
                        platform: db::chat::ChatPlatform::Twitch,
                        user_id,
                    };
                    events::publish(state, &revoked).await;
                }
            }
            Ok(Message::Event { subscription, .. }) => {
                tracing::debug!(kind = subscription.kind, "Unhandled Twitch event");
            }
            Ok(Message::Revocation { subscription }) => {
                tracing::warn!(
                    kind = subscription.kind,
                    status = subscription.status,
                    channel = subscription.condition("broadcaster_user_id"),
                    "Twitch revoked a subscription"
                );
                if let Err(error) = db::chat::forget_subscription(
                    &state.db,
                    db::chat::ChatPlatform::Twitch,
                    &subscription.id,
                )
                .await
                {
                    tracing::warn!(%error, "Couldn't forget a revoked subscription");
                }
            }
            Ok(Message::Other(kind)) => tracing::debug!(kind, "Unhandled EventSub message"),
            Err(error) => tracing::warn!(%error, "Couldn't read an EventSub message"),
        }
    }
}

/// Twitch may deliver a message twice (e.g. around a reconnect); only the first counts.
async fn seen_first(state: &State, message_id: &str) -> bool {
    kv::first_time(&state.buckets.seen, &kv::key(&["twitch", message_id]))
        .await
        .unwrap_or(true)
}

/// Connects and waits for the welcome: the socket, its session ID and how long
/// to wait for any message before calling the connection dead.
async fn connect(url: &str) -> Result<(Socket, String, Duration)> {
    let (mut socket, _) = timeout(WELCOME_TIMEOUT, tokio_tungstenite::connect_async(url))
        .await
        .map_err(|_| Error::Other("timed out connecting to Twitch".into()))??;
    loop {
        let frame = timeout(WELCOME_TIMEOUT, socket.next())
            .await
            .map_err(|_| Error::Other("no welcome from Twitch".into()))?
            .ok_or_else(|| Error::Other("Twitch closed before the welcome".into()))??;
        if let WsMessage::Text(text) = frame
            && let Ok(Message::Welcome {
                session_id,
                keepalive,
            }) = eventsub::decode(&text)
        {
            let keepalive = Duration::from_secs(keepalive.unwrap_or(10) + 5);
            return Ok((socket, session_id, keepalive));
        }
    }
}

/// Makes sure the conduit exists, then runs [`run`]; for the app to spawn.
pub async fn start(state: State) {
    if let Some(twitch) = state.twitch.clone() {
        let mut backoff = Duration::from_secs(1);
        while let Err(error) = twitch.ensure_conduit(&state.db).await {
            tracing::warn!(%error, "Couldn't set up the Twitch conduit");
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
        run(state).await;
    }
}

#[cfg(test)]
mod tests {
    use super::target;

    #[test]
    fn shards_are_split_over_live_instances() {
        assert_eq!(target(4, 1), 4);
        assert_eq!(target(4, 2), 2);
        assert_eq!(target(4, 3), 2);
        assert_eq!(target(4, 8), 1);
        assert_eq!(target(4, 0), 4);
    }
}

/// Lets go of every shard this instance holds (on shutdown), so the others
/// take them over at their next heartbeat instead of after the claims lapse.
pub async fn release_all(state: &State) {
    let Ok(mut keys) = state.buckets.shards.keys().await else {
        return;
    };
    while let Some(Ok(key)) = keys.next().await {
        if !key.starts_with(&format!("{}.", state.environment)) {
            continue;
        }
        if let Ok(Some(entry)) = state.buckets.shards.entry(&key).await
            && entry.value.as_ref() == state.instance_id.as_bytes()
        {
            kv::release(&state.buckets.shards, &key, entry.revision).await;
        }
    }
}
