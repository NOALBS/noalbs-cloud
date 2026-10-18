use crate::{
    config::OAuthConfig,
    oauth::{AuthorizeRequest, PlatformError, Profile, TokenResponse},
    routes::ApiError,
};

const AUTH_BASE: &str = "https://id.kick.com";
const API_BASE: &str = "https://api.kick.com";
/// Sign-in: who the user is.
pub const LOGIN_SCOPES: &str = "user:read";
/// A streamer lets the chat service subscribe to their channel's chat.
pub const CHANNEL_SCOPES: &[&str] = &["user:read", "events:subscribe"];
/// A bot account lets the chat service post as it.
pub const BOT_SCOPES: &[&str] = &["user:read", "chat:write"];

fn auth_base(provider: &OAuthConfig) -> &str {
    provider.auth_base.as_deref().unwrap_or(AUTH_BASE)
}

fn api_base(provider: &OAuthConfig) -> &str {
    provider.api_base.as_deref().unwrap_or(API_BASE)
}

/// Kick has no way to force the account prompt (`force_verify` is ignored).
pub fn authorize_url(provider: &OAuthConfig, request: &AuthorizeRequest<'_>) -> String {
    let mut url = reqwest::Url::parse(&format!("{}/oauth/authorize", auth_base(provider)))
        .expect("valid Kick authorization URL");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &provider.client_id)
        .append_pair("redirect_uri", request.redirect_uri)
        .append_pair("scope", request.scopes)
        .append_pair("state", request.state)
        .append_pair("code_challenge", request.challenge)
        .append_pair("code_challenge_method", "S256");
    url.into()
}

pub async fn exchange_code(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse, ApiError> {
    super::exchange_code(
        http,
        &format!("{}/oauth/token", auth_base(provider)),
        &provider.client_id,
        &provider.client_secret,
        code,
        redirect_uri,
        code_verifier,
    )
    .await
}

#[derive(serde::Deserialize)]
struct KickUser {
    #[serde(alias = "user_id")]
    id: i64,
    #[serde(alias = "name")]
    username: String,
    #[serde(alias = "profile_picture")]
    profile_pic: Option<String>,
}

#[derive(serde::Deserialize)]
struct KickUserResponse {
    data: Vec<KickUser>,
    // message: Option<String>,
}

pub async fn fetch_profile(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    access_token: &str,
) -> Result<Profile, ApiError> {
    let response = http
        .get(format!("{}/public/v1/users", api_base(provider)))
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(upstream_error)?;

    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "Kick profile request was rejected");
        return Err(ApiError::bad_gateway("Kick profile lookup failed"));
    }

    let user: KickUserResponse = response.json().await.map_err(upstream_error)?;
    let user = user
        .data
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::bad_gateway("Kick profile lookup failed"))?;

    Ok(Profile {
        id: user.id.to_string(),
        login: user.username.clone(),
        display_name: user.username,
        avatar_url: user.profile_pic,
    })
}

fn upstream_error(error: reqwest::Error) -> ApiError {
    tracing::warn!(%error, "Kick OAuth request failed");
    ApiError::bad_gateway("Kick is unavailable")
}

/// A fresh app access token (client credentials) and its lifetime in seconds;
/// `OAuthProviders::app_token` caches it.
pub async fn app_token(
    http: &reqwest::Client,
    provider: &OAuthConfig,
) -> Result<(String, i64), ApiError> {
    #[derive(serde::Deserialize)]
    struct Token {
        access_token: String,
        expires_in: i64,
    }
    let response = http
        .post(format!("{}/oauth/token", auth_base(provider)))
        .form(&[
            ("client_id", provider.client_id.as_str()),
            ("client_secret", provider.client_secret.as_str()),
            ("grant_type", "client_credentials"),
        ])
        .send()
        .await
        .map_err(upstream_error)?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "Kick refused an app token");
        return Err(ApiError::bad_gateway("Kick is unavailable"));
    }
    let token: Token = response.json().await.map_err(upstream_error)?;
    Ok((token.access_token, token.expires_in))
}

/// A Kick user by name (their channel slug, lowercase with dashes for
/// underscores): `(id, slug)`, if there is one.
pub async fn find_user(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    token: &str,
    name: &str,
) -> Result<Option<(String, String)>, ApiError> {
    #[derive(serde::Deserialize)]
    struct Channel {
        broadcaster_user_id: i64,
        slug: String,
    }
    #[derive(serde::Deserialize)]
    struct Channels {
        data: Vec<Channel>,
    }
    let lower = name.to_lowercase();
    for slug in [lower.clone(), lower.replace('_', "-")] {
        let response = http
            .get(format!("{}/public/v1/channels", api_base(provider)))
            .bearer_auth(token)
            .query(&[("slug", slug.as_str())])
            .send()
            .await
            .map_err(upstream_error)?;
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "Kick user lookup was rejected");
            return Err(ApiError::bad_gateway("Kick is unavailable"));
        }
        let channels: Channels = response.json().await.map_err(upstream_error)?;
        if let Some(channel) = channels.data.into_iter().next() {
            return Ok(Some((
                channel.broadcaster_user_id.to_string(),
                channel.slug,
            )));
        }
    }
    Ok(None)
}

/// How many event subscriptions the Kick app has.
pub async fn subscription_count(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    token: &str,
) -> Result<usize, PlatformError> {
    #[derive(serde::Deserialize)]
    struct Subscriptions {
        data: Vec<serde_json::Value>,
    }
    let response = http
        .get(format!(
            "{}/public/v1/events/subscriptions",
            api_base(provider)
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| PlatformError::request("Kick", error))?;
    if !response.status().is_success() {
        return Err(PlatformError::status("Kick", response).await);
    }
    let subscriptions: Subscriptions = response
        .json()
        .await
        .map_err(|error| PlatformError::request("Kick", error))?;
    Ok(subscriptions.data.len())
}
