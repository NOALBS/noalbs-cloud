pub mod admin;
pub mod admin_actions;
pub mod admin_organizations;
pub mod admin_status;
pub mod auth;
pub mod belabox;
pub mod billing;
pub mod chat;
pub mod chat_bots;
pub mod feed;
pub mod history;
pub mod invites;
pub mod markers;
pub mod me;
pub mod members;
pub mod moblin;
pub mod my_workspaces;
pub mod notices;
pub mod organization_audit;
pub mod organization_ops;
pub mod organizations;
pub mod plans;
pub mod stream_servers;
pub mod support;
pub mod switchers;
pub mod workspaces;

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

pub type ApiResult<T> = std::result::Result<T, ApiError>;

/// An error returned by an API handler.
///
/// It is rendered as JSON (`{"error": {"code": "...", "message": "..."}}`) so
/// clients can branch on the stable `code` while showing the `message`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    /// Seconds until a retry may work, sent as `Retry-After`.
    retry_after: Option<u64>,
}

/// An error response's `code`, in its extensions for `request_spans`.
#[derive(Debug, Clone, Copy)]
pub struct ErrorCode(pub &'static str);

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retry_after: None,
        }
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    pub fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn bad_gateway(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, "upstream_error", message)
    }

    /// 429 `rate_limited`: too many requests; another one is allowed after `wait`.
    pub fn rate_limited(wait: std::time::Duration) -> Self {
        let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
        Self {
            retry_after: Some(seconds),
            ..Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                format!("Too many requests; try again in {seconds} s"),
            )
        }
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Internal server error",
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// The JSON body of every error response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDetail {
    /// Stable, machine-readable error code, e.g. `slug_taken` or `plan_limit`.
    pub code: String,
    /// Human-readable description of the error.
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code.to_owned(),
                message: self.message,
            },
        };

        let mut response = (self.status, Json(body)).into_response();
        response.extensions_mut().insert(ErrorCode(self.code));
        if let Some(seconds) = self.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}

impl From<db::Error> for ApiError {
    fn from(error: db::Error) -> Self {
        if error.is_unique_violation() {
            return Self::conflict("already_exists", "This already exists");
        }

        tracing::error!(%error, "Database operation failed");
        Self::internal()
    }
}

#[cfg(test)]
mod tests {
    use super::ApiError;
    use axum::{http::StatusCode, response::IntoResponse};
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn errors_render_as_json_with_a_code() {
        let response = ApiError::conflict("slug_taken", "That URL is taken").into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "slug_taken");
        assert_eq!(json["error"]["message"], "That URL is taken");
    }
}
