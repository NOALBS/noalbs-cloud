use crate::{
    config::OAuthConfig,
    oauth::{AuthorizeRequest, PlatformError, Profile, TokenResponse},
    routes::ApiError,
};

const AUTH_BASE: &str = "https://id.twitch.tv";
const API_BASE: &str = "https://api.twitch.tv";
/// Sign-in: who the user is. Their public profile needs no scope, and we don't
/// ask for their email (we don't use it).
pub const LOGIN_SCOPES: &str = "";
/// A streamer lets our app's bots into their channel (and gives them the Chat Bot badge).
pub const CHANNEL_SCOPES: &[&str] = &["channel:bot"];
/// A bot account lets our app read chat and speak as it with an app token.
pub const BOT_SCOPES: &[&str] = &["user:read:chat", "user:write:chat", "user:bot"];

fn auth_base(provider: &OAuthConfig) -> &str {
    provider.auth_base.as_deref().unwrap_or(AUTH_BASE)
}

fn api_base(provider: &OAuthConfig) -> &str {
    provider.api_base.as_deref().unwrap_or(API_BASE)
}

pub fn authorize_url(provider: &OAuthConfig, request: &AuthorizeRequest<'_>) -> String {
    let mut url = reqwest::Url::parse(&format!("{}/oauth2/authorize", auth_base(provider)))
        .expect("valid Twitch authorization URL");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &provider.client_id)
        .append_pair("redirect_uri", request.redirect_uri)
        .append_pair("scope", request.scopes)
        .append_pair("state", request.state)
        .append_pair("code_challenge", request.challenge)
        .append_pair("code_challenge_method", "S256");
    if request.force_verify {
        // Ask even when the browser is signed in to Twitch, so they can pick the
        // right account (the bot, not their own).
        url.query_pairs_mut().append_pair("force_verify", "true");
    }
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
        &format!("{}/oauth2/token", auth_base(provider)),
        &provider.client_id,
        &provider.client_secret,
        code,
        redirect_uri,
        code_verifier,
    )
    .await
}

#[derive(serde::Deserialize)]
struct TwitchUsersResponse {
    data: Vec<TwitchUser>,
}

#[derive(serde::Deserialize)]
struct TwitchUser {
    id: String,
    login: String,
    display_name: String,
    profile_image_url: Option<String>,
}

pub async fn fetch_profile(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    access_token: &str,
) -> Result<Profile, ApiError> {
    let response = http
        .get(format!("{}/helix/users", api_base(provider)))
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Client-Id", &provider.client_id)
        .send()
        .await
        .map_err(upstream_error)?;

    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "Twitch profile request was rejected");
        return Err(ApiError::bad_gateway("Twitch profile lookup failed"));
    }

    let mut users: TwitchUsersResponse = response.json().await.map_err(upstream_error)?;
    let user = users
        .data
        .pop()
        .ok_or_else(|| ApiError::bad_gateway("Twitch returned no user profile"))?;

    Ok(Profile {
        id: user.id,
        login: user.login,
        display_name: user.display_name,
        avatar_url: user.profile_image_url,
    })
}

fn upstream_error(error: reqwest::Error) -> ApiError {
    tracing::warn!(%error, "Twitch OAuth request failed");
    ApiError::bad_gateway("Twitch is unavailable")
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
        .post(format!("{}/oauth2/token", auth_base(provider)))
        .form(&[
            ("client_id", provider.client_id.as_str()),
            ("client_secret", provider.client_secret.as_str()),
            ("grant_type", "client_credentials"),
        ])
        .send()
        .await
        .map_err(upstream_error)?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "Twitch refused an app token");
        return Err(ApiError::bad_gateway("Twitch is unavailable"));
    }
    let token: Token = response.json().await.map_err(upstream_error)?;
    Ok((token.access_token, token.expires_in))
}

/// A Twitch user by login: `(id, login)`, if there is one.
pub async fn find_user(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    token: &str,
    login: &str,
) -> Result<Option<(String, String)>, ApiError> {
    let response = http
        .get(format!("{}/helix/users", api_base(provider)))
        .bearer_auth(token)
        .header("Client-Id", &provider.client_id)
        .query(&[("login", login.to_lowercase())])
        .send()
        .await
        .map_err(upstream_error)?;
    if response.status() == reqwest::StatusCode::BAD_REQUEST {
        // Not a valid login at all.
        return Ok(None);
    }
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "Twitch user lookup was rejected");
        return Err(ApiError::bad_gateway("Twitch is unavailable"));
    }
    let users: TwitchUsersResponse = response.json().await.map_err(upstream_error)?;
    Ok(users
        .data
        .into_iter()
        .next()
        .map(|user| (user.id, user.login)))
}

/// A conduit shard as Twitch sees it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConduitShard {
    pub id: String,
    /// `enabled`, or why not (e.g. `websocket_disconnected`).
    pub status: String,
}

#[derive(serde::Deserialize)]
struct Page<T> {
    data: Vec<T>,
    #[serde(default)]
    pagination: Pagination,
    #[serde(default)]
    total_cost: i64,
    #[serde(default)]
    max_total_cost: i64,
}

#[derive(Default, serde::Deserialize)]
struct Pagination {
    cursor: Option<String>,
}

/// Pages through a Helix list (at most `max_pages` of 100).
async fn pages<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    token: &str,
    path: &str,
    query: &[(&str, &str)],
    max_pages: usize,
) -> Result<(Vec<T>, i64, i64), PlatformError> {
    let mut all = Vec::new();
    let mut costs = (0, 0);
    let mut cursor: Option<String> = None;
    for _ in 0..max_pages {
        let mut params: Vec<(&str, &str)> = query.to_vec();
        params.push(("first", "100"));
        if let Some(cursor) = &cursor {
            params.push(("after", cursor));
        }
        let response = http
            .get(format!("{}/helix{path}", api_base(provider)))
            .bearer_auth(token)
            .header("Client-Id", &provider.client_id)
            .query(&params)
            .send()
            .await
            .map_err(|error| PlatformError::request("Twitch", error))?;
        if !response.status().is_success() {
            return Err(PlatformError::status("Twitch", response).await);
        }
        let page: Page<T> = response
            .json()
            .await
            .map_err(|error| PlatformError::request("Twitch", error))?;
        all.extend(page.data);
        costs = (page.total_cost, page.max_total_cost);
        match page.pagination.cursor {
            Some(next) if !next.is_empty() => cursor = Some(next),
            _ => break,
        }
    }
    Ok((all, costs.0, costs.1))
}

pub async fn conduit_shards(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    token: &str,
    conduit_id: &str,
) -> Result<Vec<ConduitShard>, PlatformError> {
    let (shards, _, _) = pages(
        http,
        provider,
        token,
        "/eventsub/conduits/shards",
        &[("conduit_id", conduit_id)],
        200,
    )
    .await?;
    Ok(shards)
}

/// An EventSub subscription, as far as the status page cares.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EventSubscription {
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub transport: serde_json::Value,
}

/// The app's EventSub subscriptions (up to 5,000) and their cost against the limit.
pub async fn subscriptions(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    token: &str,
) -> Result<(Vec<EventSubscription>, i64, i64), PlatformError> {
    pages(http, provider, token, "/eventsub/subscriptions", &[], 50).await
}
