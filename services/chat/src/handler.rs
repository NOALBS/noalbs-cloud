//! Handles chat messages: every instance subscribes to [`inbound::SUBJECT`] in
//! the [`inbound::QUEUE`] group, so each message is handled by one of them.
//!
//! A message runs on every switcher its channel is linked to (read by the bot
//! that delivered it, on Twitch), each with its own prefix, commands, roles and
//! bot. With several, replies start with `[<switcher name>]`.
//!
//! Most messages aren't commands, so they are dropped on what [`crate::cache`]
//! holds (the channel's links, with their prefixes): no database query.

use std::sync::Arc;

use db::chat::{ChatLink, ChatPlatform};
use futures_util::StreamExt;
use tokio::sync::Semaphore;
use tracing::Instrument;

use crate::{
    app::State,
    commands::{self, Role, run::Ctx},
    error::{Error, Result},
    inbound::InboundMessage,
    kv, messages, reply,
};

/// Messages handled at the same time. A message that finds none free waits, so a
/// flood queues in NATS rather than as tasks and database connections here.
const MAX_IN_FLIGHT: usize = 256;

pub async fn run(state: State) {
    let mut messages = match state
        .nats
        .queue_subscribe(state.subject("inbound"), state.queue("chat"))
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to chat messages");
            return;
        }
    };
    let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    while let Some(message) = messages.next().await {
        let Ok(message) = serde_json::from_slice::<InboundMessage>(&message.payload) else {
            tracing::warn!("Dropped an unreadable chat message");
            continue;
        };
        let Ok(permit) = in_flight.clone().acquire_owned().await else {
            return;
        };
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = handle(&state, &message).await {
                tracing::warn!(%error, channel = message.channel_id, "Couldn't handle a chat message");
            }
            drop(permit);
        });
    }
}

/// The links that answer this message: on Twitch, those read by the bot whose
/// subscription delivered it (Kick and YouTube deliver once per channel).
fn answering(links: Vec<ChatLink>, message: &InboundMessage) -> Vec<ChatLink> {
    links
        .into_iter()
        .filter(|link| {
            message.platform != ChatPlatform::Twitch
                || link.bot_platform_user_id.as_deref() == message.bot_id.as_deref()
        })
        .collect()
}

/// Whether the message starts with one of the links' prefixes, i.e. might be a
/// command of one of them.
fn might_be_a_command(links: &[ChatLink], message: &InboundMessage) -> bool {
    links
        .iter()
        .any(|link| commands::parse(&message.text, &link.command_prefix).is_some())
}

pub async fn handle(state: &State, message: &InboundMessage) -> Result<()> {
    let links = state
        .cache
        .links
        .get_or_load((message.platform, message.channel_id.clone()), || {
            db::chat::links_for_channel(&state.db, message.platform, &message.channel_id)
        })
        .await?;
    let links = answering(links, message);
    if links.is_empty() {
        tracing::debug!(
            channel = message.channel_id,
            bot = message.bot_id,
            "No switcher answers in this channel for this bot"
        );
        return Ok(());
    }
    if !might_be_a_command(&links, message) {
        return Ok(());
    }
    // Bots never command each other (or themselves).
    if state
        .cache
        .bot_ids
        .get_or_load(message.platform, || {
            db::chat::bot_user_ids(&state.db, message.platform)
        })
        .await?
        .contains(&message.chatter.id)
    {
        tracing::debug!(
            chatter = message.chatter.login,
            "Ignoring a message from a bot account"
        );
        return Ok(());
    }
    let several = links.len() > 1;
    for link in &links {
        if let Err(error) = handle_link(state, message, link, several).await {
            tracing::warn!(%error, switcher = %link.switcher_id, "A chat command failed");
        }
    }
    Ok(())
}

async fn handle_link(
    state: &State,
    message: &InboundMessage,
    link: &ChatLink,
    several: bool,
) -> Result<()> {
    let Some(switcher) = db::switchers::get(&state.db, link.switcher_id).await? else {
        return Ok(());
    };
    let Some(invocation) = commands::parse(&message.text, &switcher.command_prefix) else {
        return Ok(());
    };
    if db::chat::is_ignored(
        &state.db,
        switcher.id,
        message.platform,
        &message.chatter.id,
    )
    .await?
    {
        return Ok(());
    }
    let commands = db::chat_commands::list_for_switcher(&state.db, switcher.id).await?;
    let Some(command) = commands::resolve(&invocation.word, &commands) else {
        tracing::debug!(word = invocation.word, "Not a command of this switcher");
        return Ok(());
    };
    let role = role_of(state, message, link, switcher.mod_commands_enabled).await?;
    if !commands::allowed(role, command, switcher.public_commands_enabled) {
        tracing::debug!(
            command = command.command,
            ?role,
            chatter = message.chatter.login,
            "Not allowed"
        );
        return Ok(());
    }
    // Like the public, subscribers and VIPs can be many.
    if role < Role::Moderator {
        let cooldown = kv::key(&[
            message.platform.as_str(),
            &message.channel_id,
            &switcher.id.to_string(),
            &command.command,
        ]);
        if !kv::first_time(&state.buckets.cooldowns, &cooldown).await? {
            tracing::debug!(command = command.command, "Cooling down");
            return Ok(());
        }
    }

    // A trace per command (not per chat message): what it does through `control`
    // and obs-link, and the reply.
    let platform = message.platform.label();
    let span = tracing::info_span!(
        "chat.command",
        otel.name = format!("!{}", command.command),
        chat.command = command.command,
        chat.platform = platform,
        switcher.id = %switcher.id,
    );
    tracing::info!(
        parent: &span,
        chatter = message.chatter.id,
        "Chat command"
    );
    run_command(
        state,
        message,
        link,
        several,
        switcher,
        &command.command,
        &invocation.args,
    )
    .instrument(span)
    .await
}

async fn run_command(
    state: &State,
    message: &InboundMessage,
    link: &ChatLink,
    several: bool,
    switcher: db::switchers::Switcher,
    command: &str,
    args: &[String],
) -> Result<()> {
    crate::metrics::command(message.platform, command);
    let platform = message.platform.label();
    let language = switcher.language.clone();
    let name = switcher.name.clone();
    let threaded = switcher.threaded_replies;
    let ctx = Ctx {
        state,
        switcher,
        platform: message.platform,
        by: format!("{} ({platform} chat)", message.chatter.display_name),
    };
    let Some(msg) = commands::run::run(&ctx, command, args).await else {
        return Ok(());
    };
    let mut text = messages::text(&language, &msg);
    if several {
        text = format!("[{name}] {text}");
    }

    let (Some(bot_id), Some(bot_user_id)) = (link.bot_id, link.bot_platform_user_id.as_deref())
    else {
        return Err(Error::Other("no bot to answer with".into()));
    };
    reply::send(
        state,
        message.platform,
        &message.channel_id,
        &reply::Bot {
            id: bot_id,
            platform_user_id: bot_user_id,
        },
        &text,
        threaded.then_some(message.message_id.as_str()),
    )
    .await
}

async fn role_of(
    state: &State,
    message: &InboundMessage,
    link: &ChatLink,
    mod_enabled: bool,
) -> Result<Role> {
    if message.chatter.id == message.channel_id
        || db::chat::is_member_account(
            &state.db,
            link.workspace_id,
            message.platform,
            &message.chatter.id,
        )
        .await?
    {
        return Ok(Role::Admin);
    }
    Ok(commands::badge_role(&message.chatter, mod_enabled))
}

#[cfg(test)]
mod tests {
    use db::chat::{ChatLink, ChatPlatform};
    use uuid::Uuid;

    use super::{answering, might_be_a_command};
    use crate::inbound::{Chatter, InboundMessage};

    fn link(bot: &str) -> ChatLink {
        ChatLink {
            switcher_id: Uuid::new_v4(),
            switcher_name: "Main".into(),
            workspace_id: Uuid::new_v4(),
            command_prefix: "!".into(),
            bot_id: Some(Uuid::new_v4()),
            bot_platform_user_id: Some(bot.into()),
            bot_login: Some(bot.into()),
        }
    }

    fn message(platform: ChatPlatform, bot: Option<&str>) -> InboundMessage {
        InboundMessage {
            platform,
            channel_id: "100".into(),
            bot_id: bot.map(str::to_owned),
            message_id: "m".into(),
            chatter: Chatter {
                id: "5".into(),
                login: "viewer".into(),
                display_name: "Viewer".into(),
                moderator: false,
                vip: false,
                subscriber: false,
            },
            text: "!bitrate".into(),
        }
    }

    #[test]
    fn only_messages_with_a_links_prefix_can_be_commands() {
        let mut with_prefix = message(ChatPlatform::Twitch, Some("1"));
        let mut other = link("1");
        other.command_prefix = "?".into();
        let links = vec![link("1"), other];

        with_prefix.text = "hello chat".into();
        assert!(!might_be_a_command(&links, &with_prefix));
        with_prefix.text = "!bitrate".into();
        assert!(might_be_a_command(&links, &with_prefix));
        with_prefix.text = "  ?switch live".into();
        assert!(might_be_a_command(&links, &with_prefix));
        // Nothing after the prefix is no command either.
        with_prefix.text = "!".into();
        assert!(!might_be_a_command(&links, &with_prefix));
    }

    #[test]
    fn on_twitch_only_the_delivering_bot_answers() {
        let links = vec![link("1"), link("4")];
        let answering_links = answering(links.clone(), &message(ChatPlatform::Twitch, Some("4")));
        assert_eq!(answering_links.len(), 1);
        assert_eq!(
            answering_links[0].bot_platform_user_id.as_deref(),
            Some("4")
        );
        // Kick delivers once per channel: every link answers.
        assert_eq!(
            answering(links.clone(), &message(ChatPlatform::Kick, None)).len(),
            2
        );
        assert_eq!(
            answering(links, &message(ChatPlatform::YouTube, None)).len(),
            2
        );
    }
}
