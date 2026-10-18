//! Keeps the platforms' event subscriptions in line with the chat links.
//!
//! What the links need ([`db::chat::wanted_subscriptions`]) is compared with
//! what we recorded (`chat_subscriptions`): missing ones are created, ones
//! nothing needs are deleted. A subscription belongs to a channel (+ the bot
//! reading it, on Twitch), so unlinking one of several switchers changes
//! nothing; a channel nothing links is deleted with its grant once its
//! subscriptions are gone. It runs when the API says links changed (`live::chat_reconcile_subject`)
//! and every [`INTERVAL`]; the periodic run also sweeps the platforms for
//! subscriptions we don't know of. One instance at a time runs it (a lease);
//! a request that arrives meanwhile is picked up when the running one ends.

use std::{collections::BTreeSet, sync::Arc, time::Duration};

use bytes::Bytes;
use db::chat::{ChatPlatform, ChatSubscription, TokenOwner, WantedSubscription};
use futures_util::StreamExt;
use tokio::sync::Mutex;

use crate::{
    app::State,
    error::Result,
    kick,
    kv::{HEARTBEAT, Lease},
    tokens, twitch,
};

pub const INTERVAL: Duration = Duration::from_secs(600);

/// One subscription as the platform knows it: event type, channel, bot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Want {
    pub platform: ChatPlatform,
    pub event_type: &'static str,
    pub channel_id: String,
    /// The reading bot on Twitch; empty on Kick.
    pub bot_id: String,
}

/// What the links need: chat per channel and speaking bot, raids where a
/// linked switcher stops the stream on raiding out (Twitch, `raid_channels`), and
/// one revocation subscription for the Twitch app. `twitch` says whether Twitch is configured.
pub fn wants(chat: &[WantedSubscription], raid_channels: &[String], twitch: bool) -> Vec<Want> {
    let mut wants: Vec<Want> = chat
        .iter()
        .filter_map(|wanted| {
            Some(Want {
                platform: wanted.platform,
                event_type: match wanted.platform {
                    ChatPlatform::Twitch => twitch::CHAT_MESSAGE,
                    ChatPlatform::Kick => kick::CHAT_MESSAGE,
                    // No subscriptions: the YouTube watcher follows live chats.
                    ChatPlatform::YouTube => return None,
                },
                channel_id: wanted.platform_channel_id.clone(),
                bot_id: wanted.platform_bot_id.clone(),
            })
        })
        .collect();
    wants.extend(raid_channels.iter().map(|channel| Want {
        platform: ChatPlatform::Twitch,
        event_type: twitch::RAID_OUT,
        channel_id: channel.clone(),
        bot_id: String::new(),
    }));
    if twitch {
        wants.push(Want {
            platform: ChatPlatform::Twitch,
            event_type: twitch::AUTH_REVOKE,
            channel_id: String::new(),
            bot_id: String::new(),
        });
    }
    wants
}

/// What to create and what to delete so `existing` matches `wants`.
pub fn diff<'a>(
    wants: &'a [Want],
    existing: &'a [ChatSubscription],
) -> (Vec<&'a Want>, Vec<&'a ChatSubscription>) {
    let have: BTreeSet<(ChatPlatform, &str, &str, &str)> = existing
        .iter()
        .map(|sub| {
            (
                sub.platform,
                sub.event_type.as_str(),
                sub.platform_channel_id.as_str(),
                sub.platform_bot_id.as_str(),
            )
        })
        .collect();
    let want: BTreeSet<(ChatPlatform, &str, &str, &str)> = wants
        .iter()
        .map(|want| {
            (
                want.platform,
                want.event_type,
                want.channel_id.as_str(),
                want.bot_id.as_str(),
            )
        })
        .collect();
    let create = wants
        .iter()
        .filter(|want| {
            !have.contains(&(
                want.platform,
                want.event_type,
                want.channel_id.as_str(),
                want.bot_id.as_str(),
            ))
        })
        .collect();
    let delete = existing
        .iter()
        .filter(|sub| {
            !want.contains(&(
                sub.platform,
                sub.event_type.as_str(),
                sub.platform_channel_id.as_str(),
                sub.platform_bot_id.as_str(),
            ))
        })
        .collect();
    (create, delete)
}

/// Runs until the task is dropped: on request and every [`INTERVAL`].
pub async fn run(state: State) {
    let mut requests = match state
        .nats
        .queue_subscribe(
            live::chat_reconcile_subject(&state.environment),
            state.queue("chat-reconcile"),
        )
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to reconcile requests");
            return;
        }
    };
    let mut tick = tokio::time::interval(INTERVAL);
    loop {
        let sweep = tokio::select! {
            _ = tick.tick() => true,
            Some(_) = requests.next() => false,
        };
        request(&state, sweep).await;
    }
}

/// Runs a reconcile now if no other instance is, else leaves a note for it.
async fn request(state: &State, sweep: bool) {
    tracing::debug!(sweep, "Reconcile requested");
    let lease = match Lease::acquire(
        &state.buckets.locks,
        &state.key(&["reconcile"]),
        &state.instance_id,
    )
    .await
    {
        Ok(Some(lease)) => {
            tracing::debug!("Took the reconcile lease");
            Arc::new(Mutex::new(lease))
        }
        Ok(None) => {
            tracing::debug!("Another instance is reconciling; leaving a request");
            if let Err(error) = state
                .buckets
                .locks
                .put(
                    state.key(&["reconcile-pending"]),
                    Bytes::from(state.instance_id.clone()),
                )
                .await
            {
                tracing::warn!(%error, "Couldn't leave a reconcile request");
            }
            return;
        }
        Err(error) => {
            tracing::warn!(%error, "Couldn't take the reconcile lease");
            return;
        }
    };
    let renewing = {
        let lease = lease.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(HEARTBEAT).await;
                if !lease.lock().await.renew().await {
                    tracing::warn!("Lost the reconcile lease");
                    return;
                }
            }
        })
    };

    let mut sweep = sweep;
    loop {
        // Anything that asked before this point is covered by this run.
        let _ = state
            .buckets
            .locks
            .purge(state.key(&["reconcile-pending"]))
            .await;
        if let Err(error) = reconcile(state, sweep).await {
            tracing::warn!(%error, "Reconciling chat subscriptions failed");
        }
        match state
            .buckets
            .locks
            .get(state.key(&["reconcile-pending"]))
            .await
        {
            Ok(Some(_)) => sweep = false,
            _ => break,
        }
    }

    renewing.abort();
    // Wait for it to go, so this is the last handle on the lease.
    let _ = renewing.await;
    match Arc::try_unwrap(lease) {
        Ok(lease) => lease.into_inner().release().await,
        Err(_) => tracing::warn!("The reconcile lease is still shared; it lapses on its own"),
    }
}

#[tracing::instrument(name = "chat.reconcile", skip(state))]
pub async fn reconcile(state: &State, sweep: bool) -> Result<()> {
    tracing::debug!(sweep, "Reconciling");
    if let Some(twitch) = &state.twitch {
        twitch.ensure_conduit(&state.db).await?;
    }
    if sweep {
        sweep_platforms(state).await?;
    }
    let wanted = wants(
        &db::chat::wanted_subscriptions(&state.db).await?,
        &db::chat::raid_channels(&state.db, ChatPlatform::Twitch).await?,
        state.twitch.is_some(),
    );
    let existing = db::chat::subscriptions(&state.db).await?;
    let (create, delete) = diff(&wanted, &existing);
    if !create.is_empty() || !delete.is_empty() {
        tracing::info!(
            create = create.len(),
            delete = delete.len(),
            "Reconciling chat subscriptions"
        );
    }
    for sub in delete {
        if let Err(error) = unsubscribe(state, sub).await {
            tracing::warn!(%error, channel = sub.platform_channel_id, "Couldn't unsubscribe");
        }
    }
    for want in create {
        if let Err(error) = subscribe(state, want).await {
            tracing::warn!(
                %error,
                platform = want.platform.as_str(),
                channel = want.channel_id,
                "Couldn't subscribe to a channel"
            );
        }
    }
    // Channels nothing links any more go with their grants (tokens), once their
    // subscriptions are gone; one whose unsubscribe failed waits for the next run.
    let deleted = db::chat::delete_unlinked_channels(&state.db).await?;
    if deleted > 0 {
        tracing::info!(deleted, "Deleted unlinked chat channels");
    }
    Ok(())
}

async fn unsubscribe(state: &State, sub: &ChatSubscription) -> Result<()> {
    match sub.platform {
        ChatPlatform::Twitch => {
            if let Some(twitch) = &state.twitch {
                twitch
                    .helix
                    .unsubscribe(&sub.platform_subscription_id)
                    .await?;
            }
        }
        ChatPlatform::Kick => {
            if let Some(kick) = &state.kick {
                kick.unsubscribe(&sub.platform_subscription_id).await?;
            }
        }
        ChatPlatform::YouTube => {}
    }
    db::chat::forget_subscription(&state.db, sub.platform, &sub.platform_subscription_id).await?;
    Ok(())
}

async fn subscribe(state: &State, want: &Want) -> Result<()> {
    let id = match want.platform {
        ChatPlatform::Twitch => {
            let Some(twitch) = &state.twitch else {
                return Ok(());
            };
            let conduit = twitch.ensure_conduit(&state.db).await?;
            let (kind, condition) = twitch::subscription_spec(
                want.event_type,
                &want.channel_id,
                &want.bot_id,
                &twitch.config.client_id,
            );
            match twitch.helix.subscribe(kind, &condition, &conduit).await? {
                twitch::helix::Subscribed::Created(id) => id,
                twitch::helix::Subscribed::Exists => {
                    // Made before we recorded it: adopt it if it's on our conduit.
                    let found = twitch
                        .helix
                        .subscriptions(Some(kind))
                        .await?
                        .into_iter()
                        .find(|sub| twitch::matches(sub, kind, &condition));
                    match found {
                        Some(sub) if sub.transport.conduit_id.as_deref() == Some(&conduit) => {
                            sub.id
                        }
                        Some(sub) => {
                            // On an old conduit: replace it.
                            twitch.helix.unsubscribe(&sub.id).await?;
                            match twitch.helix.subscribe(kind, &condition, &conduit).await? {
                                twitch::helix::Subscribed::Created(id) => id,
                                twitch::helix::Subscribed::Exists => return Ok(()),
                            }
                        }
                        None => return Ok(()),
                    }
                }
            }
        }
        ChatPlatform::Kick => {
            let Some(kick) = &state.kick else {
                return Ok(());
            };
            match kick.subscribe(&want.channel_id, None).await {
                Ok(id) => id,
                // Not with the app token: try the streamer's own.
                Err(error) if error.is_unauthorized() => {
                    let channel = db::chat::channel_by_platform_id(
                        &state.db,
                        ChatPlatform::Kick,
                        &want.channel_id,
                    )
                    .await?
                    .ok_or(error)?;
                    let token =
                        tokens::access_token(state, TokenOwner::Channel(channel.id)).await?;
                    kick.subscribe(&want.channel_id, Some(&token)).await?
                }
                Err(error) => return Err(error),
            }
        }
        ChatPlatform::YouTube => return Ok(()),
    };
    db::chat::record_subscription(
        &state.db,
        want.platform,
        want.event_type,
        &want.channel_id,
        &want.bot_id,
        &id,
    )
    .await?;
    Ok(())
}

/// Deletes platform subscriptions we have no record of (e.g. from a crash
/// between creating and recording), and forgets records the platform lost, so
/// the next pass recreates them.
async fn sweep_platforms(state: &State) -> Result<()> {
    let recorded = db::chat::subscriptions(&state.db).await?;
    let known = |platform: ChatPlatform, id: &str| {
        recorded
            .iter()
            .any(|sub| sub.platform == platform && sub.platform_subscription_id == id)
    };

    if let Some(twitch) = &state.twitch
        && let Some(conduit) = twitch.conduit_id().await
    {
        let live = twitch.helix.subscriptions(None).await?;
        for sub in live
            .iter()
            .filter(|sub| sub.transport.conduit_id.as_deref() == Some(conduit.as_str()))
        {
            if !known(ChatPlatform::Twitch, &sub.id) {
                tracing::info!(id = sub.id, "Deleting an unknown Twitch subscription");
                twitch.helix.unsubscribe(&sub.id).await?;
            }
        }
        for sub in recorded
            .iter()
            .filter(|sub| sub.platform == ChatPlatform::Twitch)
        {
            let alive = live
                .iter()
                .any(|live| live.id == sub.platform_subscription_id && live.status == "enabled");
            if !alive {
                db::chat::forget_subscription(
                    &state.db,
                    sub.platform,
                    &sub.platform_subscription_id,
                )
                .await?;
            }
        }
    }

    if let Some(kick) = &state.kick {
        let live = kick.subscriptions().await?;
        for sub in live.iter().filter(|sub| sub.event == kick::CHAT_MESSAGE) {
            if !known(ChatPlatform::Kick, &sub.id) {
                tracing::info!(id = sub.id, "Deleting an unknown Kick subscription");
                kick.unsubscribe(&sub.id).await?;
            }
        }
        for sub in recorded
            .iter()
            .filter(|sub| sub.platform == ChatPlatform::Kick)
        {
            if !live
                .iter()
                .any(|live| live.id == sub.platform_subscription_id)
            {
                db::chat::forget_subscription(
                    &state.db,
                    sub.platform,
                    &sub.platform_subscription_id,
                )
                .await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use db::chat::{ChatPlatform, ChatSubscription, WantedSubscription};
    use uuid::Uuid;

    use super::{diff, wants};

    fn wanted(platform: ChatPlatform, channel: &str, bot: &str) -> WantedSubscription {
        WantedSubscription {
            platform,
            platform_channel_id: channel.into(),
            platform_bot_id: bot.into(),
        }
    }

    fn existing(platform: ChatPlatform, kind: &str, channel: &str, bot: &str) -> ChatSubscription {
        ChatSubscription {
            id: Uuid::new_v4(),
            platform,
            event_type: kind.into(),
            platform_channel_id: channel.into(),
            platform_bot_id: bot.into(),
            platform_subscription_id: format!("sub-{channel}-{bot}"),
        }
    }

    #[test]
    fn missing_ones_are_created_and_unneeded_ones_deleted() {
        let wants = wants(
            &[
                wanted(ChatPlatform::Twitch, "100", "1"),
                wanted(ChatPlatform::Twitch, "100", "4"),
                wanted(ChatPlatform::Kick, "200", ""),
            ],
            &[],
            false,
        );
        let existing = [
            existing(ChatPlatform::Twitch, "channel.chat.message", "100", "1"),
            // The channel's own bot was removed: its reader goes.
            existing(ChatPlatform::Twitch, "channel.chat.message", "300", "1"),
        ];
        let (create, delete) = diff(&wants, &existing);
        assert_eq!(
            create
                .iter()
                .map(|want| (
                    want.event_type,
                    want.channel_id.as_str(),
                    want.bot_id.as_str()
                ))
                .collect::<Vec<_>>(),
            [
                ("channel.chat.message", "100", "4"),
                ("chat.message.sent", "200", "")
            ]
        );
        assert_eq!(delete.len(), 1);
        assert_eq!(delete[0].platform_channel_id, "300");
    }

    #[test]
    fn outgoing_raids_where_wanted_and_one_revocation_subscription() {
        let wants = wants(
            &[wanted(ChatPlatform::Twitch, "100", "1")],
            &["300".to_owned()],
            true,
        );
        assert_eq!(
            wants
                .iter()
                .map(|want| (want.event_type, want.channel_id.as_str()))
                .collect::<Vec<_>>(),
            [
                ("channel.chat.message", "100"),
                ("channel.raid.out", "300"),
                ("user.authorization.revoke", ""),
            ]
        );
    }

    #[test]
    fn nothing_to_do_when_they_match() {
        let wants = wants(&[wanted(ChatPlatform::Kick, "200", "")], &[], false);
        let existing = [existing(ChatPlatform::Kick, "chat.message.sent", "200", "")];
        let (create, delete) = diff(&wants, &existing);
        assert!(create.is_empty() && delete.is_empty());
    }
}
