//! Chat notifications: when a switcher changes scenes on its own and has
//! `auto_switch_notification` on, or someone picks a scene in the dashboard and it
//! has `dashboard_switch_notification` on, its linked channels hear about it,
//! except those whose link has alerts off; a stream stopped by the offline or
//! ending timeout is said in every linked channel. Every instance listens to the
//! switchers' events in one queue group, so each switch is announced once.

use futures_util::StreamExt;
use live::{EventDetail, SwitchReason, SwitcherEvent};
use uuid::Uuid;

use crate::{
    app::State,
    error::Result,
    messages::{self, Msg},
    reply,
};

pub async fn run(state: State) {
    let mut events = match state
        .nats
        .queue_subscribe("switcher.*.event", state.queue("chat-notify"))
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::error!(%error, "Couldn't subscribe to switcher events");
            return;
        }
    };
    while let Some(message) = events.next().await {
        let Some(switcher_id) = message
            .subject
            .split('.')
            .nth(1)
            .and_then(|id| Uuid::parse_str(id).ok())
        else {
            continue;
        };
        let Ok(event) = serde_json::from_slice::<SwitcherEvent>(&message.payload) else {
            continue;
        };
        if let Some((msg, audience)) = notification(&event.detail) {
            let state = state.clone();
            tokio::spawn(async move {
                if let Err(error) = announce(&state, switcher_id, &msg, audience).await {
                    tracing::warn!(%error, %switcher_id, "Couldn't post a notification");
                }
            });
        }
    }
}

/// Which linked channels hear a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// Those with alerts on, when the switcher announces its own switches
    /// (`auto_switch_notification`).
    AutoSwitches,
    /// Those with alerts on, when the switcher announces scenes picked in the
    /// dashboard (`dashboard_switch_notification`).
    DashboardSwitches,
    /// All of them: the stream is ending, like NOALBS always said.
    Everyone,
}

/// What to say about an event, if anything: the switcher's own switches, a
/// scene picked in the dashboard (a command's switch is answered where it was
/// asked; one made in OBS itself isn't ours to announce) and the offline and
/// ending timeouts.
pub fn notification(detail: &EventDetail) -> Option<(Msg, Audience)> {
    match detail {
        EventDetail::SceneSwitched {
            to,
            reason: SwitchReason::Auto,
            server,
            bitrate,
            rtt,
            ..
        } => Some((
            Msg::AutoSwitched {
                scene: to.clone(),
                server: server.clone(),
                bitrate: *bitrate,
                rtt: *rtt,
            },
            Audience::AutoSwitches,
        )),
        EventDetail::SceneSwitched {
            to, picked: true, ..
        } => Some((Msg::SwitchedTo(to.clone()), Audience::DashboardSwitches)),
        EventDetail::OfflineTimeout { .. } => Some((Msg::OfflineTimeout, Audience::Everyone)),
        EventDetail::EndingTimeout { .. } => Some((Msg::EndingTimeout, Audience::Everyone)),
        _ => None,
    }
}

async fn announce(state: &State, switcher_id: Uuid, msg: &Msg, audience: Audience) -> Result<()> {
    let Some(switcher) = db::switchers::get(&state.db, switcher_id).await? else {
        return Ok(());
    };
    let wanted = match audience {
        Audience::AutoSwitches => switcher.auto_switch_notification,
        Audience::DashboardSwitches => switcher.dashboard_switch_notification,
        Audience::Everyone => true,
    };
    if !wanted {
        return Ok(());
    }
    let alerts_only = audience != Audience::Everyone;
    let channels = db::chat::channels_of_switcher(&state.db, switcher_id).await?;
    let text = messages::text(&switcher.language, msg);
    for channel in channels {
        if alerts_only && !channel.auto_switch_notification {
            continue;
        }
        let (Some(bot_id), Some(bot_user_id)) =
            (channel.bot_id, channel.bot_platform_user_id.as_deref())
        else {
            continue;
        };
        let text = if channel.shared {
            format!("[{}] {text}", switcher.name)
        } else {
            text.clone()
        };
        let bot = reply::Bot {
            id: bot_id,
            platform_user_id: bot_user_id,
        };
        if let Err(error) = reply::send(
            state,
            channel.platform,
            &channel.platform_channel_id,
            &bot,
            &text,
            None,
        )
        .await
        {
            tracing::warn!(
                %error,
                %switcher_id,
                platform = channel.platform.label(),
                "Couldn't post a notification"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use live::{EventDetail, SwitchReason};

    use super::{Audience, notification};
    use crate::messages::Msg;

    fn switched(reason: SwitchReason) -> EventDetail {
        EventDetail::SceneSwitched {
            from: Some("LIVE".into()),
            to: "LOW".into(),
            reason,
            by: None,
            server: Some("BELABOX Cloud".into()),
            bitrate: Some(640),
            rtt: None,
            picked: false,
        }
    }

    /// A switch someone asked for: `picked` in the dashboard, or not (a chat
    /// command, a refresh).
    fn asked(picked: bool) -> EventDetail {
        EventDetail::SceneSwitched {
            from: Some("LIVE".into()),
            to: "Just Chatting".into(),
            reason: SwitchReason::Dashboard,
            by: Some("kate (Web)".into()),
            server: None,
            bitrate: None,
            rtt: None,
            picked,
        }
    }

    #[test]
    fn automatic_and_picked_switches_are_announced() {
        assert!(notification(&switched(SwitchReason::Auto)).is_some());
        assert!(notification(&asked(true)).is_some());
        // A command answered where it was asked, and a change made in OBS.
        assert!(notification(&asked(false)).is_none());
        assert!(notification(&switched(SwitchReason::Dashboard)).is_none());
        assert!(notification(&switched(SwitchReason::Obs)).is_none());
        assert!(notification(&EventDetail::ObsConnected).is_none());
    }

    #[test]
    fn a_picked_scene_is_said_where_alerts_are_on() {
        assert_eq!(
            notification(&asked(true)),
            Some((
                Msg::SwitchedTo("Just Chatting".into()),
                Audience::DashboardSwitches
            ))
        );
    }

    #[test]
    fn the_timeouts_are_said_everywhere() {
        for detail in [
            EventDetail::OfflineTimeout { minutes: 5 },
            EventDetail::EndingTimeout { minutes: 5 },
        ] {
            let (_, audience) = notification(&detail).expect("announced");
            assert_eq!(audience, Audience::Everyone);
        }
        let (_, audience) = notification(&switched(SwitchReason::Auto)).expect("announced");
        assert_eq!(audience, Audience::AutoSwitches);
    }
}
