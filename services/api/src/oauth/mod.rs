pub mod kick;
pub mod twitch;
pub mod youtube;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

use crate::{
    config::{Config, OAuthConfig},
    routes::ApiError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoginPlatform {
    Twitch,
    Kick,
    YouTube,
}

impl LoginPlatform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Twitch => "twitch",
            Self::Kick => "kick",
            Self::YouTube => "youtube",
        }
    }

    pub const fn db_provider(self) -> db::oauth2_connections::OAuth2Provider {
        match self {
            Self::Twitch => db::oauth2_connections::OAuth2Provider::Twitch,
            Self::Kick => db::oauth2_connections::OAuth2Provider::Kick,
            Self::YouTube => db::oauth2_connections::OAuth2Provider::YouTube,
        }
    }
}

impl std::str::FromStr for LoginPlatform {
    type Err = ApiError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "twitch" => Ok(Self::Twitch),
            "kick" => Ok(Self::Kick),
            "youtube" => Ok(Self::YouTube),
            _ => Err(ApiError::not_found("OAuth provider not found")),
        }
    }
}

#[derive(Clone)]
pub struct OAuthProviders {
    base_url: String,
    twitch: Option<OAuthConfig>,
    kick: Option<OAuthConfig>,
    youtube: Option<OAuthConfig>,
    /// App access tokens (client credentials), reused until shortly before they expire.
    app_tokens: Arc<Mutex<HashMap<LoginPlatform, (String, Instant)>>>,
}

impl OAuthProviders {
    pub fn from_config(config: &Config) -> Self {
        Self {
            base_url: config.base_url.trim_end_matches('/').to_owned(),
            twitch: config.oauth_twitch.clone(),
            kick: config.oauth_kick.clone(),
            youtube: config.oauth_youtube.clone(),
            app_tokens: Arc::default(),
        }
    }

    /// An app access token for the platform, reused while it has minutes left.
    /// Google has none (YouTube lookups use the API key).
    pub async fn app_token(
        &self,
        http: &reqwest::Client,
        platform: LoginPlatform,
    ) -> Result<String, ApiError> {
        if let Some((token, until)) = self.app_tokens.lock().unwrap().get(&platform)
            && *until > Instant::now()
        {
            return Ok(token.clone());
        }
        let provider = self.provider(platform)?;
        let (token, expires_in) = match platform {
            LoginPlatform::Twitch => twitch::app_token(http, provider).await?,
            LoginPlatform::Kick => kick::app_token(http, provider).await?,
            LoginPlatform::YouTube => {
                tracing::error!("Asked for a YouTube app token; Google has none");
                return Err(ApiError::internal());
            }
        };
        let lifetime = Duration::from_secs(expires_in.saturating_sub(300).max(0) as u64);
        self.app_tokens
            .lock()
            .unwrap()
            .insert(platform, (token.clone(), Instant::now() + lifetime));
        Ok(token)
    }

    /// Drops a cached app token the platform refused, so the next call gets a new one.
    pub fn forget_app_token(&self, platform: LoginPlatform) {
        self.app_tokens.lock().unwrap().remove(&platform);
    }

    pub fn provider(&self, platform: LoginPlatform) -> Result<&OAuthConfig, ApiError> {
        let provider = match platform {
            LoginPlatform::Twitch => self.twitch.as_ref(),
            LoginPlatform::Kick => self.kick.as_ref(),
            LoginPlatform::YouTube => self.youtube.as_ref(),
        };

        provider.ok_or_else(|| {
            ApiError::new(
                axum::http::StatusCode::NOT_IMPLEMENTED,
                "provider_not_configured",
                format!("{} OAuth is not configured", platform.as_str()),
            )
        })
    }

    pub fn is_configured(&self, platform: LoginPlatform) -> bool {
        self.provider(platform).is_ok()
    }

    pub fn callback_url(&self, platform: LoginPlatform) -> String {
        format!("{}/auth/{}/callback", self.base_url, platform.as_str())
    }

    pub fn link_callback_url(&self, platform: LoginPlatform) -> String {
        format!("{}/auth/link/{}/callback", self.base_url, platform.as_str())
    }

    /// Where the chat grants (channels and bot accounts) come back to.
    pub fn chat_callback_url(&self, platform: LoginPlatform) -> String {
        format!(
            "{}/chat/connect/{}/callback",
            self.base_url,
            platform.as_str()
        )
    }

    pub fn authorize_url(
        &self,
        platform: LoginPlatform,
        request: &AuthorizeRequest<'_>,
    ) -> Result<String, ApiError> {
        let provider = self.provider(platform)?;
        Ok(match platform {
            LoginPlatform::Twitch => twitch::authorize_url(provider, request),
            LoginPlatform::Kick => kick::authorize_url(provider, request),
            LoginPlatform::YouTube => youtube::authorize_url(provider, request),
        })
    }

    pub async fn exchange_code(
        &self,
        http: &reqwest::Client,
        platform: LoginPlatform,
        code: &str,
        redirect_uri: &str,
        code_verifier: &str,
    ) -> Result<TokenResponse, ApiError> {
        let provider = self.provider(platform)?;
        match platform {
            LoginPlatform::Twitch => {
                twitch::exchange_code(http, provider, code, redirect_uri, code_verifier).await
            }
            LoginPlatform::Kick => {
                kick::exchange_code(http, provider, code, redirect_uri, code_verifier).await
            }
            LoginPlatform::YouTube => {
                youtube::exchange_code(http, provider, code, redirect_uri, code_verifier).await
            }
        }
    }

    /// Finds an account by its name, with an app token (YouTube: the API key):
    /// `(id, login)`.
    pub async fn find_user(
        &self,
        http: &reqwest::Client,
        platform: LoginPlatform,
        name: &str,
    ) -> Result<Option<(String, String)>, ApiError> {
        let provider = self.provider(platform)?;
        match platform {
            LoginPlatform::Twitch => {
                let token = self.app_token(http, platform).await?;
                twitch::find_user(http, provider, &token, name).await
            }
            LoginPlatform::Kick => {
                let token = self.app_token(http, platform).await?;
                kick::find_user(http, provider, &token, name).await
            }
            LoginPlatform::YouTube => youtube::find_user(http, provider, name).await,
        }
    }

    /// Who the token belongs to.
    pub async fn fetch_profile(
        &self,
        http: &reqwest::Client,
        platform: LoginPlatform,
        access_token: &str,
    ) -> Result<Profile, ApiError> {
        let provider = self.provider(platform)?;
        match platform {
            LoginPlatform::Twitch => twitch::fetch_profile(http, provider, access_token).await,
            LoginPlatform::Kick => kick::fetch_profile(http, provider, access_token).await,
            LoginPlatform::YouTube => youtube::fetch_profile(http, provider, access_token).await,
        }
    }
}

/// What an authorization request asks for.
pub struct AuthorizeRequest<'a> {
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub challenge: &'a str,
    /// Space-separated.
    pub scopes: &'a str,
    /// Show the account prompt even when already signed in (Twitch; on YouTube the
    /// consent screen, which also brings a refresh token).
    pub force_verify: bool,
}

/// A platform account, from its profile.
#[derive(Debug, Clone)]
pub struct Profile {
    pub id: String,
    /// The name in URLs (lowercase on Twitch).
    pub login: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

pub struct PkceChallenge {
    pub verifier: String,
    pub challenge: String,
}

pub fn generate_pkce() -> PkceChallenge {
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    PkceChallenge {
        verifier,
        challenge,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OAuthFlow {
    Login,
    Link,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OAuthStateCookie {
    pub state: String,
    pub code_verifier: String,
    pub flow: OAuthFlow,
    pub redirect_target: Option<String>,
}

pub const OAUTH_STATE_COOKIE_NAME: &str = "oauth_state";

/// First path segments served by something other than the dashboard app, or
/// that would loop: never sent back to (`/api/auth/twitch` would start another
/// sign-in). Mirrors the dashboard's `lib/redirect.ts`.
const NOT_THE_APP: &[&str] = &[
    "api", "link", "source", "chat", "assets", "privacy", "login",
];

/// A dashboard path on this site to return to after an OAuth flow, or `None`.
pub fn sanitize_redirect_target(target: &str) -> Option<String> {
    if !target.starts_with('/') || target.starts_with("//") || target.starts_with("/\\") {
        return None;
    }
    let first = target[1..]
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    (!NOT_THE_APP.contains(&first.as_str()) && !first.ends_with(".html")).then(|| target.to_owned())
}

#[derive(Debug, serde::Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: Option<String>,
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub scope: serde_json::Value,
}

impl TokenResponse {
    /// Whether every one of `required` was granted.
    pub fn has_scopes(&self, required: &[&str]) -> bool {
        let granted = self.scopes();
        required
            .iter()
            .all(|scope| granted.iter().any(|granted| granted == scope))
    }

    pub fn scopes(&self) -> Vec<String> {
        match &self.scope {
            serde_json::Value::Array(values) => values
                .iter()
                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                .collect(),
            serde_json::Value::String(value) => {
                value.split_whitespace().map(ToOwned::to_owned).collect()
            }
            _ => Vec::new(),
        }
    }
}

pub async fn exchange_code(
    http: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse, ApiError> {
    let response = http
        .post(token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|error| {
            tracing::warn!(%error, "OAuth token exchange request failed");
            ApiError::bad_gateway("OAuth provider is unavailable")
        })?;

    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "OAuth token exchange was rejected");
        return Err(ApiError::bad_gateway("OAuth token exchange failed"));
    }

    response.json().await.map_err(|error| {
        tracing::warn!(%error, "OAuth token response could not be parsed");
        ApiError::bad_gateway("OAuth provider returned an invalid token response")
    })
}

/// A platform call for the admin status page that failed; shown, not fatal.
#[derive(Debug)]
pub struct PlatformError {
    /// The platform refused the token (drop the cached one).
    pub unauthorized: bool,
    pub message: String,
}

impl PlatformError {
    pub(crate) fn request(platform: &str, error: reqwest::Error) -> Self {
        Self {
            unauthorized: false,
            message: format!("{platform} didn't answer: {error}"),
        }
    }

    pub(crate) async fn status(platform: &str, response: reqwest::Response) -> Self {
        let status = response.status();
        let mut body = response.text().await.unwrap_or_default();
        body.truncate(200);
        Self {
            unauthorized: status == reqwest::StatusCode::UNAUTHORIZED,
            message: format!("{platform} answered {status}: {body}"),
        }
    }
}
