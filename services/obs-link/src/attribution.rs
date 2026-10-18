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
}

pub type SharedAttribution = Arc<Mutex<Attribution>>;

/// What to log for a scene change reported by OBS.
#[derive(Debug, PartialEq, Eq)]
pub enum SceneChange {
    /// Same scene as before: nothing to log.
    Unchanged,
    /// Our own automatic switch coming back: already logged.
    Own,
    /// A change to log, from the previous scene (if known), by someone (if known).
    Changed {
        from: Option<String>,
        by: Option<String>,
    },
}

impl Attribution {
    pub fn add_intent(&mut self, intent: Intent) {
        self.intents.push((intent, Instant::now()));
    }

    /// Who asked for `action` from the dashboard, if someone did recently.
    pub fn take_intent(&mut self, action: &IntentAction) -> Option<String> {
        self.intents.retain(|(_, at)| at.elapsed() < INTENT_WINDOW);
        let index = self
            .intents
            .iter()
            .position(|(intent, _)| intent.action == *action)?;
        Some(self.intents.remove(index).0.by)
    }

    /// The scene on air, when first seen (e.g. read from OBS on start).
    pub fn scene_seen(&mut self, name: &str) {
        if self.scene.is_none() {
            self.scene = Some(name.to_owned());
        }
    }

    pub fn scene(&self) -> Option<String> {
        self.scene.clone()
    }

    /// Records a switch we made ourselves, before OBS reports it.
    pub fn own_switch(&mut self, uuid: Uuid, name: &str) {
        self.own_switch = Some((uuid, Instant::now()));
        self.scene = Some(name.to_owned());
    }

    /// Classifies a scene change reported by OBS and remembers the new scene.
    pub fn scene_changed(&mut self, uuid: Option<Uuid>, name: &str) -> SceneChange {
        if let Some((own, at)) = self.own_switch
            && Some(own) == uuid
            && at.elapsed() < OWN_SWITCH_WINDOW
        {
            self.own_switch = None;
            self.scene = Some(name.to_owned());
            return SceneChange::Own;
        }

        let from = self.scene.replace(name.to_owned());
        if from.as_deref() == Some(name) {
            return SceneChange::Unchanged;
        }

        let by = uuid.and_then(|uuid| self.take_intent(&IntentAction::Scene { uuid }));
        SceneChange::Changed { from, by }
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
    fn dashboard_switches_are_attributed_once() {
        let mut attribution = Attribution::default();
        attribution.scene_seen("LOW");
        attribution.add_intent(Intent {
            action: IntentAction::Scene { uuid: uuid(1) },
            by: "streamer".into(),
        });

        assert_eq!(
            attribution.scene_changed(Some(uuid(1)), "Just Chatting"),
            SceneChange::Changed {
                from: Some("LOW".into()),
                by: Some("streamer".into())
            }
        );
        // The intent is used up; a later change is from OBS.
        assert_eq!(
            attribution.scene_changed(Some(uuid(2)), "LIVE"),
            SceneChange::Changed {
                from: Some("Just Chatting".into()),
                by: None
            }
        );
    }

    #[test]
    fn repeats_and_own_switches_are_not_logged_again() {
        let mut attribution = Attribution::default();
        attribution.scene_seen("LIVE");
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
        attribution.scene_seen("LIVE");
        attribution.scene_seen("BRB");
        assert_eq!(attribution.scene(), Some("LIVE".into()));
    }

    #[test]
    fn stream_intents_match_by_action() {
        let mut attribution = Attribution::default();
        attribution.add_intent(Intent {
            action: IntentAction::Stream { active: true },
            by: "mod_kate".into(),
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
