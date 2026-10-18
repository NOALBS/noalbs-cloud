//! Follows the live chats of linked YouTube channels. One instance at a time
//! (the `youtube` lease) does it, like the Kick raids watcher.
//!
//! A channel ([`db::chat::youtube_channels`], re-read on every reconcile request
//! and every [`REFRESH`]) only matters while one of its switchers streams: then
//! its broadcast on air is looked up with the channel's grant (right away, a few
//! more times [`EARLY_CHECK_EVERY`] since YouTube takes a moment to show a new
//! broadcast, then every `live_check_interval`), and once found its chat is
//! followed over `StreamList` until the chat ends or the switcher stops
//! streaming. The chat's ID is kept in [`crate::kv::YOUTUBE`] so any instance
//! can answer in it. Text messages become [`InboundMessage`]s like Twitch's and
//! Kick's.

use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use db::chat::{ChatPlatform, TokenOwner, YouTubeChannel};
use futures_util::StreamExt;
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};
use uuid::Uuid;

use super::{YouTube, chat_key, is_quota, proto, record_error};
use crate::{
    app::State,
    error::Result,
    inbound::{self, Chatter, InboundMessage},
    kv::{self, HEARTBEAT, Lease},
    tokens,
};

/// How often the wanted channels are re-read (besides on reconcile requests).
const REFRESH: Duration = Duration::from_secs(60);
/// How often the switchers' streaming state is checked.
const CHECK_STREAMING: Duration = Duration::from_secs(5);
/// After a switcher starts streaming, how many lookups come sooner.
const EARLY_CHECKS: u32 = 3;
const EARLY_CHECK_EVERY: Duration = Duration::from_secs(15);
/// A new connection starts with the chat's recent messages: those sent longer
/// than this before it are history, not commands for us.
const BACKLOG: chrono::Duration = chrono::Duration::seconds(30);
const MAX_BACKOFF: Duration = Duration::from_secs(32);
/// After YouTube says the quota is used up, wait this long before asking again.
const QUOTA_BACKOFF: Duration = Duration::from_secs(15 * 60);

/// Runs until the task is dropped: takes the lease when it's free and follows
/// the chats while it holds it.
pub async fn run(state: State) {
    let Some(youtube) = state.youtube.clone() else {
        return;
    };
    loop {
        match Lease::acquire(&state.buckets.locks, &lease_key(&state), &state.instance_id).await {
            Ok(Some(lease)) => {
                tracing::info!("Following YouTube chats");
                watch(&state, &youtube, lease).await;
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "Couldn't take the YouTube lease"),
        }
        tokio::time::sleep(HEARTBEAT).await;
    }
}

fn lease_key(state: &State) -> String {
    state.key(&["youtube"])
}

/// Lets go of the lease if this instance holds it (on shutdown, once [`run`]
/// has stopped), so another takes over at its next try.
pub async fn release(state: &State) {
    let key = lease_key(state);
    if let Ok(Some(entry)) = state.buckets.locks.entry(&key).await
        && entry.value.as_ref() == state.instance_id.as_bytes()
    {
        kv::release(&state.buckets.locks, &key, entry.revision).await;
        tracing::info!("Handed the YouTube chats over");
    }
}

async fn watch(state: &State, youtube: &Arc<YouTube>, mut lease: Lease) {
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
    let (events_tx, mut events) = mpsc::channel(64);
    let mut watcher = Watcher {
        youtube: youtube.clone(),
        channels: HashMap::new(),
        events: events_tx,
    };
    watcher.refresh(state).await;
    let mut renew = tokio::time::interval(HEARTBEAT);
    renew.tick().await;
    let mut refresh = tokio::time::interval(REFRESH);
    refresh.tick().await;
    let mut check = tokio::time::interval(CHECK_STREAMING);
    loop {
        tokio::select! {
            _ = renew.tick() => {
                if !lease.renew().await {
                    tracing::warn!("Lost the YouTube lease");
                    return;
                }
                watcher.heartbeat(state).await;
            }
            _ = refresh.tick() => watcher.refresh(state).await,
            Some(_) = reconciles.next() => watcher.refresh(state).await,
            _ = check.tick() => watcher.check(state).await,
            Some(event) = events.recv() => watcher.handle(state, event).await,
        }
    }
}

/// A task that stops when dropped.
struct Task(JoinHandle<()>);

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

enum Chat {
    /// No switcher of the channel streams.
    Idle,
    /// Streaming, no chat found yet: look (again) at `next`.
    Waiting {
        attempts: u32,
        next: Instant,
    },
    Finding {
        attempts: u32,
        _task: Task,
    },
    Open {
        live_chat_id: String,
        _task: Task,
    },
}

struct Watched {
    channel: YouTubeChannel,
    chat: Chat,
}

enum Event {
    Found {
        channel_id: String,
        result: Result<Option<String>>,
    },
    /// A followed chat is over (or can't be read): look again after `retry_in`.
    Ended {
        channel_id: String,
        live_chat_id: String,
        retry_in: Duration,
    },
}

struct Watcher {
    youtube: Arc<YouTube>,
    /// By YouTube channel ID.
    channels: HashMap<String, Watched>,
    events: mpsc::Sender<Event>,
}

impl Watcher {
    fn check_interval(&self) -> Duration {
        Duration::from_secs(self.youtube.config.live_check_interval.max(10))
    }

    async fn refresh(&mut self, state: &State) {
        let wanted = match db::chat::youtube_channels(&state.db).await {
            Ok(wanted) => wanted,
            Err(error) => {
                tracing::warn!(%error, "Couldn't read the YouTube channels to follow");
                return;
            }
        };
        let gone: Vec<String> = self
            .channels
            .keys()
            .filter(|id| {
                !wanted
                    .iter()
                    .any(|channel| channel.platform_channel_id == **id)
            })
            .cloned()
            .collect();
        for id in gone {
            if let Some(watched) = self.channels.remove(&id) {
                close(state, &id, &watched.chat).await;
            }
        }
        for channel in wanted {
            match self.channels.get_mut(&channel.platform_channel_id) {
                Some(watched) => watched.channel = channel,
                None => {
                    self.channels.insert(
                        channel.platform_channel_id.clone(),
                        Watched {
                            channel,
                            chat: Chat::Idle,
                        },
                    );
                }
            }
        }
    }

    /// Starts and stops following as the switchers start and stop streaming.
    async fn check(&mut self, state: &State) {
        let now = Instant::now();
        for (id, watched) in &mut self.channels {
            let mut streaming = false;
            for switcher_id in &watched.channel.switcher_ids {
                if state
                    .control
                    .status(*switcher_id)
                    .await
                    .is_some_and(|status| status.streaming)
                {
                    streaming = true;
                    break;
                }
            }
            if !streaming {
                if !matches!(watched.chat, Chat::Idle) {
                    tracing::debug!(channel = id, "No switcher streams: not following");
                    close(state, id, &watched.chat).await;
                    watched.chat = Chat::Idle;
                }
                continue;
            }
            let attempts = match watched.chat {
                Chat::Idle => 0,
                Chat::Waiting { attempts, next } if next <= now => attempts,
                _ => continue,
            };
            watched.chat = Chat::Finding {
                attempts,
                _task: Task(tokio::spawn(find(
                    state.clone(),
                    self.youtube.clone(),
                    watched.channel.id,
                    id.clone(),
                    self.events.clone(),
                ))),
            };
        }
    }

    async fn handle(&mut self, state: &State, event: Event) {
        let interval = self.check_interval();
        match event {
            Event::Found { channel_id, result } => {
                let Some(watched) = self.channels.get_mut(&channel_id) else {
                    return;
                };
                let Chat::Finding { attempts, .. } = watched.chat else {
                    return;
                };
                let attempts = attempts + 1;
                let retry_in = match result {
                    Ok(Some(live_chat_id)) => {
                        tracing::info!(channel = channel_id, "Following a YouTube live chat");
                        put_chat(state, &channel_id, &live_chat_id).await;
                        watched.chat = Chat::Open {
                            _task: Task(tokio::spawn(follow(
                                state.clone(),
                                self.youtube.clone(),
                                watched.channel.id,
                                channel_id.clone(),
                                live_chat_id.clone(),
                                self.events.clone(),
                            ))),
                            live_chat_id,
                        };
                        return;
                    }
                    Ok(None) if attempts <= EARLY_CHECKS => EARLY_CHECK_EVERY,
                    Ok(None) => interval,
                    Err(error) => {
                        let quota = is_quota(&error);
                        tracing::warn!(%error, channel = channel_id, "Couldn't look for a YouTube live chat");
                        record_error(state, quota, error.to_string()).await;
                        if quota { QUOTA_BACKOFF } else { interval }
                    }
                };
                watched.chat = Chat::Waiting {
                    attempts,
                    next: Instant::now() + retry_in,
                };
            }
            Event::Ended {
                channel_id,
                live_chat_id,
                retry_in,
            } => {
                let Some(watched) = self.channels.get_mut(&channel_id) else {
                    return;
                };
                if !matches!(&watched.chat, Chat::Open { live_chat_id: open, .. } if *open == live_chat_id)
                {
                    return;
                }
                tracing::info!(channel = channel_id, "A YouTube live chat ended");
                close(state, &channel_id, &watched.chat).await;
                watched.chat = Chat::Waiting {
                    attempts: EARLY_CHECKS,
                    next: Instant::now() + retry_in,
                };
            }
        }
    }

    /// Renews the open chats' keys and says how it's going.
    async fn heartbeat(&self, state: &State) {
        let mut streaming = 0u32;
        let mut open = 0u32;
        for (id, watched) in &self.channels {
            match &watched.chat {
                Chat::Idle => {}
                Chat::Open { live_chat_id, .. } => {
                    streaming += 1;
                    open += 1;
                    put_chat(state, id, live_chat_id).await;
                }
                Chat::Waiting { .. } | Chat::Finding { .. } => streaming += 1,
            }
        }
        let summary = live::YouTubeWatcherSummary {
            instance_id: state.instance_id.clone(),
            updated_at: Utc::now(),
            streaming,
            chats_open: open,
        };
        if let Ok(value) = serde_json::to_vec(&summary)
            && let Err(error) = state
                .buckets
                .youtube_status
                .put(state.key(&["watcher"]), value.into())
                .await
        {
            tracing::debug!(%error, "Couldn't write the YouTube summary");
        }
    }
}

async fn put_chat(state: &State, channel_id: &str, live_chat_id: &str) {
    if let Err(error) = state
        .buckets
        .youtube
        .put(chat_key(state, channel_id), live_chat_id.to_owned().into())
        .await
    {
        tracing::warn!(%error, channel = channel_id, "Couldn't keep a YouTube live chat");
    }
}

/// Forgets an open chat (its task stops when the state is replaced).
async fn close(state: &State, channel_id: &str, chat: &Chat) {
    if matches!(chat, Chat::Open { .. })
        && let Err(error) = state
            .buckets
            .youtube
            .delete(chat_key(state, channel_id))
            .await
    {
        tracing::debug!(%error, channel = channel_id, "Couldn't forget a YouTube live chat");
    }
}

/// Looks for the channel's broadcast on air and reports back.
async fn find(
    state: State,
    youtube: Arc<YouTube>,
    channel: Uuid,
    channel_id: String,
    events: mpsc::Sender<Event>,
) {
    let result = active_chat(&state, &youtube, TokenOwner::Channel(channel)).await;
    let _ = events.send(Event::Found { channel_id, result }).await;
}

async fn active_chat(
    state: &State,
    youtube: &YouTube,
    owner: TokenOwner,
) -> Result<Option<String>> {
    let token = tokens::access_token(state, owner).await?;
    match youtube.active_chat(&token).await {
        // Google can refuse a token before its time (e.g. a changed password).
        Err(error) if error.status() == Some(reqwest::StatusCode::UNAUTHORIZED) => {
            let token = tokens::refreshed_token(state, owner).await?;
            youtube.active_chat(&token).await
        }
        result => result,
    }
}

/// Follows one live chat until it ends; reconnects where it left off.
async fn follow(
    state: State,
    youtube: Arc<YouTube>,
    channel: Uuid,
    channel_id: String,
    live_chat_id: String,
    events: mpsc::Sender<Event>,
) {
    let owner = TokenOwner::Channel(channel);
    let interval = Duration::from_secs(youtube.config.live_check_interval.max(10));
    let mut page_token: Option<String> = None;
    let mut backoff = Duration::from_secs(1);
    let mut refresh = false;
    let retry_in = loop {
        let token = if refresh {
            tokens::refreshed_token(&state, owner).await
        } else {
            tokens::access_token(&state, owner).await
        };
        refresh = false;
        let token = match token {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!(%error, channel = channel_id, "No token to read a YouTube chat with");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                continue;
            }
        };
        let skip_before = page_token.is_none().then(|| Utc::now() - BACKLOG);
        let closed = match youtube
            .stream(&token, &live_chat_id, page_token.clone())
            .await
        {
            Ok(mut stream) => {
                let mut answered = false;
                loop {
                    match stream.message().await {
                        Ok(Some(response)) => {
                            answered = true;
                            backoff = Duration::from_secs(1);
                            if let Some(next) =
                                response.next_page_token.clone().filter(|t| !t.is_empty())
                            {
                                page_token = Some(next);
                            }
                            if handle_response(&state, &channel_id, &response, skip_before).await {
                                break Closed::Ended;
                            }
                        }
                        // YouTube ends every stream after about 10 s: carry on from
                        // the last page at once.
                        Ok(None) if answered => break Closed::Again,
                        Ok(None) => {
                            break Closed::Failed(tonic::Status::unavailable(
                                "the stream closed without a message",
                            ));
                        }
                        Err(status) => break Closed::Failed(status),
                    }
                }
            }
            Err(status) => Closed::Failed(status),
        };
        let status = match closed {
            Closed::Ended => break interval,
            Closed::Again => continue,
            Closed::Failed(status) => status,
        };
        match status.code() {
            tonic::Code::Unauthenticated => refresh = true,
            // The chat ended or was turned off, or the broadcast is gone.
            tonic::Code::FailedPrecondition | tonic::Code::NotFound => break interval,
            tonic::Code::PermissionDenied => {
                tracing::warn!(
                    channel = channel_id,
                    message = status.message(),
                    "YouTube refused to show a chat"
                );
                record_error(
                    &state,
                    false,
                    format!("A channel's chat was refused: {}", status.message()),
                )
                .await;
                break interval;
            }
            tonic::Code::ResourceExhausted => {
                tracing::warn!(
                    channel = channel_id,
                    message = status.message(),
                    "YouTube's quota or rate limit"
                );
                record_error(&state, true, status.message().to_owned()).await;
                break QUOTA_BACKOFF;
            }
            _ => {
                tracing::debug!(channel = channel_id, %status, "The YouTube chat stream broke off");
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    };
    let _ = events
        .send(Event::Ended {
            channel_id,
            live_chat_id,
            retry_in,
        })
        .await;
}

/// How a `StreamList` stream ended.
enum Closed {
    /// The chat is over.
    Ended,
    /// YouTube closed it as usual: open the next one.
    Again,
    Failed(tonic::Status),
}

/// Hands the response's commands on; `true` when the chat is over.
async fn handle_response(
    state: &State,
    channel_id: &str,
    response: &proto::LiveChatMessageListResponse,
    skip_before: Option<DateTime<Utc>>,
) -> bool {
    let mut ended = response
        .offline_at
        .as_deref()
        .is_some_and(|at| !at.is_empty());
    for item in &response.items {
        if item.snippet.as_ref().and_then(|snippet| snippet.r#type) == Some(proto::CHAT_ENDED_EVENT)
        {
            ended = true;
        }
        let Some(message) = inbound_message(channel_id, item, skip_before) else {
            continue;
        };
        let seen = kv::key(&["youtube", &message.message_id]);
        match kv::first_time(&state.buckets.seen, &seen).await {
            Ok(true) => inbound::publish(state, &message).await,
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "Couldn't check a YouTube message"),
        }
    }
    ended
}

/// A text message as a chat command candidate. Messages published before
/// `skip_before` (the backlog of a new connection) are left out.
fn inbound_message(
    channel_id: &str,
    item: &proto::LiveChatMessage,
    skip_before: Option<DateTime<Utc>>,
) -> Option<InboundMessage> {
    let snippet = item.snippet.as_ref()?;
    if snippet.r#type != Some(proto::TEXT_MESSAGE_EVENT) {
        return None;
    }
    if let Some(skip_before) = skip_before {
        let published = snippet
            .published_at
            .as_deref()
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())?;
        if published < skip_before {
            return None;
        }
    }
    let text = snippet
        .text_message_details
        .as_ref()?
        .message_text
        .clone()?;
    let author = item.author_details.as_ref()?;
    let name = author
        .display_name
        .as_deref()
        .unwrap_or_default()
        .trim_start_matches('@')
        .to_owned();
    Some(InboundMessage {
        platform: ChatPlatform::YouTube,
        channel_id: channel_id.to_owned(),
        bot_id: None,
        message_id: item.id.clone()?,
        chatter: Chatter {
            id: author.channel_id.clone()?,
            login: name.clone(),
            display_name: name,
            moderator: author.is_chat_moderator.unwrap_or(false)
                || author.is_chat_owner.unwrap_or(false),
            vip: false,
            subscriber: author.is_chat_sponsor.unwrap_or(false),
        },
        text,
    })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{inbound_message, proto};

    fn item(kind: i32, published_at: &str, text: &str) -> proto::LiveChatMessage {
        proto::LiveChatMessage {
            id: Some("msg-1".into()),
            snippet: Some(proto::LiveChatMessageSnippet {
                r#type: Some(kind),
                published_at: Some(published_at.into()),
                text_message_details: Some(proto::LiveChatTextMessageDetails {
                    message_text: Some(text.into()),
                }),
            }),
            author_details: Some(proto::LiveChatMessageAuthorDetails {
                channel_id: Some("UCviewer".into()),
                display_name: Some("@Viewer".into()),
                is_chat_owner: Some(false),
                is_chat_sponsor: Some(true),
                is_chat_moderator: Some(true),
            }),
        }
    }

    #[test]
    fn text_messages_become_inbound_messages() {
        let message = inbound_message(
            "UCstreamer",
            &item(
                proto::TEXT_MESSAGE_EVENT,
                "2026-09-29T12:00:00.5+00:00",
                "!bitrate",
            ),
            None,
        )
        .unwrap();
        assert_eq!(message.channel_id, "UCstreamer");
        assert_eq!(message.message_id, "msg-1");
        assert_eq!(message.text, "!bitrate");
        assert_eq!(message.bot_id, None);
        assert_eq!(message.chatter.id, "UCviewer");
        assert_eq!(message.chatter.login, "Viewer");
        assert!(message.chatter.moderator);
        assert!(message.chatter.subscriber);
        assert!(!message.chatter.vip);
    }

    #[test]
    fn other_events_are_left_out() {
        let ended = item(proto::CHAT_ENDED_EVENT, "2026-09-29T12:00:00Z", "");
        assert_eq!(inbound_message("UCstreamer", &ended, None), None);
    }

    #[test]
    fn a_new_connections_backlog_is_skipped() {
        let skip_before = Some(Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap());
        let old = item(
            proto::TEXT_MESSAGE_EVENT,
            "2026-09-29T11:59:59Z",
            "!switch live",
        );
        assert_eq!(inbound_message("UCstreamer", &old, skip_before), None);
        let new = item(
            proto::TEXT_MESSAGE_EVENT,
            "2026-09-29T12:00:01Z",
            "!switch live",
        );
        assert!(inbound_message("UCstreamer", &new, skip_before).is_some());
    }
}
