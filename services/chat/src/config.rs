use crate::error::Result;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// The HTTP server (the Kick webhook and `/health`).
    pub host: String,
    pub port: u16,

    pub nats_url: String,
    /// Milliseconds to wait for OBS to answer.
    pub nats_request_timeout: u64,

    pub postgres_url: String,
    pub postgres_max_connections: u32,

    /// Hex-encoded 32-byte key that decrypts stored chat tokens. Must match the
    /// API's `data_encryption_key`.
    pub data_encryption_key: String,

    /// Names this instance in logs and shard claims; random when unset. If set,
    /// it must differ per running instance (a restart with the same ID takes
    /// its shards straight back).
    pub instance_id: Option<String>,

    /// The Twitch app, the same one the API signs in with (the streamers'
    /// `channel:bot` grants belong to it). Twitch chat is off without it.
    pub twitch: Option<TwitchConfig>,
    /// The Kick app, the same one the API signs in with. Kick chat is off without it.
    pub kick: Option<KickConfig>,
    /// The Google app (YouTube Data API), the same one the API signs in with.
    /// YouTube chat is off without it.
    pub youtube: Option<YouTubeConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 13000,
            nats_url: "nats://localhost:4222".into(),
            nats_request_timeout: 5000,
            postgres_url: "postgres://postgres:postgres@localhost/localdb".into(),
            postgres_max_connections: 10,
            data_encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .into(),
            instance_id: None,
            twitch: None,
            kick: None,
            youtube: None,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TwitchConfig {
    pub client_id: String,
    pub client_secret: String,
    /// Shards of the EventSub conduit, spread over the running instances. Each
    /// is one WebSocket; a shard carries at most 10,000 subscriptions' worth of
    /// traffic comfortably. Changing it resizes the conduit.
    #[serde(default = "default_shards")]
    pub shards: u32,
    /// For tests against a fake Twitch.
    #[serde(default = "default_twitch_api")]
    pub api_base: String,
    #[serde(default = "default_twitch_auth")]
    pub auth_base: String,
    #[serde(default = "default_twitch_eventsub")]
    pub eventsub_url: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct KickConfig {
    pub client_id: String,
    pub client_secret: String,
    /// For tests against a fake Kick.
    #[serde(default = "default_kick_api")]
    pub api_base: String,
    #[serde(default = "default_kick_auth")]
    pub auth_base: String,
    /// Kick's site WebSocket (Pusher), for raids: Kick has no webhook event for them.
    #[serde(default = "default_kick_pusher")]
    pub pusher_url: String,
    /// Kick's site, whose unofficial API has the channel and chatroom IDs Pusher uses.
    #[serde(default = "default_kick_web")]
    pub web_base: String,
    /// PEM public key webhooks are checked against, instead of Kick's (fetched
    /// from its API). Only for tests with self-signed webhooks.
    pub public_key: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct YouTubeConfig {
    pub client_id: String,
    pub client_secret: String,
    /// For looking channels up by handle (`!noalbs ignore`); without it that
    /// isn't available on YouTube.
    pub api_key: Option<String>,
    /// Seconds between looking for a streaming channel's live chat (1 quota
    /// unit each), once the first few quick tries found none.
    #[serde(default = "default_youtube_live_check")]
    pub live_check_interval: u64,
    /// For tests against a fake YouTube.
    #[serde(default = "default_youtube_api")]
    pub api_base: String,
    /// Where `/token` is.
    #[serde(default = "default_youtube_auth")]
    pub auth_base: String,
    /// The `StreamList` gRPC server (`http://` for plaintext HTTP/2).
    #[serde(default = "default_youtube_grpc")]
    pub grpc_url: String,
}

fn default_shards() -> u32 {
    4
}
fn default_twitch_api() -> String {
    "https://api.twitch.tv".into()
}
fn default_twitch_auth() -> String {
    "https://id.twitch.tv".into()
}
fn default_twitch_eventsub() -> String {
    "wss://eventsub.wss.twitch.tv/ws".into()
}
fn default_kick_api() -> String {
    "https://api.kick.com".into()
}
fn default_kick_auth() -> String {
    "https://id.kick.com".into()
}
fn default_kick_pusher() -> String {
    "wss://ws-us2.pusher.com/app/34bf7a0ff419a2a775b9?protocol=7&client=js&version=7.6.0&flash=false"
        .into()
}
fn default_kick_web() -> String {
    "https://kick.com".into()
}
fn default_youtube_live_check() -> u64 {
    60
}
fn default_youtube_api() -> String {
    "https://www.googleapis.com".into()
}
fn default_youtube_auth() -> String {
    "https://oauth2.googleapis.com".into()
}
fn default_youtube_grpc() -> String {
    "https://youtube.googleapis.com".into()
}

/// `config.toml` (optional) plus `CHAT_*` environment variables; see `common::settings`.
pub fn load_config() -> Result<Config> {
    let config: Config = common::settings::load("CHAT")?;
    common::settings::require_real_key("data_encryption_key", &config.data_encryption_key)?;
    if let Some(twitch) = &config.twitch
        && !(1..=20_000).contains(&twitch.shards)
    {
        return Err(config::ConfigError::Message("twitch.shards must be 1 to 20000".into()).into());
    }
    Ok(config)
}
