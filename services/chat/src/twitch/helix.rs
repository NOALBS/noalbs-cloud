//! The Twitch API calls the chat service makes, all with an app access token
//! (client credentials). Each instance gets its own token.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use crate::{
    config::TwitchConfig,
    error::{Error, Result, check},
};

pub struct Helix {
    config: TwitchConfig,
    http: reqwest::Client,
    token: Mutex<Option<AppToken>>,
}

struct AppToken {
    value: String,
    expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

#[derive(Deserialize)]
struct Data<T> {
    data: Vec<T>,
    #[serde(default)]
    pagination: Pagination,
}

#[derive(Default, Deserialize)]
struct Pagination {
    cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Conduit {
    pub id: String,
    pub shard_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Subscription {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub condition: serde_json::Map<String, serde_json::Value>,
    pub transport: Transport,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Transport {
    pub conduit_id: Option<String>,
}

/// What subscribing a channel came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subscribed {
    Created(String),
    /// Twitch already has it (409): nothing was created.
    Exists,
}

impl Helix {
    pub fn new(config: TwitchConfig, http: reqwest::Client) -> Self {
        Self {
            config,
            http,
            token: Mutex::new(None),
        }
    }

    async fn app_token(&self) -> Result<String> {
        let mut token = self.token.lock().await;
        if let Some(token) = token.as_ref()
            && token.expires_at - Utc::now() > chrono::Duration::minutes(5)
        {
            return Ok(token.value.clone());
        }
        let response = self
            .http
            .post(format!("{}/oauth2/token", self.config.auth_base))
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await?;
        let fresh: TokenResponse = check("Twitch", response).await?.json().await?;
        *token = Some(AppToken {
            value: fresh.access_token.clone(),
            expires_at: Utc::now() + chrono::Duration::seconds(fresh.expires_in),
        });
        Ok(fresh.access_token)
    }

    /// Sends a Helix request; a 401 drops the cached token and tries once more.
    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::Response> {
        for attempt in 0..2 {
            let token = self.app_token().await?;
            let mut request = self
                .http
                .request(
                    method.clone(),
                    format!("{}/helix{path}", self.config.api_base),
                )
                .bearer_auth(token)
                .header("Client-Id", &self.config.client_id)
                .query(query)
                .timeout(Duration::from_secs(10));
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request.send().await?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                *self.token.lock().await = None;
                continue;
            }
            return check("Twitch", response).await;
        }
        unreachable!("the loop returns on its second attempt")
    }

    pub async fn conduits(&self) -> Result<Vec<Conduit>> {
        let response = self
            .send(reqwest::Method::GET, "/eventsub/conduits", &[], None)
            .await?;
        Ok(response.json::<Data<Conduit>>().await?.data)
    }

    pub async fn create_conduit(&self, shard_count: u32) -> Result<Conduit> {
        let body = json!({ "shard_count": shard_count });
        let response = self
            .send(
                reqwest::Method::POST,
                "/eventsub/conduits",
                &[],
                Some(&body),
            )
            .await?;
        first(response.json::<Data<Conduit>>().await?)
    }

    pub async fn resize_conduit(&self, id: &str, shard_count: u32) -> Result<()> {
        let body = json!({ "id": id, "shard_count": shard_count });
        self.send(
            reqwest::Method::PATCH,
            "/eventsub/conduits",
            &[],
            Some(&body),
        )
        .await?;
        Ok(())
    }

    pub async fn delete_conduit(&self, id: &str) -> Result<()> {
        self.send(
            reqwest::Method::DELETE,
            "/eventsub/conduits",
            &[("id", id)],
            None,
        )
        .await?;
        Ok(())
    }

    /// Points a conduit shard at a WebSocket session of ours.
    pub async fn assign_shard(&self, conduit_id: &str, shard: u32, session_id: &str) -> Result<()> {
        let body = json!({
            "conduit_id": conduit_id,
            "shards": [{
                "id": shard.to_string(),
                "transport": { "method": "websocket", "session_id": session_id },
            }],
        });
        let response = self
            .send(
                reqwest::Method::PATCH,
                "/eventsub/conduits/shards",
                &[],
                Some(&body),
            )
            .await?;
        #[derive(Deserialize)]
        struct Assigned {
            #[serde(default)]
            errors: Vec<serde_json::Value>,
        }
        let assigned: Assigned = response.json().await?;
        if let Some(error) = assigned.errors.first() {
            return Err(Error::Other(format!(
                "Twitch refused shard {shard}: {error}"
            )));
        }
        Ok(())
    }

    pub async fn subscribe(
        &self,
        kind: &str,
        condition: &serde_json::Value,
        conduit_id: &str,
    ) -> Result<Subscribed> {
        let body = json!({
            "type": kind,
            "version": "1",
            "condition": condition,
            "transport": { "method": "conduit", "conduit_id": conduit_id },
        });
        match self
            .send(
                reqwest::Method::POST,
                "/eventsub/subscriptions",
                &[],
                Some(&body),
            )
            .await
        {
            Ok(response) => {
                let created = first(response.json::<Data<IdOnly>>().await?)?;
                Ok(Subscribed::Created(created.id))
            }
            Err(error) if error.status() == Some(reqwest::StatusCode::CONFLICT) => {
                Ok(Subscribed::Exists)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn unsubscribe(&self, id: &str) -> Result<()> {
        match self
            .send(
                reqwest::Method::DELETE,
                "/eventsub/subscriptions",
                &[("id", id)],
                None,
            )
            .await
        {
            Err(error) if error.status() == Some(reqwest::StatusCode::NOT_FOUND) => Ok(()),
            other => other.map(|_| ()),
        }
    }

    /// Every subscription of the app, optionally of one type.
    pub async fn subscriptions(&self, kind: Option<&str>) -> Result<Vec<Subscription>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut query = Vec::new();
            if let Some(kind) = kind {
                query.push(("type", kind));
            }
            if let Some(cursor) = &cursor {
                query.push(("after", cursor.as_str()));
            }
            let response = self
                .send(
                    reqwest::Method::GET,
                    "/eventsub/subscriptions",
                    &query,
                    None,
                )
                .await?;
            let page: Data<Subscription> = response.json().await?;
            all.extend(page.data);
            match page.pagination.cursor {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => return Ok(all),
            }
        }
    }

    /// A user by login: `(id, login)`, if there is one.
    pub async fn user_by_login(&self, login: &str) -> Result<Option<(String, String)>> {
        #[derive(Deserialize)]
        struct User {
            id: String,
            login: String,
        }
        let response = self
            .send(reqwest::Method::GET, "/users", &[("login", login)], None)
            .await?;
        let users: Data<User> = response.json().await?;
        Ok(users
            .data
            .into_iter()
            .next()
            .map(|user| (user.id, user.login)))
    }

    /// Posts a chat message as `sender_id` (a bot that granted `user:bot`).
    pub async fn send_chat(
        &self,
        broadcaster_id: &str,
        sender_id: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> Result<()> {
        let mut body = json!({
            "broadcaster_id": broadcaster_id,
            "sender_id": sender_id,
            "message": text,
        });
        if let Some(reply_to) = reply_to {
            body["reply_parent_message_id"] = json!(reply_to);
        }
        let response = self
            .send(reqwest::Method::POST, "/chat/messages", &[], Some(&body))
            .await?;
        #[derive(Deserialize)]
        struct Sent {
            is_sent: bool,
            drop_reason: Option<serde_json::Value>,
        }
        let sent = first(response.json::<Data<Sent>>().await?)?;
        if !sent.is_sent {
            return Err(Error::Other(format!(
                "Twitch dropped the message: {}",
                sent.drop_reason.unwrap_or_default()
            )));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct IdOnly {
    id: String,
}

fn first<T>(data: Data<T>) -> Result<T> {
    data.data
        .into_iter()
        .next()
        .ok_or_else(|| Error::Other("Twitch returned no data".into()))
}
