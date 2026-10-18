//! IRLHosting WebSocket (NOALBS v2's `WebSocket` kind): the server pushes
//! `{"type":"stats",…}` messages over a WebSocket instead of being polled.
//! `stats_url` is the ws:// or wss:// address; the token (stored like an API
//! key) is added as `?token=` unless the URL has one. A message holds one stream
//! or a `streams` list; the feed (the `publisher` setting, else the URL's
//! `?feed=`) picks one by `feed`, `streamId` or its last path segment, else the
//! first active stream is used.
//!
//! A task keeps the connection (reconnecting with a backoff, and after 10 s of
//! silence) and keeps the latest message; the usual once-a-second poll reports
//! it, and no message for 3 s reads as offline. Servers of this kind drop to
//! LOW/BRB at once and wait to go back to LIVE (see [`super::pushes_stats`]).

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures_util::StreamExt as _;
use serde_json::Value;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};

use super::{Error, Fetcher, MAX_SIZE, Result, Source, report};
use crate::switcher::{SwitchType, Triggers};

/// No message for this long: offline.
const STALE_AFTER: Duration = Duration::from_secs(3);
/// No message for this long: reconnect.
const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(16);

/// One stream from a `stats` message.
#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub stream_id: Option<String>,
    pub feed: Option<String>,
    /// kbps
    pub bitrate: u32,
    /// ms
    pub rtt: Option<f64>,
    pub connected: bool,
}

impl Stat {
    fn offline(feed: &str) -> Self {
        Self {
            stream_id: Some(feed.to_owned()),
            feed: Some(feed.to_owned()),
            bitrate: 0,
            rtt: None,
            connected: false,
        }
    }

    fn matches(&self, feed: &str) -> bool {
        self.feed.as_deref() == Some(feed)
            || self.stream_id.as_deref() == Some(feed)
            || self.stream_id.as_deref().and_then(feed_from_stream_id) == Some(feed)
    }
}

struct Latest {
    stat: Stat,
    raw: Value,
    at: Instant,
}

type Shared = Arc<Mutex<Option<Latest>>>;

pub struct IrlhostingWebsocket {
    source: Source,
    latest: Shared,
}

impl IrlhostingWebsocket {
    pub async fn run(source: Source) {
        let stats_url = &source.server.stats_url;
        let url = url_with_token(stats_url, source.secrets.api_key.as_deref());
        let feed = source
            .server
            .details()
            .publisher
            .or_else(|| query_value(stats_url, "feed"))
            .filter(|feed| !feed.is_empty());
        let latest = Shared::default();
        let connection = keep_connected(
            url,
            feed,
            latest.clone(),
            source.server.name.clone(),
            source.allow_private,
        );

        // Both run until the switcher aborts the task.
        tokio::join!(connection, super::poll(Self { source, latest }));
    }
}

async fn keep_connected(
    url: String,
    feed: Option<String>,
    latest: Shared,
    name: String,
    allow_private: bool,
) {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_SIZE))
        .max_frame_size(Some(MAX_SIZE));
    let mut backoff = BACKOFF_MIN;

    loop {
        // Errors never include the URL, which holds the token.
        match timeout(CONNECT_TIMEOUT, connect(&url, config, allow_private)).await {
            Ok(Ok(mut socket)) => {
                tracing::debug!(stream_server = %name, "Connected to the stats WebSocket");
                loop {
                    let text = match timeout(IDLE_TIMEOUT, socket.next()).await {
                        Ok(Some(Ok(Message::Text(text)))) => text.as_str().to_owned(),
                        Ok(Some(Ok(Message::Binary(bytes)))) => {
                            match String::from_utf8(bytes.to_vec()) {
                                Ok(text) => text,
                                Err(_) => continue,
                            }
                        }
                        Ok(Some(Ok(_))) => continue,
                        Ok(Some(Err(error))) => {
                            tracing::debug!(stream_server = %name, %error, "Stats WebSocket failed");
                            break;
                        }
                        Ok(None) => break,
                        Err(_) => {
                            tracing::debug!(stream_server = %name, "No stats for 10 s, reconnecting");
                            break;
                        }
                    };
                    if let Some((stat, raw)) = parse(&text, feed.as_deref()) {
                        backoff = BACKOFF_MIN;
                        if let Ok(mut latest) = latest.lock() {
                            *latest = Some(Latest {
                                stat,
                                raw,
                                at: Instant::now(),
                            });
                        }
                    }
                }
            }
            Ok(Err(error)) => {
                tracing::debug!(stream_server = %name, %error, "Couldn't connect to the stats WebSocket");
            }
            Err(_) => {
                tracing::debug!(stream_server = %name, "Timed out connecting to the stats WebSocket");
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// Opens the socket on an address checked to be public (a name is looked up
/// here, not by the WebSocket library, so it can't answer differently the
/// second time), then does the handshake over it.
async fn connect(
    url: &str,
    config: WebSocketConfig,
    allow_private: bool,
) -> std::result::Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let parsed = reqwest::Url::parse(url)?;
    let host = parsed.host_str().ok_or("no host")?;
    let port = parsed.port_or_known_default().ok_or("no port")?;
    let addresses = common::net::resolve(host, port, allow_private).await?;
    let stream = tokio::net::TcpStream::connect(addresses.as_slice()).await?;
    let (socket, _) =
        tokio_tungstenite::client_async_tls_with_config(url, stream, Some(config), None).await?;
    Ok(socket)
}

/// The stream to use from a message, and the message itself (the raw stats).
/// Anything but a `stats` message is ignored.
pub fn parse(text: &str, feed: Option<&str>) -> Option<(Stat, Value)> {
    let raw: Value = serde_json::from_str(text).ok()?;
    if raw.get("type").and_then(Value::as_str) != Some("stats") {
        return None;
    }

    let stat = match raw.get("streams").and_then(Value::as_array) {
        Some(streams) => {
            let stats = streams.iter().filter_map(parse_stream);
            match feed {
                // Not in the list: that feed isn't publishing.
                Some(feed) => stats
                    .into_iter()
                    .find(|stat| stat.matches(feed))
                    .unwrap_or_else(|| Stat::offline(feed)),
                None => {
                    let stats: Vec<Stat> = stats.collect();
                    stats
                        .iter()
                        .find(|stat| stat.connected && stat.bitrate > 0)
                        .or(stats.first())
                        .cloned()?
                }
            }
        }
        None => parse_stream(&raw).filter(|stat| feed.is_none_or(|feed| stat.matches(feed)))?,
    };

    Some((stat, raw))
}

fn parse_stream(value: &Value) -> Option<Stat> {
    let bitrate = value.get("bitrate")?;
    let bitrate = bitrate
        .as_u64()
        .or_else(|| bitrate.as_f64().map(|kbps| kbps.max(0.0).round() as u64))?;
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let stream_id = text("streamId");
    let feed = text("feed").or_else(|| {
        stream_id
            .as_deref()
            .and_then(feed_from_stream_id)
            .map(str::to_owned)
    });

    Some(Stat {
        stream_id,
        feed,
        bitrate: u32::try_from(bitrate).unwrap_or(u32::MAX),
        rtt: value.get("rtt").and_then(Value::as_f64),
        connected: value
            .get("connected")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

fn feed_from_stream_id(stream_id: &str) -> Option<&str> {
    stream_id.rsplit('/').next().filter(|feed| !feed.is_empty())
}

fn query_value(url: &str, key: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()?
        .query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

/// The URL with `?token=` added, unless it has one already.
pub fn url_with_token(url: &str, token: Option<&str>) -> String {
    let Some(token) = token.filter(|token| !token.is_empty()) else {
        return url.to_owned();
    };
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url.to_owned();
    };
    if parsed.query_pairs().any(|(name, _)| name == "token") {
        return url.to_owned();
    }
    parsed.query_pairs_mut().append_pair("token", token);
    parsed.into()
}

pub fn switch_type(stat: &Stat, triggers: &Triggers) -> SwitchType {
    if !stat.connected {
        return SwitchType::Offline;
    }

    if let Some(offline) = triggers.offline
        && stat.bitrate > 0
        && stat.bitrate <= offline
    {
        return SwitchType::Offline;
    }

    if let Some(rtt_offline) = triggers.rtt_offline
        && let Some(rtt) = stat.rtt
        && rtt >= rtt_offline.into()
    {
        return SwitchType::Offline;
    }

    if stat.bitrate == 0 {
        return SwitchType::Previous;
    }

    if let Some(low) = triggers.low
        && stat.bitrate <= low
    {
        return SwitchType::Low;
    }

    if let Some(rtt_low) = triggers.rtt
        && let Some(rtt) = stat.rtt
        && rtt >= rtt_low.into()
    {
        return SwitchType::Low;
    }

    SwitchType::Normal
}

impl Fetcher for IrlhostingWebsocket {
    const KIND: &'static str = "IRLHosting WebSocket";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let latest = {
            let latest = self.latest.lock().map_err(|_| Error::NotReady)?;
            let latest = latest.as_ref().ok_or(Error::NotReady)?;
            (latest.at.elapsed() <= STALE_AFTER).then(|| (latest.stat.clone(), latest.raw.clone()))
        };

        match latest {
            Some((stat, raw)) => {
                let switch_type = switch_type(&stat, &self.source.triggers().await);
                report(&self.source, stat.bitrate, stat.rtt, switch_type, raw);
            }
            // Gone quiet: offline, like NOALBS (no raw stats to keep).
            None => report(&self.source, 0, None, SwitchType::Offline, Value::Null),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Stat, parse, switch_type, url_with_token};
    use crate::{stream_servers::test_triggers as triggers, switcher::SwitchType};

    fn stat(text: &str, feed: Option<&str>) -> Stat {
        parse(text, feed).unwrap().0
    }

    #[test]
    fn reads_a_single_stream() {
        let text = r#"{"type":"stats","timestamp":1710000000000,"streamId":"publish/live/feed2","bitrate":6200,"packetLoss":0,"rtt":80,"connected":true}"#;
        let stat = stat(text, Some("feed2"));
        assert_eq!(stat.feed.as_deref(), Some("feed2"));
        assert_eq!((stat.bitrate, stat.rtt), (6200, Some(80.0)));
        assert!(stat.connected);

        // Another feed than the one configured: ignored.
        assert!(parse(text, Some("feed1")).is_none());
        assert!(parse(r#"{"type":"sls_stats_xml"}"#, None).is_none());
        assert!(parse("not json", None).is_none());
    }

    #[test]
    fn picks_the_feed_from_a_list() {
        let text = r#"{"type":"stats","streams":[
            {"streamId":"publish/live/feed1","feed":"feed1","bitrate":300,"connected":true},
            {"streamId":"publish/live/cameraA","feed":"cameraA","bitrate":6200,"connected":true}]}"#;
        assert_eq!(stat(text, Some("cameraA")).bitrate, 6200);
        assert_eq!(stat(text, Some("publish/live/feed1")).bitrate, 300);
    }

    #[test]
    fn without_a_feed_the_first_active_stream() {
        let text = r#"{"type":"stats","streams":[
            {"streamId":"publish/live/feed1","bitrate":0,"connected":true},
            {"streamId":"publish/live/feed2","bitrate":6200,"connected":true}]}"#;
        let active = stat(text, None);
        assert_eq!(
            (active.feed.as_deref(), active.bitrate),
            (Some("feed2"), 6200)
        );

        let idle = r#"{"type":"stats","streams":[{"streamId":"publish/live/feed1","bitrate":0}]}"#;
        assert_eq!(stat(idle, None).bitrate, 0);
        assert!(parse(r#"{"type":"stats","streams":[]}"#, None).is_none());
    }

    #[test]
    fn a_missing_feed_is_offline() {
        let stat = stat(r#"{"type":"stats","streams":[]}"#, Some("feed1"));
        assert!(!stat.connected);
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Offline);
    }

    #[test]
    fn switches_like_noalbs() {
        let mut stat = stat(
            r#"{"type":"stats","streamId":"feed1","bitrate":100,"rtt":80}"#,
            None,
        );
        // Under the BRB trigger (150).
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Offline);
        stat.bitrate = 600;
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Low);
        stat.bitrate = 6000;
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Normal);
        stat.rtt = Some(3000.0);
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Low);
        stat.bitrate = 0;
        stat.rtt = None;
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Previous);
    }

    #[tokio::test]
    async fn keeps_the_latest_message() {
        use futures_util::{SinkExt as _, StreamExt as _};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        crate::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            for text in [
                r#"{"type":"stats","feed":"feed2","bitrate":100}"#,
                r#"{"type":"stats","feed":"feed1","bitrate":5000}"#,
            ] {
                socket.send(text.into()).await.unwrap();
            }
            let _ = socket.next().await;
        });

        let latest = super::Shared::default();
        let connection = super::keep_connected(
            format!("ws://127.0.0.1:{port}/"),
            Some("feed1".into()),
            latest.clone(),
            "test".into(),
            true,
        );
        let bitrate = async {
            loop {
                if let Some(latest) = latest.lock().unwrap().as_ref() {
                    return latest.stat.bitrate;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };
        let bitrate = tokio::select! {
            bitrate = bitrate => bitrate,
            _ = connection => unreachable!(),
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => panic!("no stats"),
        };
        // feed2's message was ignored.
        assert_eq!(bitrate, 5000);
    }

    #[test]
    fn adds_the_token_once() {
        assert_eq!(
            url_with_token("wss://stats.example/ws?feed=feed1", Some("s3cret")),
            "wss://stats.example/ws?feed=feed1&token=s3cret"
        );
        assert_eq!(
            url_with_token("wss://stats.example/ws?token=mine", Some("s3cret")),
            "wss://stats.example/ws?token=mine"
        );
        assert_eq!(
            url_with_token("wss://stats.example/ws", None),
            "wss://stats.example/ws"
        );
    }
}
