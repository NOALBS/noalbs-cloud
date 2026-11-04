use std::time::Duration;

use reqwest::{StatusCode, header};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    config::{HostPlatform, Pairing},
    url_security::{validate_api_url, validate_dashboard_url, validate_verification_url},
};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectOs {
    Windows,
    Linux,
    Macos,
}

impl ConnectOs {
    pub const fn for_platform(platform: HostPlatform) -> Self {
        match platform {
            HostPlatform::Windows => Self::Windows,
            HostPlatform::MacOS => Self::Macos,
            HostPlatform::Linux => Self::Linux,
        }
    }

    pub const fn current() -> Self {
        Self::for_platform(HostPlatform::current())
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "macos",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingRequest<'a> {
    pub name: &'a str,
    pub os: ConnectOs,
    pub app_version: &'a str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PairingCreated {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u32,
    pub interval: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PairingClaimed {
    pub device_id: Uuid,
    pub device_token: String,
    pub switcher_id: Uuid,
    pub link_url: String,
}

#[derive(Debug, Clone)]
pub enum PairingStatus {
    Pending,
    Expired,
    RateLimited(Duration),
}

#[derive(Debug, Clone)]
pub struct PairingResult {
    pub pairing: Pairing,
    pub device_token: String,
}

#[derive(Debug, Error)]
pub enum PairingError {
    #[error("pairing HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("pairing API returned {status}: {message}")]
    Api { status: StatusCode, message: String },
    #[error("pairing response was invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    UnsafeUrl(String),
}

#[derive(Debug, Clone)]
pub struct PairingClient {
    api_url: String,
    dashboard_url: Option<String>,
    client: reqwest::Client,
}

impl PairingClient {
    pub fn new(api_url: impl Into<String>) -> Result<Self, PairingError> {
        Self::new_with_dashboard_url(api_url, None)
    }

    pub fn new_with_dashboard_url(
        api_url: impl Into<String>,
        dashboard_url: Option<String>,
    ) -> Result<Self, PairingError> {
        let api_url = api_url.into().trim_end_matches('/').to_owned();
        validate_api_url(&api_url).map_err(PairingError::UnsafeUrl)?;
        if let Some(dashboard_url) = dashboard_url.as_deref() {
            validate_dashboard_url(dashboard_url).map_err(PairingError::UnsafeUrl)?;
        }
        Ok(Self {
            api_url,
            dashboard_url,
            client: reqwest::Client::new(),
        })
    }

    pub async fn create(
        &self,
        name: &str,
        os: ConnectOs,
        app_version: &str,
    ) -> Result<PairingCreated, PairingError> {
        let response = self
            .client
            .post(format!("{}/connect/pair", self.api_url))
            .json(&PairingRequest {
                name,
                os,
                app_version,
            })
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(api_error(response).await);
        }
        let created: PairingCreated = response.json().await?;
        validate_verification_url(
            &self.api_url,
            self.dashboard_url.as_deref(),
            &created.verification_url,
        )
        .map_err(PairingError::UnsafeUrl)?;
        Ok(created)
    }

    pub async fn poll(&self, device_code: &str) -> Result<PairingResult, PairingError> {
        let response = self
            .client
            .post(format!("{}/connect/pair/poll", self.api_url))
            .json(&serde_json::json!({ "device_code": device_code }))
            .send()
            .await?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(10);
            return Err(PairingError::Api {
                status,
                message: format!("rate limited; retry after {retry}s"),
            });
        }
        if !status.is_success() {
            let body = response.json::<ApiErrorBody>().await.unwrap_or_default();
            let code = body.error.code.as_str();
            if code == "authorization_pending" {
                return Err(PairingError::Api {
                    status,
                    message: code.to_owned(),
                });
            }
            if code == "expired_token" {
                return Err(PairingError::Api {
                    status,
                    message: code.to_owned(),
                });
            }
            return Err(PairingError::Api {
                status,
                message: body.error.message,
            });
        }
        let claimed: PairingClaimed = response.json().await?;
        crate::url_security::validate_link_url(&claimed.link_url)
            .map_err(PairingError::UnsafeUrl)?;
        Ok(PairingResult {
            pairing: Pairing {
                device_id: claimed.device_id,
                switcher_id: claimed.switcher_id,
                link_url: claimed.link_url,
            },
            device_token: claimed.device_token,
        })
    }

    pub async fn poll_until_complete<F>(
        &self,
        device_code: &str,
        interval: Duration,
        mut update: F,
    ) -> Result<PairingResult, PairingError>
    where
        F: FnMut(PairingStatus),
    {
        let mut delay = interval.max(Duration::from_secs(1));
        loop {
            match self.poll(device_code).await {
                Ok(result) => return Ok(result),
                Err(PairingError::Api { message, .. }) if message == "authorization_pending" => {
                    update(PairingStatus::Pending);
                    tokio::time::sleep(delay).await;
                    delay = interval.max(Duration::from_secs(1));
                }
                Err(PairingError::Api { message, .. }) if message == "expired_token" => {
                    update(PairingStatus::Expired);
                    return Err(PairingError::Api {
                        status: StatusCode::BAD_REQUEST,
                        message,
                    });
                }
                Err(PairingError::Api { message, .. }) if message.starts_with("rate limited") => {
                    let seconds = message
                        .split("retry after ")
                        .nth(1)
                        .and_then(|value| value.trim_end_matches('s').parse::<u64>().ok())
                        .unwrap_or(10)
                        .clamp(1, 60);
                    delay = Duration::from_secs(seconds);
                    update(PairingStatus::RateLimited(delay));
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    error: ApiErrorDetail,
}

#[derive(Debug, Default, Deserialize)]
struct ApiErrorDetail {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

async fn api_error(response: reqwest::Response) -> PairingError {
    let status = response.status();
    let body = response.json::<ApiErrorBody>().await.unwrap_or_default();
    PairingError::Api {
        status,
        message: body.error.message,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{Json, Router, extract::State, routing::post};
    use serde_json::{Value, json};

    use super::{ConnectOs, HostPlatform, PairingClient};

    type Calls = Arc<Mutex<Vec<(String, Value)>>>;

    async fn create(State(calls): State<Calls>, Json(body): Json<Value>) -> Json<Value> {
        calls.lock().unwrap().push(("create".into(), body));
        Json(json!({
            "device_code": "private-device-code",
            "user_code": "ABCDEFGH",
            "verification_url": "http://127.0.0.1/connect?code=ABCDEFGH",
            "expires_in": 600,
            "interval": 10
        }))
    }

    async fn poll(State(calls): State<Calls>, Json(body): Json<Value>) -> Json<Value> {
        calls.lock().unwrap().push(("poll".into(), body));
        Json(json!({
            "device_id": "11111111-1111-4111-8111-111111111111",
            "device_token": "device-token",
            "switcher_id": "22222222-2222-4222-8222-222222222222",
            "link_url": "wss://app.noalbs.com/link"
        }))
    }

    #[test]
    fn macos_serializes_to_the_server_os_value() {
        assert_eq!(
            ConnectOs::for_platform(HostPlatform::MacOS).as_str(),
            "macos"
        );
        assert_eq!(
            serde_json::to_value(ConnectOs::Macos).unwrap(),
            json!("macos")
        );
    }

    #[tokio::test]
    async fn create_and_claim_use_the_api_prefix_and_contract() {
        let calls = Calls::default();
        let app = Router::new()
            .route("/api/connect/pair", post(create))
            .route("/api/connect/pair/poll", post(poll))
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = PairingClient::new(url).unwrap();
        let created = client
            .create("Studio PC", ConnectOs::Linux, "0.1.0")
            .await
            .unwrap();
        assert_eq!(created.user_code, "ABCDEFGH");
        let claimed = client.poll(&created.device_code).await.unwrap();
        assert_eq!(claimed.pairing.link_url, "wss://app.noalbs.com/link");
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls[0],
            (
                "create".into(),
                json!({"name":"Studio PC","os":"linux","app_version":"0.1.0"})
            )
        );
        assert_eq!(
            calls[1],
            ("poll".into(), json!({"device_code":"private-device-code"}))
        );
        server.abort();
    }
}
