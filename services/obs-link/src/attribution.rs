//! Who caused a change in OBS, for the activity feed.
//!
//! Lives on the connection rather than the switcher task, so it survives the
//! switcher restarting (e.g. after a settings change) between a dashboard
//! action and OBS reporting its result.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use live::{Intent, IntentAction};
use uuid::Uuid;

/// How long a dashboard action waits for OBS to confirm it.
const INTENT_WINDOW: Duration = Duration::from_secs(15);
/// OBS reports our own switches back to us; recognise them within this window.
const OWN_SWITCH_WINDOW: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct Attribution {
    intents: Vec<(Intent, Instant)>,
    own_switch: Option<(Uuid, Instant)>,
    /// The scene last known to be on air, as logged.
    scene: Option<String>,
    /// ... and its UUID, when OBS gave one.
    scene_uuid: Option<Uuid>,
}

pub type SharedAttribution = Arc<Mutex<Attribution>>;

/// What to log for a scene change reported by OBS.
#[derive(Debug, PartialEq, Eq)]
pub enum SceneChange {
    /// Same scene as before: nothing to log.
    Unchanged,
    /// Our own automatic switch coming back: already logged.
    Own,
    /// A change to log, from the previous scene (if known), by someone (if
    /// known), who `picked` it in the dashboard or not.
    Changed {
        from: Option<String>,
        by: Option<String>,
        picked: bool,
    },
}

impl Attribution {
    /// Remembers who asked for a change. Asking for the scene already on air
    /// changes nothing, so that intent is dropped: kept, it would be credited
    /// with a later return to the scene made in OBS (and said in chat).
    pub fn add_intent(&mut self, intent: Intent) {
        if let IntentAction::Scene { uuid } = intent.action
            && self.scene_uuid == Some(uuid)
        {
            return;
        }
        self.intents.push((intent, Instant::now()));
    }

    /// Who asked for `action` from the dashboard, if someone did recently.
    pub fn take_intent(&mut self, action: &IntentAction) -> Option<String> {
        self.take(action).map(|intent| intent.by)
    }

    fn take(&mut self, action: &IntentAction) -> Option<Intent> {
        self.intents.retain(|(_, at)| at.elapsed() < INTENT_WINDOW);
        let index = self
            .intents
            .iter()
            .position(|(intent, _)| intent.action == *action)?;
        Some(self.intents.remove(index).0)
    }

    /// The scene on air, as read from OBS (on start or after a reconnect). The
    /// name is kept from the first sighting, for the activity feed; the UUID
    /// always follows OBS, so a pick isn't dropped against a stale scene.
    pub fn scene_seen(&mut self, uuid: Option<Uuid>, name: &str) {
        if self.scene.is_none() {
            self.scene = Some(name.to_owned());
        }
        self.scene_uuid = uuid;
    }

    pub fn scene(&self) -> Option<String> {
        self.scene.clone()
    }

    /// Records a switch we made ourselves, before OBS reports it.
    pub fn own_switch(&mut self, uuid: Uuid, name: &str) {
        self.own_switch = Some((uuid, Instant::now()));
        self.scene = Some(name.to_owned());
        self.scene_uuid = Some(uuid);
    }

    /// Classifies a scene change reported by OBS and remembers the new scene.
    pub fn scene_changed(&mut self, uuid: Option<Uuid>, name: &str) -> SceneChange {
        if let Some((own, at)) = self.own_switch
            && Some(own) == uuid
            && at.elapsed() < OWN_SWITCH_WINDOW
        {
            self.own_switch = None;
            self.scene = Some(name.to_owned());
            self.scene_uuid = uuid;
            return SceneChange::Own;
        }

        let from = self.scene.replace(name.to_owned());
        let from_uuid = std::mem::replace(&mut self.scene_uuid, uuid);
        // Taken even when nothing changed: it has had its answer.
        let intent = uuid.and_then(|uuid| self.take(&IntentAction::Scene { uuid }));
        // The UUID follows OBS across reconnects; the name may be stale.
        let unchanged = match (from_uuid, uuid) {
            (Some(before), Some(now)) => before == now,
            _ => from.as_deref() == Some(name),
        };
        if unchanged {
            return SceneChange::Unchanged;
        }

        let picked = intent.as_ref().is_some_and(|intent| intent.picked);
        SceneChange::Changed {
            from,
            by: intent.map(|intent| intent.by),
            picked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Attribution, SceneChange};
    use live::{Intent, IntentAction};
    use uuid::Uuid;

    fn uuid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn a_pick_after_a_reconnect_uses_the_scene_obs_reports() {
        let mut attribution = Attribution::default();
        attribution.scene_seen(Some(uuid(1)), "LIVE");
        // OBS changed to BRB while disconnected, then reconnected.
        attribution.scene_seen(Some(uuid(2)), "BRB");
        attribution.add_intent(Intent {
            action: IntentAction::Scene { uuid: uuid(1) },
            by: "kate (Web)".into(),
            picked: true,
        });
        assert_eq!(
            attribution.scene_changed(Some(uuid(1)), "LIVE"),
            SceneChange::Changed {
                from: Some("LIVE".into()),
                by: Some("kate (Web)".into()),
                picked: true,
            }
        );
    }

    #[test]
    fn dashboard_switches_are_attributed_once() {
        let mut attribution = Attribution::default();
        attribution.scene_seen(None, "LOW");
        attribution.add_intent(Intent {
            action: IntentAction::Scene { uuid: uuid(1) },
            by: "streamer".into(),
            picked: true,
        });

        assert_eq!(
            attribution.scene_changed(Some(uuid(1)), "Just Chatting"),
            SceneChange::Changed {
                from: Some("LOW".into()),
                by: Some("streamer".into()),
                picked: true,
            }
        );
        // The intent is used up; a later change is from OBS.
        assert_eq!(
            attribution.scene_changed(Some(uuid(2)), "LIVE"),
            SceneChange::Changed {
                from: Some("Just Chatting".into()),
                by: None,
                picked: false,
            }
        );
    }

    #[test]
    fn command_switches_are_attributed_but_not_picked() {
        let mut attribution = Attribution::default();
        attribution.scene_seen(None, "LOW");
        attribution.add_intent(Intent {
            action: IntentAction::Scene { uuid: uuid(1) },
            by: "mod_kate (Twitch chat)".into(),
            picked: false,
        });

        assert_eq!(
            attribution.scene_changed(Some(uuid(1)), "LIVE"),
            SceneChange::Changed {
                from: Some("LOW".into()),
                by: Some("mod_kate (Twitch chat)".into()),
                picked: false,
            }
        );
    }

    #[test]
    fn picking_the_scene_on_air_is_not_credited_with_a_later_return() {
        let pick = || Intent {
            action: IntentAction::Scene { uuid: uuid(1) },
            by: "kate (Web)".into(),
            picked: true,
        };
        let from_obs = |from: &str| SceneChange::Changed {
            from: Some(from.into()),
            by: None,
            picked: false,
        };

        // LIVE is on air and OBS says nothing about a switch to it.
        let mut attribution = Attribution::default();
        attribution.scene_seen(Some(uuid(1)), "LIVE");
        attribution.add_intent(pick());
        assert_eq!(
            attribution.scene_changed(Some(uuid(2)), "BRB"),
            from_obs("LIVE")
        );
        assert_eq!(
            attribution.scene_changed(Some(uuid(1)), "LIVE"),
            from_obs("BRB")
        );

        // The scene's UUID wasn't known, and OBS repeats the scene.
        let mut attribution = Attribution::default();
        attribution.scene_seen(None, "LIVE");
        attribution.add_intent(pick());
        assert_eq!(
            attribution.scene_changed(Some(uuid(1)), "LIVE"),
            SceneChange::Unchanged
        );
        assert_eq!(
            attribution.scene_changed(Some(uuid(2)), "BRB"),
            from_obs("LIVE")
        );
        assert_eq!(
            attribution.scene_changed(Some(uuid(1)), "LIVE"),
            from_obs("BRB")
        );
    }

    #[test]
    fn repeats_and_own_switches_are_not_logged_again() {
        let mut attribution = Attribution::default();
        attribution.scene_seen(None, "LIVE");
        assert_eq!(
            attribution.scene_changed(Some(uuid(1)), "LIVE"),
            SceneChange::Unchanged
        );

        attribution.own_switch(uuid(2), "LOW");
        assert_eq!(
            attribution.scene_changed(Some(uuid(2)), "LOW"),
            SceneChange::Own
        );
        assert_eq!(attribution.scene(), Some("LOW".into()));
    }

    #[test]
    fn first_seen_scene_does_not_overwrite() {
        let mut attribution = Attribution::default();
        attribution.scene_seen(None, "LIVE");
        attribution.scene_seen(None, "BRB");
        assert_eq!(attribution.scene(), Some("LIVE".into()));
    }

    #[test]
    fn stream_intents_match_by_action() {
        let mut attribution = Attribution::default();
        attribution.add_intent(Intent {
            action: IntentAction::Stream { active: true },
            by: "mod_kate".into(),
            picked: false,
        });
        assert_eq!(
            attribution.take_intent(&IntentAction::Stream { active: false }),
            None
        );
        assert_eq!(
            attribution.take_intent(&IntentAction::Stream { active: true }),
            Some("mod_kate".into())
        );
    }
}
