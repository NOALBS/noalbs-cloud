//! Posting in chat as a link's bot, within the platform's rate limit (shared by
//! every instance through [`kv::take_token`]).

use std::time::Duration;

use db::chat::{ChatPlatform, TokenOwner};
use uuid::Uuid;

use crate::{app::State, error::Result, kv, metrics, tokens, youtube};

/// Twitch lets a bot send 20 messages per 30 s in a channel where it isn't a
/// moderator; we keep Kick and YouTube to the same.
const PER_WINDOW: f64 = 20.0;
const WINDOW: Duration = Duration::from_secs(30);
/// A reply that can't go out within this is dropped.
const MAX_WAIT: Duration = Duration::from_secs(3);

pub struct Bot<'a> {
    pub id: Uuid,
    pub platform_user_id: &'a str,
}

/// The longest message the platform takes, in characters.
fn max_chars(platform: ChatPlatform) -> usize {
    match platform {
        ChatPlatform::Twitch | ChatPlatform::Kick => 500,
        ChatPlatform::YouTube => 200,
    }
}

pub fn truncate(platform: ChatPlatform, text: &str) -> String {
    let max = max_chars(platform);
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut short: String = text.chars().take(max - 1).collect();
    short.push('…');
    short
}

/// Posts `text` in the channel. YouTube has no replies to a message, so
/// `reply_to` only counts on Twitch and Kick.
pub async fn send(
    state: &State,
    platform: ChatPlatform,
    channel_id: &str,
    bot: &Bot<'_>,
    text: &str,
    reply_to: Option<&str>,
) -> Result<()> {
    let posted = post(state, platform, channel_id, bot, text, reply_to).await;
    metrics::reply(platform, posted.as_ref().map_or("error", |outcome| outcome));
    posted.map(|_| ())
}

/// Like [`send`], saying what became of it (the `outcome` of `noalbs.chat.replies`).
async fn post(
    state: &State,
    platform: ChatPlatform,
    channel_id: &str,
    bot: &Bot<'_>,
    text: &str,
    reply_to: Option<&str>,
) -> Result<&'static str> {
    // Only a live YouTube channel has a chat to post in.
    let live_chat_id = match platform {
        ChatPlatform::YouTube => match youtube::live_chat_of(state, channel_id).await? {
            Some(live_chat_id) => Some(live_chat_id),
            None => {
                tracing::debug!(channel_id, "Dropped a YouTube reply: no live chat");
                return Ok("no_live_chat");
            }
        },
        _ => None,
    };
    let limit = kv::key(&[platform.as_str(), bot.platform_user_id, channel_id]);
    if !kv::take_token(
        &state.buckets.send_limits,
        &limit,
        PER_WINDOW,
        WINDOW,
        MAX_WAIT,
    )
    .await?
    {
        tracing::warn!(channel_id, "Dropped a chat reply: rate limited");
        return Ok("rate_limited");
    }
    let text = truncate(platform, text);
    let not_configured =
        || crate::error::Error::Other(format!("{} isn't configured", platform.label()));
    let sent = match platform {
        ChatPlatform::Twitch => {
            let twitch = state.twitch.clone().ok_or_else(not_configured)?;
            twitch
                .helix
                .send_chat(channel_id, bot.platform_user_id, &text, reply_to)
                .await
        }
        ChatPlatform::Kick => {
            let kick = state.kick.clone().ok_or_else(not_configured)?;
            let token = tokens::access_token(state, TokenOwner::Bot(bot.id)).await?;
            kick.send_chat(&token, channel_id, &text, reply_to).await
        }
        ChatPlatform::YouTube => {
            let youtube = state.youtube.clone().ok_or_else(not_configured)?;
            let live_chat_id = live_chat_id.unwrap_or_default();
            let token = tokens::access_token(state, TokenOwner::Bot(bot.id)).await?;
            let sent = youtube.send_chat(&token, &live_chat_id, &text).await;
            if let Err(error) = &sent {
                youtube::record_error(state, youtube::is_quota(error), error.to_string()).await;
            }
            sent
        }
    };
    sent.map(|()| "sent")
}

#[cfg(test)]
mod tests {
    use db::chat::ChatPlatform;

    use super::truncate;

    #[test]
    fn long_replies_are_cut_to_the_platform_limit() {
        assert_eq!(truncate(ChatPlatform::Twitch, "hi"), "hi");
        let long = "é".repeat(600);
        let cut = truncate(ChatPlatform::Kick, &long);
        assert_eq!(cut.chars().count(), 500);
        assert!(cut.ends_with('…'));
        assert_eq!(truncate(ChatPlatform::YouTube, &long).chars().count(), 200);
    }
}
