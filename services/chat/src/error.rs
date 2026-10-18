pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Database error: {0}")]
    Database(#[from] db::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Data encryption key: {0}")]
    Crypto(#[from] common::crypto::CryptoError),

    #[error("NATS error: {0}")]
    Nats(String),

    /// A platform API answered with an error status.
    #[error("{platform} answered {status}: {body}")]
    Platform {
        platform: &'static str,
        status: reqwest::StatusCode,
        body: String,
    },

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn nats(error: impl std::fmt::Display) -> Self {
        Self::Nats(error.to_string())
    }

    /// The platform refused our credentials (the grant is gone).
    pub fn is_unauthorized(&self) -> bool {
        matches!(
            self,
            Self::Platform { status, .. }
                if *status == reqwest::StatusCode::UNAUTHORIZED
                    || *status == reqwest::StatusCode::FORBIDDEN
        )
    }

    pub fn status(&self) -> Option<reqwest::StatusCode> {
        match self {
            Self::Platform { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// Turns a non-success response into [`Error::Platform`].
pub async fn check(
    platform: &'static str,
    response: reqwest::Response,
) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let mut body = response.text().await.unwrap_or_default();
    body.truncate(500);
    Err(Error::Platform {
        platform,
        status,
        body,
    })
}
