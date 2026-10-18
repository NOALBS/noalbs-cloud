//! Platform events other than chat messages (raids, revoked access): the Twitch
//! shards and the Kick raid socket ([`crate::kick::raids`]) publish them to the
//! environment's `events` subject and one instance of its queue group handles
//! each, like chat messages.

use db::chat::ChatPlatform;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::State,
    error::Result,
    messages::{self, Msg},
    reply,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEvent {
    /// `from_id` raided `to_name` (only outgoing raids are subscribed to).
    Raid {
        /// Twitch from instances older than this field.
        #[serde(default = "twitch")]
        platform: ChatPlatform,
        from_id: String,
        to_name: String,
        /// For the channel's URL (the login on Twitch, the slug on Kick). Empty
        /// from instances older than this field; the display name, lowercased,
        /// stands in then.
        #[serde(default)]
        to_login: String,
    },
    /// The account took our app's access away (all of its grants).
    Revoked {
        platform: ChatPlatform,
        user_id: String,
    },
}

fn twitch() -> ChatPlatform {
    ChatPlatform::Twitch
}

pub async fn publish(state: &crate::app::Shared, event: &ChatEvent) {
    match serde_json::to_vec(event) {
        Ok(payload) => {
            if let Err(error) = state
                .nats
                .publish(state.subject("events"), payload.into())
                .await
            {
                tracing::warn!(%error, "Couldn't publish a chat event");
            }
        }
        Err(error) => tracing::warn!(%error, "Couldn't serialize a chat event"),
    }
}

pub async fn run(state: State) {
    let mut events = match state
        .nats
        .queue_subscribe(state.subject("events"), state.queue("chat"))
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to chat events");
            return;
        }
    };
    while let Some(message) = events.next().await {
        let Ok(event) = serde_json::from_slice::<ChatEvent>(&message.payload) else {
            tracing::warn!("Dropped an unreadable chat event");
            continue;
        };
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = handle(&state, &event).await {
                tracing::warn!(%error, ?event, "Couldn't handle a chat event");
            }
        });
    }
}

async fn handle(state: &State, event: &ChatEvent) -> Result<()> {
    match event {
        ChatEvent::Raid {
            platform,
            from_id,
            to_name,
            to_login,
        } => {
            let login = if to_login.is_empty() {
                to_name.to_lowercase()
            } else {
                to_login.clone()
            };
            raiding(state, *platform, from_id, to_name, &login).await
        }
        ChatEvent::Revoked { platform, user_id } => {
            let revoked = db::chat::revoke_account(&state.db, *platform, user_id).await?;
            if revoked > 0 {
                tracing::info!(
                    platform = platform.as_str(),
                    user_id,
                    revoked,
                    "An account took our access away"
                );
                // Its subscriptions go; a revoked bot is replaced by the shared one.
                state.request_reconcile().await;
            }
            Ok(())
        }
    }
}

/// `channel_id` raided out: its switchers that stop after a raid stop the stream,
/// and say so in chat if they announce raids.
async fn raiding(
    state: &State,
    platform: ChatPlatform,
    channel_id: &str,
    to_name: &str,
    to_login: &str,
) -> Result<()> {
    let links = db::chat::links_for_channel(&state.db, platform, channel_id).await?;
    let several = links.len() > 1;
    for link in &links {
        let Some(switcher) = db::switchers::get(&state.db, link.switcher_id).await? else {
            continue;
        };
        if !switcher.auto_stop_on_raid {
            continue;
        }
        let streaming = state
            .control
            .status(switcher.id)
            .await
            .is_some_and(|status| status.obs_connected && status.streaming);
        if !streaming {
            continue;
        }
        let by = format!("the raid to {to_name}");
        if let Err(error) = state.control.stream(switcher.id, false, &by).await {
            tracing::warn!(%error, switcher = %switcher.id, "Couldn't stop the stream after a raid");
            continue;
        }
        tracing::info!(switcher = %switcher.id, "Stopped the stream after a raid");
        if !switcher.announce_raid {
            continue;
        }
        let (Some(bot_id), Some(bot_user_id)) = (link.bot_id, link.bot_platform_user_id.as_deref())
        else {
            continue;
        };
        let mut text = messages::text(
            &switcher.language,
            &Msg::RaidStopping {
                to: to_name.to_owned(),
                url: channel_url(platform, to_login),
            },
        );
        if several {
            text = format!("[{}] {text}", switcher.name);
        }
        send(state, platform, channel_id, bot_id, bot_user_id, &text).await;
    }
    Ok(())
}

/// A channel's page, from its login (Twitch), slug (Kick) or handle (YouTube).
fn channel_url(platform: ChatPlatform, login: &str) -> String {
    match platform {
        ChatPlatform::Twitch => format!("https://twitch.tv/{login}"),
        ChatPlatform::Kick => format!("https://kick.com/{login}"),
        ChatPlatform::YouTube => format!("https://www.youtube.com/@{login}"),
    }
}

async fn send(
    state: &State,
    platform: ChatPlatform,
    channel_id: &str,
    bot_id: Uuid,
    bot_user_id: &str,
    text: &str,
) {
    let bot = reply::Bot {
        id: bot_id,
        platform_user_id: bot_user_id,
    };
    if let Err(error) = reply::send(state, platform, channel_id, &bot, text, None).await {
        tracing::warn!(%error, channel_id, "Couldn't post in chat");
    }
}

#[cfg(test)]
mod tests {
    use db::chat::ChatPlatform;

    use super::{ChatEvent, channel_url};

    #[test]
    fn raids_from_older_instances_still_decode() {
        // Sent by an instance from before `to_login` existed (during a rolling deploy).
        let old: ChatEvent =
            serde_json::from_str(r#"{"kind":"raid","from_id":"100","to_name":"Bob"}"#).unwrap();
        assert_eq!(
            old,
            ChatEvent::Raid {
                platform: ChatPlatform::Twitch,
                from_id: "100".into(),
                to_name: "Bob".into(),
                to_login: String::new(),
            }
        );
    }

    #[test]
    fn raided_channels_link_to_their_platform() {
        assert_eq!(
            channel_url(ChatPlatform::Twitch, "bob"),
            "https://twitch.tv/bob"
        );
        assert_eq!(
            channel_url(ChatPlatform::Kick, "bob-2"),
            "https://kick.com/bob-2"
        );
    }
}
