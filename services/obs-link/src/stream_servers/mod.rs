//! Stats fetchers, one module per kind of stream server. Each runs as its own
//! task while the switcher runs: once a second it fetches the server's stats,
//! turns them into a [`switcher::SwitchType`] with the switcher's triggers and
//! sends a [`StreamServerStats`] to the switcher loop. The switching rules per
//! kind follow NOALBS v2.

pub mod belabox;
pub mod irlhosting;
pub mod irlhosting_websocket;
pub mod mediamtx;
pub mod nginx;
pub mod nimble;
pub mod nms;
pub mod openirl;
pub mod openrtmp;
pub mod rist;
pub mod sls;
pub mod xiu;

pub use belabox::Belabox;
pub use irlhosting::Irlhosting;
pub use irlhosting_websocket::IrlhostingWebsocket;
pub use mediamtx::Mediamtx;
pub use nginx::Nginx;
pub use nimble::Nimble;
pub use nms::NodeMediaServer;
pub use openirl::OpenIrl;
pub use openrtmp::OpenRtmp;
pub use rist::Rist;
pub use sls::SrtLiveServer;
pub use xiu::Xiu;

use std::{sync::Arc, time::Duration};

use common::crypto::DataKey;
use db::stream_servers::{Details, StreamServer, StreamServerKind};
use http_body_util::BodyExt as _;
use tokio::{
    sync::{RwLock, mpsc},
    time,
};

use crate::switcher;

pub const MAX_SIZE: usize = 1024 * 1024;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Server is unreachable")]
    Unreachable,

    #[error("Server is not accessible")]
    NotAccessible,

    #[error("Page is too large")]
    PageTooLarge,

    #[error("Invalid publisher data")]
    InvalidPublisher,

    #[error("Not ready")]
    NotReady,

    #[error("Skipped due to previous stats being the same")]
    Skipped,

    #[error("Failed to deserialize response")]
    Deserialize,
}

/// The statistics fetched from a stream server to be sent to the switcher loop
/// and stored per stream session for time-series analysis and debugging.
#[derive(Debug, Clone)]
pub struct StreamServerStats {
    /// The ID of the stream server that these stats were fetched from
    pub stream_server_id: uuid::Uuid,

    /// The bitrate of the stream in kbps
    pub bitrate: u32,

    /// The round-trip time of the stream in milliseconds
    pub rtt: Option<f64>,

    pub switch_type: switcher::SwitchType,

    /// The raw JSON stats fetched from the stream server
    pub raw_stats: serde_json::Value,

    /// The timestamp of when these stats were fetched
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// What every fetcher gets from the switcher.
pub struct Source {
    pub server: StreamServer,
    pub reqwest_client: reqwest::Client,
    pub stats_tx: mpsc::UnboundedSender<StreamServerStats>,
    pub triggers: Arc<RwLock<switcher::Triggers>>,
    pub secrets: Secrets,
    /// Lets stats URLs point at private addresses (local development).
    pub allow_private: bool,
}

impl Source {
    /// The current triggers (the switcher may change them).
    pub async fn triggers(&self) -> switcher::Triggers {
        self.triggers.read().await.clone()
    }
}

/// The secrets in a server's `details`, decrypted. A secret that can't be
/// decrypted (another key, or tampered with) is left out and logged.
#[derive(Clone, Default)]
pub struct Secrets {
    pub password: Option<String>,
    pub api_key: Option<String>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secrets(..)")
    }
}

impl Secrets {
    pub fn decrypt(server: &StreamServer, key: &DataKey) -> Self {
        let details = server.details();
        let decrypt = |what: &str, stored: Option<&String>| {
            let stored = stored?;
            match key.decrypt_string(stored) {
                Ok(value) => Some(value),
                Err(error) => {
                    tracing::warn!(%error, stream_server = %server.name, "Couldn't decrypt the {what}");
                    None
                }
            }
        };

        Self {
            password: decrypt("password", details.encrypted_password.as_ref()),
            api_key: decrypt("API key", details.encrypted_api_key.as_ref()),
        }
    }
}

/// Adds HTTP basic auth when the server has a username (MediaMTX, NMS).
pub fn with_basic_auth(
    request: reqwest::RequestBuilder,
    details: &Details,
    secrets: &Secrets,
) -> reqwest::RequestBuilder {
    match &details.username {
        Some(username) => request.basic_auth(username, secrets.password.as_ref()),
        None => request,
    }
}

/// The HTTP client for polling stats URLs. A URL is whatever a user typed, so the
/// client looks names up itself and refuses private, loopback and link-local
/// addresses (every time, so a name can't change its answer after it was saved),
/// and follows no redirects (which could lead there). IP addresses in a URL never
/// reach the resolver: see [`url_allowed`].
pub fn http_client(user_agent: &str, allow_private: bool) -> reqwest::Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none());
    if allow_private {
        builder.build()
    } else {
        builder.dns_resolver(Arc::new(PublicOnly)).build()
    }
}

struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let addresses = common::net::resolve(name.as_str(), 0, false).await?;
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Whether a stats URL may be polled: not an IP address on a private network
/// (names are checked as they're looked up, by [`http_client`] or when a WebSocket
/// connects).
pub fn url_allowed(url: &str, allow_private: bool) -> bool {
    allow_private
        || reqwest::Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(common::net::is_private_literal))
            .is_some_and(|private| !private)
}

/// Sends a stats request and reads the body, at most [`MAX_SIZE`].
pub async fn fetch_body(request: reqwest::RequestBuilder) -> Result<Vec<u8>> {
    let response = request.send().await.map_err(|_| Error::Unreachable)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(Error::NotAccessible);
    }

    let body = http_body_util::Limited::new(reqwest::Body::from(response), MAX_SIZE)
        .collect()
        .await
        .map_err(|_| Error::PageTooLarge)?
        .to_bytes();

    Ok(body.to_vec())
}

/// Hands a reading to the switcher loop.
pub fn report(
    source: &Source,
    bitrate: u32,
    rtt: Option<f64>,
    switch_type: switcher::SwitchType,
    raw_stats: serde_json::Value,
) {
    // The switcher may have stopped; it aborts this task shortly.
    let _ = source.stats_tx.send(StreamServerStats {
        stream_server_id: source.server.id,
        bitrate,
        rtt,
        switch_type,
        raw_stats,
        timestamp: chrono::Utc::now(),
    });
}

/// A stream server's stats fetcher.
pub trait Fetcher: Send {
    /// For logs, e.g. "BELABOX".
    const KIND: &'static str;

    fn source(&self) -> &Source;

    /// Fetches the stats once and reports them to the switcher.
    fn fetch(&mut self) -> impl Future<Output = Result<()>> + Send;
}

/// Runs a fetcher once a second until the switcher aborts the task.
pub async fn poll<F: Fetcher>(mut fetcher: F) {
    let mut timer = time::interval(Duration::from_secs(1));

    loop {
        timer.tick().await;
        tracing::trace!(kind = F::KIND, stream_server = %fetcher.source().server.name, "Fetching stats");

        let fetched = fetcher.fetch().await;
        crate::metrics::STATS_FETCHES.add(
            1,
            &[
                opentelemetry::KeyValue::new("kind", F::KIND),
                crate::metrics::outcome(&fetched),
            ],
        );
        if let Err(error) = fetched {
            tracing::debug!(
                kind = F::KIND,
                stream_server = %fetcher.source().server.name,
                %error,
                "Failed to fetch stats"
            );
        }
    }
}

/// Whether the server pushes its stats (IRLHosting WebSocket) rather than being
/// polled. Like NOALBS, such servers drop to LOW/BRB at once and wait
/// `retry_attempts` to go back to LIVE (`decide::Server`).
pub fn pushes_stats(kind: StreamServerKind) -> bool {
    kind == StreamServerKind::IrlhostingWebsocket
}

/// Triggers for tests: LOW at 800 kbps, BRB at 150, LOW over 2500 ms RTT.
#[cfg(test)]
pub fn test_triggers() -> switcher::Triggers {
    switcher::Triggers {
        low: Some(800),
        offline: Some(150),
        rtt: Some(2500),
        rtt_offline: None,
    }
}

/// Joins a base URL and path segments with single slashes.
pub fn join_url(base: &str, segments: &[&str]) -> String {
    let mut url = base.trim_end_matches('/').to_owned();
    for segment in segments {
        url.push('/');
        url.push_str(segment.trim_matches('/'));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::join_url;

    #[test]
    fn joins_urls_without_double_slashes() {
        assert_eq!(
            join_url("http://nms:8000/api/streams/", &["live", "/feed1"]),
            "http://nms:8000/api/streams/live/feed1"
        );
        assert_eq!(
            join_url("http://nimble:8082", &["manage", "rtmp_status"]),
            "http://nimble:8082/manage/rtmp_status"
        );
    }
}

#[cfg(test)]
mod private_address_tests {
    use super::{fetch_body, http_client, url_allowed};

    /// Answers one request with a redirect to `location`.
    async fn redirecting_server(location: &'static str) -> u16 {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer).await;
            let response =
                format!("HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\n\r\n");
            let _ = stream.write_all(response.as_bytes()).await;
        });
        port
    }

    #[test]
    fn private_ip_addresses_in_urls_are_not_polled() {
        assert!(!url_allowed(
            "http://169.254.169.254/latest/meta-data",
            false
        ));
        assert!(!url_allowed("http://[::1]:8080/stats", false));
        assert!(!url_allowed("not a url", false));
        assert!(url_allowed("https://stats.example.com/stats", false));
        assert!(url_allowed("http://1.1.1.1/stats", false));
        assert!(url_allowed("http://127.0.0.1:8080/stats", true));
    }

    #[tokio::test]
    async fn names_that_resolve_to_private_addresses_are_refused() {
        let port = redirecting_server("/").await;
        let client = http_client("test", false).unwrap();
        let request = client.get(format!("http://localhost:{port}/"));
        assert!(fetch_body(request).await.is_err());
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        // With private addresses allowed (so the first hop works), the redirect to
        // the metadata address must still not be followed: the 302 is the answer.
        let port = redirecting_server("http://169.254.169.254/").await;
        let client = http_client("test", true).unwrap();
        let response = client
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    }
}
