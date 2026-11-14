//! A chat message from any platform, as every instance handles it. The Twitch
//! shards, the Kick webhook and the YouTube watcher publish them to the
//! environment's `inbound` subject; each is handled by one instance of its
//! `chat` queue group. Publishing costs no round trip, so reading a busy chat
//! never waits on anything: what a message needs is looked up by the handler.

use db::chat::ChatPlatform;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundMessage {
    pub platform: ChatPlatform,
    /// The channel (broadcaster) it was sent in.
    pub channel_id: String,
    /// The channel's login, for logs. Empty from instances that predate it.
    #[serde(default)]
    pub channel_login: String,
    /// Twitch: the bot whose subscription delivered it. Only that bot's links
    /// answer, so a channel read by two bots doesn't answer twice.
    pub bot_id: Option<String>,
    pub message_id: String,
    /// What the platform calls this delivery (Twitch's EventSub message, Kick's
    /// webhook delivery, YouTube's chat message). A platform may deliver twice,
    /// around a reconnect or as a retried webhook, with the same ID: the handler
    /// runs a command once ([`seen_key`](Self::seen_key)). `None` from instances
    /// that checked this before publishing.
    #[serde(default)]
    pub delivery_id: Option<String>,
    pub chatter: Chatter,
    pub text: String,
}

impl InboundMessage {
    /// The key in the `seen` bucket that says this delivery was handled already.
    pub fn seen_key(&self) -> Option<String> {
        self.delivery_id
            .as_deref()
            .map(|delivery| crate::kv::key(&[self.platform.as_str(), delivery]))
    }

    /// The channel as a log can show it: its login, else its platform ID.
    pub fn channel_label(&self) -> &str {
        if self.channel_login.is_empty() {
            &self.channel_id
        } else {
            &self.channel_login
        }
    }
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

#[cfg(test)]
mod tests {
    use super::InboundMessage;

    #[test]
    fn messages_from_instances_that_checked_repeats_themselves_still_decode() {
        let message: InboundMessage = serde_json::from_str(
            r#"{"platform":"kick","channel_id":"200","bot_id":null,"message_id":"01JH5",
                "chatter":{"id":"77","login":"bob","display_name":"Bob","moderator":false},
                "text":"!bitrate"}"#,
        )
        .unwrap();
        assert_eq!(message.delivery_id, None);
        // Nothing to check it by: the publisher did.
        assert_eq!(message.seen_key(), None);
    }

    #[test]
    fn a_delivery_is_remembered_per_platform() {
        let mut message: InboundMessage = serde_json::from_str(
            r#"{"platform":"kick","channel_id":"200","bot_id":null,"message_id":"01JH5",
                "delivery_id":"01JH5:retry/1",
                "chatter":{"id":"77","login":"bob","display_name":"Bob","moderator":false},
                "text":"!bitrate"}"#,
        )
        .unwrap();
        // Only characters a NATS KV key takes.
        assert_eq!(message.seen_key().as_deref(), Some("kick.01JH5_retry_1"));
        message.platform = db::chat::ChatPlatform::YouTube;
        assert_eq!(message.seen_key().as_deref(), Some("youtube.01JH5_retry_1"));
    }
}
