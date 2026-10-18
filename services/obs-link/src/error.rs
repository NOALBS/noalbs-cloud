use crate::stream_servers;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("NATS error: {0}")]
    NatsConnect(#[from] async_nats::error::Error<async_nats::ConnectErrorKind>),

    #[error("NATS publish error: {0}")]
    NatsPublish(#[from] async_nats::error::Error<async_nats::PublishErrorKind>),

    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("Database error: {0}")]
    Database(#[from] db::Error),

    #[error("Stream server stats error: {0}")]
    StreamServer(#[from] stream_servers::Error),

    #[error("OBS error: {0}")]
    Obs(#[from] obs_actions::Error),

    #[error("NATS error: {0}")]
    Nats(String),

    #[error("Data encryption key: {0}")]
    Crypto(#[from] common::crypto::CryptoError),

    #[error("The switcher no longer exists")]
    SwitcherGone,
}
