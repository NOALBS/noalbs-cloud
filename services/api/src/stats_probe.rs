//! "Test connection" for a stream server: fetch its stats URL once and say
//! what came back (for a WebSocket kind: connect and wait for the first stats
//! message). The URL comes from the user, so private and loopback addresses are
//! refused unless the config allows them (local development).

use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use futures_util::StreamExt as _;
use reqwest::{StatusCode, Url, redirect};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::timeout_at;
use tokio_tungstenite::tungstenite::{self, Message, protocol::WebSocketConfig};
use utoipa::ToSchema;

use db::stream_servers::Details;

use crate::{routes::stream_servers::StreamServerKindDto, server_details::Secrets};

const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Serialize, ToSchema)]
pub struct StatsProbe {
    /// The stats page answered with something we could read.
    pub reachable: bool,
    /// A video feed is coming in right now.
    pub receiving: bool,
    pub http_status: Option<u16>,
    pub elapsed_ms: Option<u32>,
    /// Only for kinds that report it in a single request.
    pub bitrate: Option<u32>,
    pub rtt: Option<f64>,
    /// A sentence for people: what worked, or what to check. Never names the
    /// host: the dashboard may be on stream.
    pub message: String,
}

impl StatsProbe {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            reachable: false,
            receiving: false,
            http_status: None,
            elapsed_ms: None,
            bitrate: None,
            rtt: None,
            message: message.into(),
        }
    }
}

/// What to send to a kind's stats endpoint (as obs-link does).
#[derive(Debug)]
pub struct ProbeRequest {
    pub url: String,
    /// POST this JSON instead of a GET (Xiu).
    pub json: Option<serde_json::Value>,
    pub basic_auth: Option<(String, Option<String>)>,
    /// Sent as `Authorization` (SLS).
    pub api_key: Option<String>,
}

impl ProbeRequest {
    pub fn get(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            json: None,
            basic_auth: None,
            api_key: None,
        }
    }
}

/// Kinds whose stats URL is a WebSocket (ws:// or wss://) that pushes stats.
pub fn is_websocket(kind: StreamServerKindDto) -> bool {
    kind == StreamServerKindDto::IrlhostingWebsocket
}

/// The URL with `?token=` added, unless it has one already (as obs-link does).
fn url_with_token(url: &str, token: Option<&str>) -> String {
    let Some(token) = token.filter(|token| !token.is_empty()) else {
        return url.to_owned();
    };
    let Ok(mut parsed) = Url::parse(url) else {
        return url.to_owned();
    };
    if parsed.query_pairs().any(|(name, _)| name == "token") {
        return url.to_owned();
    }
    parsed.query_pairs_mut().append_pair("token", token);
    parsed.into()
}

fn join_url(base: &str, segments: &[&str]) -> String {
    let mut url = base.trim_end_matches('/').to_owned();
    for segment in segments {
        url.push('/');
        url.push_str(segment.trim_matches('/'));
    }
    url
}

/// The request obs-link makes for this kind and these settings.
pub fn request_for(
    kind: StreamServerKindDto,
    stats_url: &str,
    details: &Details,
    secrets: &Secrets,
) -> ProbeRequest {
    use StreamServerKindDto as K;

    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    let mut request = ProbeRequest::get(stats_url);
    match kind {
        K::Nms => {
            request.url = join_url(
                stats_url,
                &[&text(&details.application), &text(&details.key)],
            )
        }
        K::Nimble => request.url = join_url(stats_url, &["manage", "srt_receiver_stats"]),
        K::Xiu => {
            request.json = Some(serde_json::json!({
                "identifier": { "rtmp": {
                    "app_name": text(&details.application),
                    "stream_name": text(&details.key),
                } }
            }));
        }
        K::Sls => request.api_key = secrets.api_key.clone(),
        K::IrlhostingWebsocket => {
            request.url = url_with_token(stats_url, secrets.api_key.as_deref())
        }
        _ => {}
    }
    if let Some(username) = &details.username {
        request.basic_auth = Some((username.clone(), secrets.password.clone()));
    }
    request
}

pub async fn probe(
    kind: StreamServerKindDto,
    request: ProbeRequest,
    details: &Details,
    allow_private: bool,
) -> StatsProbe {
    let Ok(url) = Url::parse(&request.url) else {
        return StatsProbe::failed("That isn't a valid URL.");
    };
    let websocket = is_websocket(kind);
    if websocket && !matches!(url.scheme(), "ws" | "wss") {
        return StatsProbe::failed("Use a ws:// or wss:// URL.");
    }
    if !websocket && !matches!(url.scheme(), "http" | "https") {
        return StatsProbe::failed("Use an http:// or https:// URL.");
    }
    let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
        return StatsProbe::failed("That URL has no host.");
    };
    let addresses = match common::net::resolve(host, port, allow_private).await {
        Ok(addresses) => addresses,
        Err(common::net::ResolveError::NotFound) => {
            return StatsProbe::failed("Couldn't find that address. Check the host name.");
        }
        Err(common::net::ResolveError::Private) => {
            return StatsProbe::failed(
                "That address is on a private network, so NOALBS Cloud can't reach it. \
                 Use an address that's reachable from the internet.",
            );
        }
    };
    let address = addresses[0];
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();

    if websocket {
        return probe_websocket(url, address, details).await;
    }

    // Connect to the address we checked, not whatever a second lookup returns.
    let client = match reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(redirect::Policy::none())
        .resolve(&host, address)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "Failed to build the stats probe client");
            return StatsProbe::failed("Couldn't run the test. Try again.");
        }
    };

    let started = Instant::now();
    let mut builder = match &request.json {
        Some(json) => client.post(url).json(json),
        None => client.get(url),
    };
    if let Some((username, password)) = &request.basic_auth {
        builder = builder.basic_auth(username, password.as_ref());
    }
    if let Some(api_key) = &request.api_key {
        builder = builder.header(reqwest::header::AUTHORIZATION, api_key);
    }
    let response = match builder.send().await {
        Ok(response) => response,
        Err(error) if error.is_timeout() => {
            return StatsProbe::failed("No answer within 5 seconds.");
        }
        Err(_) => {
            return StatsProbe::failed(
                "Couldn't connect. Check the address and port, and that the server is running.",
            );
        }
    };
    let status = response.status();
    let body = read_limited(response).await;
    let elapsed_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);

    let mut result = interpret(kind, status, body.as_deref(), details);
    result.http_status = Some(status.as_u16());
    result.elapsed_ms = Some(elapsed_ms);
    result
}

/// Opens the WebSocket at the checked address and waits for the first stats
/// message about the feed, all within [`TIMEOUT`].
async fn probe_websocket(url: Url, address: SocketAddr, details: &Details) -> StatsProbe {
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let feed = details
        .publisher
        .clone()
        .or_else(|| {
            url.query_pairs()
                .find(|(name, _)| name == "feed")
                .map(|(_, value)| value.into_owned())
        })
        .filter(|feed| !feed.is_empty());
    let with_elapsed = |mut result: StatsProbe| {
        result.elapsed_ms = Some(u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX));
        result
    };

    let stream = match timeout_at(deadline, tokio::net::TcpStream::connect(address)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(_)) => {
            return StatsProbe::failed(
                "Couldn't connect. Check the address and port, and that the server is running.",
            );
        }
        Err(_) => return StatsProbe::failed("No answer within 5 seconds."),
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_BODY))
        .max_frame_size(Some(MAX_BODY));
    let handshake =
        tokio_tungstenite::client_async_tls_with_config(url.as_str(), stream, Some(config), None);
    let mut socket = match timeout_at(deadline, handshake).await {
        Ok(Ok((socket, _))) => socket,
        Ok(Err(tungstenite::Error::Http(response))) => {
            let status = response.status();
            let mut result = if status.is_redirection() {
                StatsProbe::failed(format!(
                    "HTTP {} redirect. Use the final URL.",
                    status.as_u16()
                ))
            } else if matches!(status.as_u16(), 401 | 403) {
                StatsProbe::failed(format!(
                    "The server refused the token (HTTP {}). Check the token.",
                    status.as_u16()
                ))
            } else {
                StatsProbe::failed(format!(
                    "The server answered HTTP {} instead of opening a WebSocket. Check the URL.",
                    status.as_u16()
                ))
            };
            result.http_status = Some(status.as_u16());
            return with_elapsed(result);
        }
        Ok(Err(_)) => {
            return StatsProbe::failed(
                "Couldn't open a WebSocket there. Check the address, and ws:// or wss://.",
            );
        }
        Err(_) => return StatsProbe::failed("No answer within 5 seconds."),
    };

    loop {
        let text = match timeout_at(deadline, socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => text.as_str().to_owned(),
            Ok(Some(Ok(Message::Binary(bytes)))) => match String::from_utf8(bytes.to_vec()) {
                Ok(text) => text,
                Err(_) => continue,
            },
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(_)) | None) => {
                return with_elapsed(StatsProbe::failed(
                    "The WebSocket closed before sending stats. Check the token.",
                ));
            }
            Err(_) => {
                return with_elapsed(StatsProbe {
                    reachable: true,
                    ..StatsProbe::failed(
                        "Connected, but no stats for this feed arrived within 5 seconds.",
                    )
                });
            }
        };
        if let Some(result) = interpret_websocket(&text, feed.as_deref()) {
            return with_elapsed(result);
        }
    }
}

/// What a WebSocket message says about the feed (the first active stream
/// without one); `None` for other messages, or a single stream of another feed.
fn interpret_websocket(text: &str, feed: Option<&str>) -> Option<StatsProbe> {
    let message: Value = serde_json::from_str(text).ok()?;
    if message.get("type").and_then(Value::as_str) != Some("stats") {
        return None;
    }
    fn field<'a>(stream: &'a Value, key: &str) -> Option<&'a str> {
        stream.get(key).and_then(Value::as_str)
    }
    let is_feed = |stream: &Value| {
        feed.is_none_or(|feed| {
            field(stream, "feed") == Some(feed)
                || field(stream, "streamId")
                    .is_some_and(|id| id == feed || id.rsplit('/').next() == Some(feed))
        })
    };
    let bitrate = |stream: &Value| stream.get("bitrate").and_then(Value::as_f64);
    let receiving = |stream: &&Value| {
        stream.get("connected").and_then(Value::as_bool) != Some(false)
            && bitrate(stream).is_some_and(|kbps| kbps > 0.0)
    };

    let streams: Vec<&Value> = match message.get("streams").and_then(Value::as_array) {
        Some(streams) => streams.iter().filter(|stream| is_feed(stream)).collect(),
        None if is_feed(&message) && bitrate(&message).is_some() => vec![&message],
        None => return None,
    };
    let answered = |message: &str| StatsProbe {
        reachable: true,
        ..StatsProbe::failed(message)
    };
    Some(match streams.into_iter().find(receiving) {
        Some(stream) => StatsProbe {
            receiving: true,
            bitrate: bitrate(stream).map(|kbps| kbps.round() as u32),
            rtt: stream.get("rtt").and_then(Value::as_f64),
            ..answered("Receiving video.")
        },
        None if feed.is_some() => {
            answered("Connected. Nothing is publishing to that feed right now.")
        }
        None => answered("Connected. Nothing is publishing right now."),
    })
}

async fn read_limited(mut response: reqwest::Response) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        if body.len() + chunk.len() > MAX_BODY {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

#[derive(Deserialize)]
struct PublisherStats {
    publishers: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct PublisherNumbers {
    bitrate: i64,
    rtt: f64,
}

#[derive(Deserialize)]
struct MediamtxPath {
    ready: bool,
}

#[derive(Deserialize)]
struct OpenRtmpNumbers {
    bitrate_kbps: f64,
    rtt_ms: f64,
}

fn kind_label(kind: StreamServerKindDto) -> &'static str {
    use StreamServerKindDto as K;
    match kind {
        K::Belabox => "BELABOX",
        K::Mediamtx => "MediaMTX",
        K::Sls => "SRT Live Server",
        K::Nginx => "NGINX RTMP",
        K::Nms => "Node Media Server",
        K::Nimble => "Nimble",
        K::Xiu => "Xiu",
        K::Openirl => "OpenIRL",
        K::Openrtmp => "OpenRTMP",
        K::Rist => "RIST",
        K::Irlhosting => "IRLHosting",
        K::IrlhostingWebsocket => "IRLHosting WebSocket",
    }
}

/// What a stats page's answer means, per kind. Pure, for tests.
fn interpret(
    kind: StreamServerKindDto,
    status: StatusCode,
    body: Option<&[u8]>,
    details: &Details,
) -> StatsProbe {
    use StreamServerKindDto as K;

    let answered = |message: String| StatsProbe {
        reachable: true,
        ..StatsProbe::failed(message)
    };
    let not_a_stats_page = || {
        StatsProbe::failed(format!(
            "That doesn't look like a {} stats page.",
            kind_label(kind)
        ))
    };

    if status.is_redirection() {
        return StatsProbe::failed(format!(
            "HTTP {} redirect. Use the final URL.",
            status.as_u16()
        ));
    }
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        let what = if kind == K::Sls {
            "API key"
        } else {
            "username and password"
        };
        return StatsProbe::failed(format!(
            "The server refused the credentials (HTTP {}). Check the {what}.",
            status.as_u16()
        ));
    }
    if kind == K::Mediamtx && status == StatusCode::NOT_FOUND {
        return answered(
            "MediaMTX answered, but that path doesn't exist yet. It appears once your encoder publishes to it."
                .into(),
        );
    }
    if !status.is_success() {
        return StatsProbe::failed(format!(
            "The server answered HTTP {}. Check the URL and key.",
            status.as_u16()
        ));
    }
    let Some(body) = body else {
        return StatsProbe::failed("The page is too large to be a stats page.");
    };

    match kind {
        // Publishers by name: BELABOX (always "live") and SLS.
        K::Belabox | K::Sls => {
            let Ok(stats) = serde_json::from_slice::<PublisherStats>(body) else {
                return not_a_stats_page();
            };
            let wanted = match kind {
                K::Belabox => "live",
                _ => details.publisher.as_deref().unwrap_or_default(),
            };
            match stats.publishers.get(wanted).and_then(|publisher| {
                serde_json::from_value::<PublisherNumbers>(publisher.clone()).ok()
            }) {
                Some(publisher) => StatsProbe {
                    receiving: publisher.bitrate > 0,
                    bitrate: Some(u32::try_from(publisher.bitrate.max(0)).unwrap_or(u32::MAX)),
                    rtt: Some(publisher.rtt),
                    ..answered("Receiving video.".into())
                },
                None => {
                    answered("Reachable. Nothing is publishing to that stream right now.".into())
                }
            }
        }
        K::Mediamtx => match serde_json::from_slice::<MediamtxPath>(body) {
            Ok(path) if path.ready => StatsProbe {
                receiving: true,
                ..answered("Receiving video. Bitrate shows once the switcher runs.".into())
            },
            Ok(_) => answered("Reachable. Nothing is publishing to this path right now.".into()),
            Err(_) => StatsProbe::failed(
                "That doesn't look like a MediaMTX path. Use …/v3/paths/get/<path>.",
            ),
        },
        // Plain text while offline, JSON while live.
        K::Openrtmp => match serde_json::from_slice::<OpenRtmpNumbers>(body) {
            Ok(numbers) => StatsProbe {
                receiving: numbers.bitrate_kbps > 0.0,
                bitrate: Some(numbers.bitrate_kbps.max(0.0).round() as u32),
                rtt: Some(numbers.rtt_ms),
                ..answered("Receiving video.".into())
            },
            Err(_) => answered("Reachable. Nothing is publishing right now.".into()),
        },
        K::Nginx => {
            if String::from_utf8_lossy(body).contains("<rtmp") {
                answered("Reachable. NOALBS finds your stream on it once it's live.".into())
            } else {
                StatsProbe::failed(
                    "That doesn't look like an NGINX RTMP stats page (the rtmp_stat XML).",
                )
            }
        }
        // WebSocket kinds are read by `interpret_websocket`.
        K::Nms
        | K::Nimble
        | K::Xiu
        | K::Openirl
        | K::Rist
        | K::Irlhosting
        | K::IrlhostingWebsocket => {
            if serde_json::from_slice::<serde_json::Value>(body).is_ok() {
                answered(
                    "Reachable. NOALBS reads the stream's numbers once the switcher runs.".into(),
                )
            } else {
                not_a_stats_page()
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use futures_util::StreamExt as _;
    use reqwest::StatusCode;

    use db::stream_servers::Details;

    use super::{ProbeRequest, interpret, interpret_websocket, request_for};
    use crate::{
        routes::stream_servers::StreamServerKindDto::{
            Belabox, IrlhostingWebsocket, Mediamtx, Nginx, Nimble, Nms, Openrtmp, Sls, Xiu,
        },
        server_details::Secrets,
    };

    fn no_secrets() -> Secrets {
        Secrets {
            password: None,
            api_key: None,
        }
    }

    #[test]
    fn builds_each_kinds_request() {
        let rtmp = Details {
            application: Some("live".into()),
            key: Some("feed1".into()),
            ..Details::default()
        };
        let nms = request_for(Nms, "http://nms:8000/api/streams/", &rtmp, &no_secrets());
        assert_eq!(nms.url, "http://nms:8000/api/streams/live/feed1");
        let nimble = request_for(Nimble, "http://nimble:8082", &rtmp, &no_secrets());
        assert_eq!(nimble.url, "http://nimble:8082/manage/srt_receiver_stats");
        let xiu = request_for(Xiu, "http://xiu/api/query_stream", &rtmp, &no_secrets());
        assert_eq!(
            xiu.json.unwrap()["identifier"]["rtmp"]["stream_name"],
            "feed1"
        );

        let secrets = Secrets {
            password: Some("pw".into()),
            api_key: Some("key".into()),
        };
        let authed = Details {
            username: Some("admin".into()),
            ..Details::default()
        };
        let mediamtx = request_for(Mediamtx, "http://m/v3/paths/get/live", &authed, &secrets);
        assert_eq!(
            mediamtx.basic_auth,
            Some(("admin".into(), Some("pw".into())))
        );
        let sls = request_for(Sls, "http://sls/stats", &Details::default(), &secrets);
        assert_eq!(sls.api_key.as_deref(), Some("key"));
        assert_eq!(sls.basic_auth, None);
    }

    #[test]
    fn new_kinds_and_credentials_are_explained() {
        let refused = interpret(
            Mediamtx,
            StatusCode::UNAUTHORIZED,
            Some(b""),
            &Details::default(),
        );
        assert!(!refused.reachable && refused.message.contains("username and password"));
        let refused = interpret(
            Sls,
            StatusCode::UNAUTHORIZED,
            Some(b""),
            &Details::default(),
        );
        assert!(refused.message.contains("API key"));

        let sls = Details {
            publisher: Some("publish/live/feed1".into()),
            ..Details::default()
        };
        let live = interpret(
            Sls,
            StatusCode::OK,
            Some(br#"{"publishers":{"publish/live/feed1":{"bitrate":6000,"rtt":80}}}"#),
            &sls,
        );
        assert!(live.receiving && live.bitrate == Some(6000));

        let offline = interpret(
            Openrtmp,
            StatusCode::OK,
            Some(b"stream offline"),
            &Details::default(),
        );
        assert!(offline.reachable && !offline.receiving);
        let nginx = interpret(
            Nginx,
            StatusCode::OK,
            Some(b"<?xml?><rtmp><server/></rtmp>"),
            &Details::default(),
        );
        assert!(nginx.reachable);
        let html = interpret(Nms, StatusCode::OK, Some(b"<html>"), &Details::default());
        assert!(!html.reachable);
    }

    #[test]
    fn belabox_answers() {
        let live = br#"{"publishers":{"live":{"bitrate":6240,"rtt":182.5}}}"#;
        let probe = interpret(Belabox, StatusCode::OK, Some(live), &Details::default());
        assert!(probe.reachable && probe.receiving);
        assert_eq!((probe.bitrate, probe.rtt), (Some(6240), Some(182.5)));

        let empty = interpret(
            Belabox,
            StatusCode::OK,
            Some(br#"{"publishers":{}}"#),
            &Details::default(),
        );
        assert!(empty.reachable && !empty.receiving);

        let wrong = interpret(
            Belabox,
            StatusCode::OK,
            Some(b"<html>"),
            &Details::default(),
        );
        assert!(!wrong.reachable);

        let denied = interpret(
            Belabox,
            StatusCode::FORBIDDEN,
            Some(b""),
            &Details::default(),
        );
        assert!(!denied.reachable);
        assert!(denied.message.contains("403"));
    }

    #[tokio::test]
    async fn messages_never_name_the_host() {
        for url in [
            "http://secret-host.invalid/stats",
            "http://127.0.0.1:1/stats",
        ] {
            let probe =
                super::probe(Belabox, ProbeRequest::get(url), &Details::default(), true).await;
            assert!(!probe.reachable);
            assert!(!probe.message.contains("secret-host"), "{}", probe.message);
            assert!(!probe.message.contains("127.0.0.1"), "{}", probe.message);
        }
    }

    #[test]
    fn websocket_token_goes_in_the_url() {
        let secrets = Secrets {
            password: None,
            api_key: Some("s3cret".into()),
        };
        let request = request_for(
            IrlhostingWebsocket,
            "wss://stats.example/ws?feed=feed1",
            &Details::default(),
            &secrets,
        );
        assert_eq!(
            request.url,
            "wss://stats.example/ws?feed=feed1&token=s3cret"
        );
        assert_eq!(request.api_key, None);
    }

    #[test]
    fn websocket_messages() {
        let list = r#"{"type":"stats","streams":[
            {"streamId":"publish/live/feed1","bitrate":0},
            {"streamId":"publish/live/feed2","bitrate":6200,"rtt":80,"connected":true}]}"#;
        let first_active = interpret_websocket(list, None).unwrap();
        assert!(first_active.receiving);
        assert_eq!(
            (first_active.bitrate, first_active.rtt),
            (Some(6200), Some(80.0))
        );
        let idle = interpret_websocket(list, Some("feed1")).unwrap();
        assert!(idle.reachable && !idle.receiving);
        let missing = interpret_websocket(list, Some("cameraA")).unwrap();
        assert!(missing.reachable && !missing.receiving);

        let single = r#"{"type":"stats","streamId":"publish/live/feed2","bitrate":6200}"#;
        assert!(
            interpret_websocket(single, Some("feed2"))
                .unwrap()
                .receiving
        );
        // Another feed's stats, or other messages: keep waiting.
        assert!(interpret_websocket(single, Some("feed1")).is_none());
        assert!(interpret_websocket(r#"{"type":"hello"}"#, None).is_none());
    }

    #[tokio::test]
    async fn websocket_probe_reads_the_first_stats_message() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            use futures_util::SinkExt as _;
            for text in [
                r#"{"type":"hello"}"#,
                r#"{"type":"stats","streams":[{"feed":"feed1","bitrate":5000,"rtt":42}]}"#,
            ] {
                socket.send(text.into()).await.unwrap();
            }
            // Hold the connection until the probe has read.
            let _ = socket.next().await;
        });

        let url = format!("ws://127.0.0.1:{port}/ws?feed=feed1");
        let probe = super::probe(
            IrlhostingWebsocket,
            ProbeRequest::get(&url),
            &Details::default(),
            true,
        )
        .await;
        assert!(probe.receiving, "{}", probe.message);
        assert_eq!((probe.bitrate, probe.rtt), (Some(5000), Some(42.0)));

        // Private addresses are refused like any stats URL.
        let refused = super::probe(
            IrlhostingWebsocket,
            ProbeRequest::get(&url),
            &Details::default(),
            false,
        )
        .await;
        assert!(!refused.reachable && refused.message.contains("private"));
        let http = super::probe(
            IrlhostingWebsocket,
            ProbeRequest::get("http://example.net/ws"),
            &Details::default(),
            true,
        )
        .await;
        assert!(http.message.contains("ws://"));
    }

    #[tokio::test]
    async fn websocket_probe_explains_a_refused_token() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await;
            let _ = stream
                .write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\n\r\n")
                .await;
        });

        let probe = super::probe(
            IrlhostingWebsocket,
            ProbeRequest::get(&format!("ws://127.0.0.1:{port}/ws")),
            &Details::default(),
            true,
        )
        .await;
        assert!(!probe.reachable);
        assert_eq!(probe.http_status, Some(401));
        assert!(probe.message.contains("token"), "{}", probe.message);
    }

    #[test]
    fn mediamtx_answers() {
        let ready = interpret(
            Mediamtx,
            StatusCode::OK,
            Some(br#"{"ready":true}"#),
            &Details::default(),
        );
        assert!(ready.reachable && ready.receiving);

        let missing = interpret(
            Mediamtx,
            StatusCode::NOT_FOUND,
            Some(b""),
            &Details::default(),
        );
        assert!(missing.reachable && !missing.receiving);

        let redirect = interpret(
            Mediamtx,
            StatusCode::MOVED_PERMANENTLY,
            None,
            &Details::default(),
        );
        assert!(!redirect.reachable);
    }
}
