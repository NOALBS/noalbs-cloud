//! YouTube chat. YouTube has no push events for chat or for going live, so one
//! instance at a time ([`watcher`]) looks for the live chat of each linked
//! channel while one of its switchers streams (`liveBroadcasts.list`, with the
//! channel's grant) and follows it over the gRPC `StreamList` stream. Bots post
//! with `liveChatMessages.insert` and their own grant; which chat a channel has
//! open is in the `chat-youtube` bucket, so every instance can answer.
//!
//! Quota: an app gets 10,000 units a day by default. A broadcast lookup costs 1,
//! a message 50, so messages are what runs out; the last quota error is kept
//! for Admin → Status ([`record_error`]).

pub mod proto;
pub mod watcher;

use std::time::Duration;

use chrono::Utc;
use http::uri::PathAndQuery;
use serde::Deserialize;
use serde_json::json;
use tonic::{
    metadata::MetadataValue,
    transport::{Channel, ClientTlsConfig, Endpoint},
};

use crate::{
    app::Shared,
    config::YouTubeConfig,
    error::{Error, Result, check},
};

pub struct YouTube {
    pub config: YouTubeConfig,
    http: reqwest::Client,
    grpc: Channel,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    /// Google keeps the refresh token: this is only set on the first grant.
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct Items<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
}

impl YouTube {
    pub fn new(config: YouTubeConfig, http: reqwest::Client) -> Result<Self> {
        let mut endpoint = Endpoint::from_shared(config.grpc_url.clone())
            .map_err(|error| Error::Other(format!("youtube.grpc_url: {error}")))?
            .http2_keep_alive_interval(Duration::from_secs(30))
            .keep_alive_while_idle(true)
            .connect_timeout(Duration::from_secs(10));
        if config.grpc_url.starts_with("https://") {
            endpoint = endpoint
                .tls_config(ClientTlsConfig::new().with_webpki_roots())
                .map_err(|error| Error::Other(format!("YouTube gRPC TLS: {error}")))?;
        }
        Ok(Self {
            grpc: endpoint.connect_lazy(),
            config,
            http,
        })
    }

    /// Swaps a refresh token for a new access token.
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse> {
        let response = self
            .http
            .post(format!("{}/token", self.config.auth_base))
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await?;
        Ok(check("YouTube", response).await?.json().await?)
    }

    /// The live chat of the channel's broadcast on air, if any (1 quota unit).
    pub async fn active_chat(&self, channel_token: &str) -> Result<Option<String>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Broadcast {
            snippet: Snippet,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Snippet {
            live_chat_id: Option<String>,
        }
        let response = self
            .http
            .get(format!(
                "{}/youtube/v3/liveBroadcasts",
                self.config.api_base
            ))
            .bearer_auth(channel_token)
            .query(&[
                ("part", "snippet"),
                ("broadcastStatus", "active"),
                ("broadcastType", "all"),
            ])
            .send()
            .await?;
        let broadcasts: Items<Broadcast> = check("YouTube", response).await?.json().await?;
        Ok(broadcasts
            .items
            .into_iter()
            .find_map(|broadcast| broadcast.snippet.live_chat_id))
    }

    /// Follows a live chat from `page_token` (or its recent messages).
    pub async fn stream(
        &self,
        token: &str,
        live_chat_id: &str,
        page_token: Option<String>,
    ) -> std::result::Result<tonic::Streaming<proto::LiveChatMessageListResponse>, tonic::Status>
    {
        let mut grpc = tonic::client::Grpc::new(self.grpc.clone());
        grpc.ready()
            .await
            .map_err(|error| tonic::Status::unavailable(error.to_string()))?;
        let mut request = tonic::Request::new(proto::LiveChatMessageListRequest {
            live_chat_id: Some(live_chat_id.to_owned()),
            page_token,
            part: vec!["id".into(), "snippet".into(), "authorDetails".into()],
        });
        let bearer = MetadataValue::try_from(format!("Bearer {token}"))
            .map_err(|_| tonic::Status::unauthenticated("the token isn't a header value"))?;
        request.metadata_mut().insert("authorization", bearer);
        let response = grpc
            .server_streaming(
                request,
                PathAndQuery::from_static(proto::STREAM_LIST),
                tonic_prost::ProstCodec::default(),
            )
            .await?;
        Ok(response.into_inner())
    }

    /// Posts in a live chat as the bot whose token this is (50 quota units).
    pub async fn send_chat(&self, bot_token: &str, live_chat_id: &str, text: &str) -> Result<()> {
        let body = json!({
            "snippet": {
                "liveChatId": live_chat_id,
                "type": "textMessageEvent",
                "textMessageDetails": { "messageText": text },
            }
        });
        let response = self
            .http
            .post(format!(
                "{}/youtube/v3/liveChat/messages",
                self.config.api_base
            ))
            .bearer_auth(bot_token)
            .query(&[("part", "snippet")])
            .json(&body)
            .send()
            .await?;
        check("YouTube", response).await?;
        Ok(())
    }

    /// A channel by its handle (with or without `@`): `(channel ID, handle)`.
    /// Needs the API key.
    pub async fn channel_by_handle(&self, handle: &str) -> Result<Option<(String, String)>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Channel {
            id: String,
            snippet: ChannelSnippet,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ChannelSnippet {
            custom_url: Option<String>,
            title: String,
        }
        let key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| Error::Other("youtube.api_key isn't set".into()))?;
        let handle = format!("@{}", handle.trim_start_matches('@'));
        let response = self
            .http
            .get(format!("{}/youtube/v3/channels", self.config.api_base))
            .query(&[
                ("part", "snippet"),
                ("forHandle", handle.as_str()),
                ("key", key),
            ])
            .send()
            .await?;
        let channels: Items<Channel> = check("YouTube", response).await?.json().await?;
        Ok(channels.items.into_iter().next().map(|channel| {
            let name = channel
                .snippet
                .custom_url
                .map(|url| url.trim_start_matches('@').to_owned())
                .unwrap_or(channel.snippet.title);
            (channel.id, name)
        }))
    }
}

/// Where a channel's open live chat is kept, in [`crate::kv::YOUTUBE`].
pub fn chat_key(state: &Shared, channel_id: &str) -> String {
    state.key(&["chat", channel_id])
}

/// The live chat a channel has open, as the watcher last said.
pub async fn live_chat_of(state: &Shared, channel_id: &str) -> Result<Option<String>> {
    let entry = state
        .buckets
        .youtube
        .get(chat_key(state, channel_id))
        .await
        .map_err(Error::nats)?;
    Ok(entry.and_then(|value| String::from_utf8(value.to_vec()).ok()))
}

/// Whether YouTube refused for the app's quota (not one user's rate limit).
pub fn is_quota(error: &Error) -> bool {
    matches!(error, Error::Platform { status, body, .. }
        if *status == reqwest::StatusCode::FORBIDDEN && body.contains("quotaExceeded"))
}

/// A YouTube error for Admin → Status: Google's message and reason rather than
/// the whole JSON body it answered with.
pub fn describe(error: &Error) -> String {
    #[derive(Deserialize)]
    struct Body {
        error: Details,
    }
    #[derive(Deserialize)]
    struct Details {
        message: String,
        #[serde(default)]
        errors: Vec<Reason>,
    }
    #[derive(Deserialize)]
    struct Reason {
        reason: Option<String>,
    }
    if let Error::Platform { status, body, .. } = error
        && let Ok(Body { error }) = serde_json::from_str(body)
    {
        let code = status.as_u16();
        return match error.errors.into_iter().find_map(|e| e.reason) {
            Some(reason) => format!("{} ({code} {reason})", error.message),
            None => format!("{} ({code})", error.message),
        };
    }
    error.to_string()
}

/// Without HTML tags (Google links words in its messages) and on one line.
fn plain_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Keeps a YouTube error for Admin → Status ([`live::CHAT_YOUTUBE_STATUS_BUCKET`]).
pub async fn record_error(state: &Shared, quota: bool, message: String) {
    let error = live::YouTubeLastError {
        at: Utc::now(),
        quota,
        message: plain_text(&message),
    };
    let Ok(value) = serde_json::to_vec(&error) else {
        return;
    };
    if let Err(error) = state
        .buckets
        .youtube_status
        .put(state.key(&["error"]), value.into())
        .await
    {
        tracing::debug!(%error, "Couldn't keep a YouTube error");
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::{describe, is_quota, plain_text};
    use crate::error::Error;

    /// Google's real quota answer, escapes kept like it sends them.
    const QUOTA_BODY: &str = r#"{
  "error": {
    "code": 403,
    "message": "The request cannot be completed because you have exceeded your \u003ca href=\"/youtube/data/getting-started#quota\"\u003equota\u003c/a\u003e.",
    "errors": [
      {
        "message": "The request cannot be completed because you have exceeded your \u003ca href=\"/youtube/data/getting-started#quota\"\u003equota\u003c/a\u003e.",
        "domain": "youtube.quota",
        "reason": "quotaExceeded"
      }
    ]
  }
}
"#;

    #[test]
    fn quota_errors_read_like_a_sentence() {
        let error = Error::Platform {
            platform: "YouTube",
            status: StatusCode::FORBIDDEN,
            body: QUOTA_BODY.into(),
        };
        assert!(is_quota(&error));
        assert_eq!(
            plain_text(&describe(&error)),
            "The request cannot be completed because you have exceeded your quota. (403 quotaExceeded)"
        );
    }

    #[test]
    fn non_json_bodies_fall_back_to_the_full_error() {
        let error = Error::Platform {
            platform: "YouTube",
            status: StatusCode::BAD_GATEWAY,
            body: "upstream\n  connect error".into(),
        };
        assert_eq!(
            plain_text(&describe(&error)),
            "YouTube answered 502 Bad Gateway: upstream connect error"
        );
    }

    #[test]
    fn plain_text_drops_tags() {
        assert_eq!(
            plain_text("exceeded your <a href=\"/youtube/v3/getting-started#quota\">quota</a>."),
            "exceeded your quota."
        );
    }

    #[tokio::test]
    async fn the_error_keeps_the_whole_answer() {
        let body = QUOTA_BODY.replace(
            "\n    ]\n",
            "\n    ],\n    \"status\": \"PERMISSION_DENIED\"\n",
        );
        assert!(body.len() > 500);
        let response =
            reqwest::Response::from(http::Response::builder().status(403).body(body).unwrap());
        let error = crate::error::check("YouTube", response).await.unwrap_err();
        assert!(is_quota(&error));
        assert_eq!(
            plain_text(&describe(&error)),
            "The request cannot be completed because you have exceeded your quota. (403 quotaExceeded)"
        );
    }
}
