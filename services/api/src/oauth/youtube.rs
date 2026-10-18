//! Google's OAuth for YouTube. The account is a YouTube channel (its `UC…` ID),
//! which is also who chat messages come from, so a member's linked channel counts
//! as theirs in chat. A Google account can hold several channels (brand
//! accounts); Google's own prompt picks one.

use crate::{
    config::OAuthConfig,
    oauth::{AuthorizeRequest, Profile, TokenResponse},
    routes::ApiError,
};

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API_BASE: &str = "https://www.googleapis.com";

const READONLY: &str = "https://www.googleapis.com/auth/youtube.readonly";
const FORCE_SSL: &str = "https://www.googleapis.com/auth/youtube.force-ssl";

/// Sign-in: which channel the user is.
pub const LOGIN_SCOPES: &str = READONLY;
/// A streamer lets the chat service find their live broadcast and read its chat.
pub const CHANNEL_SCOPES: &[&str] = &[READONLY];
/// A bot account lets the chat service post as it.
pub const BOT_SCOPES: &[&str] = &[FORCE_SSL];

/// With an `auth_base` (tests against a fake Google), both the authorization and
/// token endpoints live under it.
fn authorize_url_base(provider: &OAuthConfig) -> String {
    match &provider.auth_base {
        Some(base) => format!("{base}/o/oauth2/v2/auth"),
        None => AUTHORIZE_URL.to_owned(),
    }
}

fn token_url(provider: &OAuthConfig) -> String {
    match &provider.auth_base {
        Some(base) => format!("{base}/token"),
        None => TOKEN_URL.to_owned(),
    }
}

fn api_base(provider: &OAuthConfig) -> &str {
    provider.api_base.as_deref().unwrap_or(API_BASE)
}

/// Always asks for offline access (a refresh token, which the chat service needs);
/// Google only hands one out with the consent screen, which `force_verify` shows.
pub fn authorize_url(provider: &OAuthConfig, request: &AuthorizeRequest<'_>) -> String {
    let mut url =
        reqwest::Url::parse(&authorize_url_base(provider)).expect("valid Google authorization URL");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &provider.client_id)
        .append_pair("redirect_uri", request.redirect_uri)
        .append_pair("scope", request.scopes)
        .append_pair("state", request.state)
        .append_pair("code_challenge", request.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair(
            "prompt",
            if request.force_verify {
                "consent select_account"
            } else {
                "select_account"
            },
        );
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
        &token_url(provider),
        &provider.client_id,
        &provider.client_secret,
        code,
        redirect_uri,
        code_verifier,
    )
    .await
}

#[derive(serde::Deserialize)]
struct Channels {
    #[serde(default)]
    items: Vec<Channel>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Channel {
    id: String,
    snippet: Snippet,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snippet {
    title: String,
    /// The handle, `@name`.
    custom_url: Option<String>,
    thumbnails: Option<Thumbnails>,
}

#[derive(serde::Deserialize)]
struct Thumbnails {
    default: Option<Thumbnail>,
}

#[derive(serde::Deserialize)]
struct Thumbnail {
    url: String,
}

impl Channel {
    /// The handle without its `@`, or the title for the few channels without one.
    fn login(&self) -> String {
        self.snippet
            .custom_url
            .as_deref()
            .map(|handle| handle.trim_start_matches('@').to_owned())
            .unwrap_or_else(|| self.snippet.title.clone())
    }
}

pub async fn fetch_profile(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    access_token: &str,
) -> Result<Profile, ApiError> {
    let response = http
        .get(format!("{}/youtube/v3/channels", api_base(provider)))
        .bearer_auth(access_token)
        .query(&[("part", "snippet"), ("mine", "true")])
        .send()
        .await
        .map_err(upstream_error)?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "YouTube channel request was rejected");
        return Err(ApiError::bad_gateway("YouTube channel lookup failed"));
    }
    let channels: Channels = response.json().await.map_err(upstream_error)?;
    let channel = channels.items.into_iter().next().ok_or_else(|| {
        ApiError::bad_request(
            "no_youtube_channel",
            "That Google account has no YouTube channel",
        )
    })?;
    Ok(Profile {
        login: channel.login(),
        display_name: channel.snippet.title.clone(),
        avatar_url: channel
            .snippet
            .thumbnails
            .as_ref()
            .and_then(|thumbnails| thumbnails.default.as_ref())
            .map(|thumbnail| thumbnail.url.clone()),
        id: channel.id,
    })
}

/// A channel by its handle (with or without `@`), with the API key: `(channel
/// ID, handle)`. Google has no app tokens, so without a key names can't be looked up.
pub async fn find_user(
    http: &reqwest::Client,
    provider: &OAuthConfig,
    name: &str,
) -> Result<Option<(String, String)>, ApiError> {
    let key = provider.api_key.as_deref().ok_or_else(|| {
        ApiError::new(
            axum::http::StatusCode::NOT_IMPLEMENTED,
            "provider_not_configured",
            "Looking YouTube channels up needs an API key",
        )
    })?;
    let handle = format!("@{}", name.trim_start_matches('@'));
    let response = http
        .get(format!("{}/youtube/v3/channels", api_base(provider)))
        .query(&[
            ("part", "snippet"),
            ("forHandle", handle.as_str()),
            ("key", key),
        ])
        .send()
        .await
        .map_err(upstream_error)?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "YouTube channel lookup was rejected");
        return Err(ApiError::bad_gateway("YouTube is unavailable"));
    }
    let channels: Channels = response.json().await.map_err(upstream_error)?;
    Ok(channels
        .items
        .into_iter()
        .next()
        .map(|channel| (channel.id.clone(), channel.login())))
}

fn upstream_error(error: reqwest::Error) -> ApiError {
    tracing::warn!(%error, "YouTube OAuth request failed");
    ApiError::bad_gateway("YouTube is unavailable")
}
