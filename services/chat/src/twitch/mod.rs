//! Twitch chat: an EventSub **conduit** (one per environment, stored in
//! `chat_platform_state`) whose shards are WebSockets spread over the running
//! instances ([`shards`]). Twitch hands each event to one enabled shard, so each
//! chat message reaches one instance, which publishes it for handling.

pub mod eventsub;
pub mod helix;
pub mod shards;

use db::chat::ChatPlatform;
use tokio::sync::RwLock;

use crate::{config::TwitchConfig, error::Result};

pub const CONDUIT_KEY: &str = "twitch_conduit_id";

// What we subscribe to, as recorded in `chat_subscriptions.event_type`.
/// Chat in a channel, read by a bot.
pub const CHAT_MESSAGE: &str = "channel.chat.message";
/// The channel raids someone, which ends the stream (`auto_stop_on_raid`). Only
/// outgoing raids matter, so the condition is `from_broadcaster_user_id`.
pub const RAID_OUT: &str = "channel.raid.out";
/// Someone took our app's access away (one subscription for the app).
pub const AUTH_REVOKE: &str = "user.authorization.revoke";

/// The Twitch subscription type and condition for one of our kinds.
pub fn subscription_spec(
    kind: &str,
    channel_id: &str,
    bot_id: &str,
    client_id: &str,
) -> (&'static str, serde_json::Value) {
    use serde_json::json;
    match kind {
        RAID_OUT => (
            "channel.raid",
            json!({ "from_broadcaster_user_id": channel_id }),
        ),
        AUTH_REVOKE => (AUTH_REVOKE, json!({ "client_id": client_id })),
        _ => (
            CHAT_MESSAGE,
            json!({ "broadcaster_user_id": channel_id, "user_id": bot_id }),
        ),
    }
}

/// Whether a subscription Twitch lists has this type and condition (Twitch
/// fills in the condition fields we didn't set, so only ours are compared).
pub fn matches(sub: &helix::Subscription, kind: &str, condition: &serde_json::Value) -> bool {
    sub.kind == kind
        && condition.as_object().is_some_and(|wanted| {
            wanted
                .iter()
                .all(|(key, value)| sub.condition.get(key) == Some(value))
        })
}

pub struct Twitch {
    pub config: TwitchConfig,
    pub helix: helix::Helix,
    conduit_id: RwLock<Option<String>>,
}

impl Twitch {
    pub fn new(config: TwitchConfig, http: reqwest::Client) -> Self {
        Self {
            helix: helix::Helix::new(config.clone(), http),
            config,
            conduit_id: RwLock::new(None),
        }
    }

    /// The conduit, as last made sure of.
    pub async fn conduit_id(&self) -> Option<String> {
        self.conduit_id.read().await.clone()
    }

    /// Makes sure our conduit exists with the configured shard count, creating
    /// it if needed. Several instances may race here: the database keeps one
    /// conduit and the losers delete theirs.
    pub async fn ensure_conduit(&self, pool: &db::DbPool) -> Result<String> {
        let stored = db::chat::platform_state(pool, CONDUIT_KEY).await?;
        let existing = self.helix.conduits().await?;
        if let Some(stored) = &stored
            && let Some(conduit) = existing.iter().find(|conduit| &conduit.id == stored)
        {
            if conduit.shard_count != self.config.shards {
                tracing::info!(
                    from = conduit.shard_count,
                    to = self.config.shards,
                    "Resizing the Twitch conduit"
                );
                self.helix
                    .resize_conduit(&conduit.id, self.config.shards)
                    .await?;
            }
            *self.conduit_id.write().await = Some(stored.clone());
            return Ok(stored.clone());
        }

        let created = self.helix.create_conduit(self.config.shards).await?;
        let winner =
            db::chat::replace_platform_state(pool, CONDUIT_KEY, stored.as_deref(), &created.id)
                .await?;
        if winner == created.id {
            tracing::info!(conduit = created.id, "Created the Twitch conduit");
            if stored.is_some() {
                // The old conduit is gone, and its subscriptions with it.
                db::chat::forget_subscriptions(pool, ChatPlatform::Twitch).await?;
            }
        } else if let Err(error) = self.helix.delete_conduit(&created.id).await {
            tracing::warn!(%error, "Couldn't delete a conduit another instance beat");
        }
        *self.conduit_id.write().await = Some(winner.clone());
        Ok(winner)
    }
}
