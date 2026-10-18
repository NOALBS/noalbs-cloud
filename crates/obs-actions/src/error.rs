pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("NATS error: {0}")]
    Nats(#[from] async_nats::Error),

    #[error("NATS connection error: {0}")]
    NatsConnection(#[from] async_nats::ConnectError),

    #[error("NATS request error: {0}")]
    NatsRequest(#[from] async_nats::RequestError),

    #[error("JSON serialization/deserialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("OBS request error: {0}")]
    ObsRequestError(String),

    #[error("From UTF-8 error: {0}")]
    FromUtf8(#[from] std::string::FromUtf8Error),

    #[error("Request timed out")]
    RequestTimeout,

    #[error("Request ID mismatch: expected {expected}, got {actual}")]
    RequestIdMismatch { expected: String, actual: String },

    #[error("OBS response error (code: {code}, result: {result}, comment: {comment:?})")]
    ObsResponseError {
        code: u32,
        result: bool,
        comment: Option<String>,
    },
}
