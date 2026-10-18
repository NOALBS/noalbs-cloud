//! Kick chat: webhook deliveries of `chat.message.sent` (to whichever instance
//! the load balancer picks, see [`webhook`]), event subscriptions per channel,
//! and posting as a bot account with its user token. Kick has no webhook event
//! for raids, so those come from Kick's own site WebSocket instead ([`raids`]),
//! until it does.

pub mod pusher;
pub mod raids;
pub mod webhook;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{Mutex, RwLock};

use crate::{
    config::KickConfig,
    error::{Error, Result, check},
};

pub const CHAT_MESSAGE: &str = "chat.message.sent";

pub struct Kick {
    pub config: KickConfig,
    http: reqwest::Client,
    app_token: Mutex<Option<(String, DateTime<Utc>)>>,
    /// Kick's webhook key (DER), fetched once.
    public_key: RwLock<Option<Vec<u8>>>,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
}

const SITE_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";

/// A channel as Kick's site API has it.
#[derive(Debug, Clone, Deserialize)]
pub struct SiteChannel {
    /// The channel's own ID (not its broadcaster's user ID).
    pub id: u64,
    pub user_id: u64,
    pub chatroom: SiteChatroom,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SiteChatroom {
    pub id: u64,
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub event: String,
}

/// A channel or user ID, as Kick's API wants it (a number).
pub fn numeric_id(id: &str) -> Result<i64> {
    id.parse()
        .map_err(|_| Error::Other(format!("not a Kick user ID: {id}")))
}

impl Kick {
    pub fn new(config: KickConfig, http: reqwest::Client) -> Self {
        Self {
            config,
            http,
            app_token: Mutex::new(None),
            public_key: RwLock::new(None),
        }
    }

    async fn app_token(&self) -> Result<String> {
        let mut token = self.app_token.lock().await;
        if let Some((value, expires_at)) = token.as_ref()
            && *expires_at - Utc::now() > chrono::Duration::minutes(5)
        {
            return Ok(value.clone());
        }
        let response = self
            .http
            .post(format!("{}/oauth/token", self.config.auth_base))
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await?;
        let fresh: TokenResponse = check("Kick", response).await?.json().await?;
        let expires_at = Utc::now() + chrono::Duration::seconds(fresh.expires_in.unwrap_or(3600));
        *token = Some((fresh.access_token.clone(), expires_at));
        Ok(fresh.access_token)
    }

    /// Swaps a refresh token for new tokens. Kick replaces the refresh token too.
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse> {
        let response = self
            .http
            .post(format!("{}/oauth/token", self.config.auth_base))
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await?;
        Ok(check("Kick", response).await?.json().await?)
    }

    /// Kick's webhook signing key (DER): the configured one, or Kick's own.
    pub async fn public_key(&self) -> Result<Vec<u8>> {
        if let Some(key) = self.public_key.read().await.as_ref() {
            return Ok(key.clone());
        }
        let pem = match &self.config.public_key {
            Some(pem) => pem.clone(),
            None => {
                #[derive(Deserialize)]
                struct Key {
                    public_key: String,
                }
                let response = self
                    .http
                    .get(format!("{}/public/v1/public-key", self.config.api_base))
                    .send()
                    .await?;
                check("Kick", response)
                    .await?
                    .json::<Data<Key>>()
                    .await?
                    .data
                    .public_key
            }
        };
        let der = webhook::pem_to_der(&pem)
            .ok_or_else(|| Error::Other("Kick's public key isn't PEM".into()))?;
        *self.public_key.write().await = Some(der.clone());
        Ok(der)
    }

    /// Subscribes to a channel's chat, with the app token (or the channel's own
    /// token when given). Returns the subscription ID.
    pub async fn subscribe(&self, channel_id: &str, user_token: Option<&str>) -> Result<String> {
        let token = match user_token {
            Some(token) => token.to_owned(),
            None => self.app_token().await?,
        };
        let body = json!({
            "broadcaster_user_id": numeric_id(channel_id)?,
            "events": [{ "name": CHAT_MESSAGE, "version": 1 }],
            "method": "webhook",
        });
        let response = self
            .http
            .post(format!(
                "{}/public/v1/events/subscriptions",
                self.config.api_base
            ))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        #[derive(Deserialize)]
        struct Created {
            subscription_id: Option<String>,
            error: Option<String>,
        }
        let created: Data<Vec<Created>> = check("Kick", response).await?.json().await?;
        let created = created
            .data
            .into_iter()
            .next()
            .ok_or_else(|| Error::Other("Kick returned no subscription".into()))?;
        match (created.subscription_id, created.error) {
            (Some(id), None) => Ok(id),
            (_, error) => Err(Error::Other(format!(
                "Kick refused the subscription: {}",
                error.unwrap_or_default()
            ))),
        }
    }

    pub async fn unsubscribe(&self, id: &str) -> Result<()> {
        let response = self
            .http
            .delete(format!(
                "{}/public/v1/events/subscriptions",
                self.config.api_base
            ))
            .bearer_auth(self.app_token().await?)
            .query(&[("id", id)])
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        check("Kick", response).await?;
        Ok(())
    }

    /// Every event subscription of the app.
    pub async fn subscriptions(&self) -> Result<Vec<Subscription>> {
        let response = self
            .http
            .get(format!(
                "{}/public/v1/events/subscriptions",
                self.config.api_base
            ))
            .bearer_auth(self.app_token().await?)
            .send()
            .await?;
        Ok(check("Kick", response)
            .await?
            .json::<Data<Vec<Subscription>>>()
            .await?
            .data)
    }

    /// A user by their channel slug (what chat shows, lowercased): `(id, slug)`.
    pub async fn user_by_slug(&self, name: &str) -> Result<Option<(String, String)>> {
        #[derive(Deserialize)]
        struct Channel {
            broadcaster_user_id: i64,
            slug: String,
        }
        // Slugs are lowercase with dashes where usernames have underscores.
        let lower = name.to_lowercase();
        for slug in [lower.clone(), lower.replace('_', "-")] {
            let response = self
                .http
                .get(format!("{}/public/v1/channels", self.config.api_base))
                .bearer_auth(self.app_token().await?)
                .query(&[("slug", slug.as_str())])
                .send()
                .await?;
            let channels: Data<Vec<Channel>> = check("Kick", response).await?.json().await?;
            if let Some(channel) = channels.data.into_iter().next() {
                return Ok(Some((
                    channel.broadcaster_user_id.to_string(),
                    channel.slug,
                )));
            }
        }
        Ok(None)
    }

    /// A channel's slug by its broadcaster's user ID (what we store).
    pub async fn slug_of(&self, broadcaster_user_id: &str) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct Channel {
            slug: String,
        }
        let response = self
            .http
            .get(format!("{}/public/v1/channels", self.config.api_base))
            .bearer_auth(self.app_token().await?)
            .query(&[("broadcaster_user_id", broadcaster_user_id)])
            .send()
            .await?;
        let channels: Data<Vec<Channel>> = check("Kick", response).await?.json().await?;
        Ok(channels.data.into_iter().next().map(|channel| channel.slug))
    }

    /// The channel and chatroom IDs Kick's site WebSocket uses, which its public
    /// API doesn't give: from the site's own (unofficial) API.
    pub async fn site_channel(&self, slug: &str) -> Result<SiteChannel> {
        let response = self
            .http
            .get(format!("{}/api/v2/channels/{slug}", self.config.web_base))
            .header(reqwest::header::ACCEPT, "application/json")
            // Kick's site refuses clients that don't look like a browser.
            .header(reqwest::header::USER_AGENT, SITE_USER_AGENT)
            .send()
            .await?;
        Ok(check("Kick", response).await?.json().await?)
    }

    /// Posts in `channel_id`'s chat as the bot whose token this is.
    pub async fn send_chat(
        &self,
        bot_token: &str,
        channel_id: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> Result<()> {
        let mut body = json!({
            "broadcaster_user_id": numeric_id(channel_id)?,
            "content": text,
            "type": "user",
        });
        if let Some(reply_to) = reply_to {
            body["reply_to_message_id"] = json!(reply_to);
        }
        let response = self
            .http
            .post(format!("{}/public/v1/chat", self.config.api_base))
            .bearer_auth(bot_token)
            .json(&body)
            .send()
            .await?;
        check("Kick", response).await?;
        Ok(())
    }
}
