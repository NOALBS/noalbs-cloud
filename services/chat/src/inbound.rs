//! A chat message from any platform, as every instance handles it. The Twitch
//! shards and the Kick webhook publish them to the environment's `inbound`
//! subject; each is handled by one instance of its `chat` queue group.

use db::chat::ChatPlatform;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundMessage {
    pub platform: ChatPlatform,
    /// The channel (broadcaster) it was sent in.
    pub channel_id: String,
    /// Twitch: the bot whose subscription delivered it. Only that bot's links
    /// answer, so a channel read by two bots doesn't answer twice.
    pub bot_id: Option<String>,
    pub message_id: String,
    pub chatter: Chatter,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chatter {
    pub id: String,
    pub login: String,
    pub display_name: String,
    /// Has the (lead) moderator badge (the broadcaster counts as one too).
    pub moderator: bool,
    /// Has the VIP badge. Defaulted, like `subscriber`, so messages from
    /// instances that predate them still decode.
    #[serde(default)]
    pub vip: bool,
    /// Has the subscriber (or founder) badge.
    #[serde(default)]
    pub subscriber: bool,
}

/// Hands a message to whichever instance takes it from the queue group.
pub async fn publish(state: &crate::app::Shared, message: &InboundMessage) {
    match serde_json::to_vec(message) {
        Ok(payload) => {
            if let Err(error) = state
                .nats
                .publish(state.subject("inbound"), payload.into())
                .await
            {
                tracing::warn!(%error, "Couldn't publish a chat message");
            }
        }
        Err(error) => tracing::warn!(%error, "Couldn't serialize a chat message"),
    }
}
