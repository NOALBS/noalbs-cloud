//! Audio levels for the dashboard's mixer (see [`live::audio_subject`]).
//!
//! OBS sends levels only to sessions subscribed to `InputVolumeMeters`, 20 a
//! second. [`Meters`] decides when to ask for them (someone demanded them in
//! the last [`live::AUDIO_DEMAND_TTL`]) and folds the frames into one update per
//! [`live::AUDIO_LEVELS_INTERVAL`], keeping the loudest of each channel.

use std::time::{Duration, Instant};

use live::{AudioUpdate, ChannelLevel, InputLevels};
use obs_actions::types::{
    EVENT_SUBSCRIPTION_INPUT_VOLUME_METERS, EVENT_SUBSCRIPTION_INPUTS, EVENT_SUBSCRIPTION_OUTPUTS,
    EVENT_SUBSCRIPTION_SCENES, InputVolumeMeters,
};

/// An update is due this long after the last: a little early, so frames the browser
/// source already spaced [`live::AUDIO_LEVELS_INTERVAL`] apart (newer ones do) aren't
/// skipped for arriving a few ms early. Half an OBS frame (50 ms).
const DUE_AFTER: Duration = live::AUDIO_LEVELS_INTERVAL.saturating_sub(Duration::from_millis(25));

/// The events we use: scene changes, stream/record state, mute and fader. The
/// rest of OBS's default (media playback, filters, transitions, plugins' vendor
/// events, …) would only cost the streamer's upload.
const USED_EVENTS: u32 =
    EVENT_SUBSCRIPTION_SCENES | EVENT_SUBSCRIPTION_INPUTS | EVENT_SUBSCRIPTION_OUTPUTS;

/// The event subscriptions to ask OBS for.
pub fn subscriptions(levels: bool) -> u32 {
    if levels {
        USED_EVENTS | EVENT_SUBSCRIPTION_INPUT_VOLUME_METERS
    } else {
        USED_EVENTS
    }
}

#[derive(Debug, Default)]
pub struct Meters {
    wanted_until: Option<Instant>,
    /// OBS was asked for levels (and hasn't reconnected since).
    subscribed: bool,
    /// Inputs heard since the last update, in OBS's order.
    pending: Vec<InputLevels>,
    last_sent: Option<Instant>,
    /// The last update said "all silent": no need to repeat it.
    sent_silence: bool,
}

impl Meters {
    fn wanted(&self, now: Instant) -> bool {
        self.wanted_until.is_some_and(|until| now < until)
    }

    /// Someone watches. Returns whether OBS must now be asked for levels.
    pub fn demand(&mut self, now: Instant, obs_connected: bool) -> Option<bool> {
        self.wanted_until = Some(now + live::AUDIO_DEMAND_TTL);
        self.resubscribe(now, obs_connected)
    }

    /// Checked now and then: returns the subscription OBS must be switched to,
    /// if it changes (levels on while wanted, off once nobody watches).
    pub fn resubscribe(&mut self, now: Instant, obs_connected: bool) -> Option<bool> {
        let wanted = obs_connected && self.wanted(now);
        if wanted == self.subscribed {
            return None;
        }
        self.subscribed = wanted;
        if !wanted {
            self.pending.clear();
        }
        Some(wanted)
    }

    /// OBS (re)connected or went away: a new session starts without levels.
    pub fn obs_reconnected(&mut self) {
        self.subscribed = false;
        self.pending.clear();
    }

    /// A frame of levels. Returns an update when one is due.
    pub fn frame(&mut self, meters: &InputVolumeMeters, now: Instant) -> Option<AudioUpdate> {
        if !self.subscribed {
            return None;
        }
        for meter in &meters.inputs {
            let channels: Vec<ChannelLevel> = meter
                .input_levels_mul
                .iter()
                .map(|levels| {
                    let at = |i: usize| levels.get(i).copied().unwrap_or(0.0);
                    ChannelLevel::from_mul(at(0), at(1), at(2))
                })
                .collect();
            if channels.iter().all(ChannelLevel::is_silent) {
                continue;
            }
            match self
                .pending
                .iter_mut()
                .find(|pending| pending.input == meter.input_name)
            {
                Some(pending) => {
                    if pending.channels.len() < channels.len() {
                        pending.channels.resize(channels.len(), channels[0]);
                    }
                    for (into, level) in pending.channels.iter_mut().zip(channels) {
                        *into = into.max(level);
                    }
                }
                None => self.pending.push(InputLevels {
                    input: meter.input_name.clone(),
                    channels,
                }),
            }
        }

        let due = self
            .last_sent
            .is_none_or(|sent| now.duration_since(sent) >= DUE_AFTER);
        if !due || (self.pending.is_empty() && self.sent_silence) {
            return None;
        }
        self.last_sent = Some(now);
        self.sent_silence = self.pending.is_empty();
        Some(AudioUpdate::Levels {
            inputs: std::mem::take(&mut self.pending),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use live::{AudioUpdate, ChannelLevel};
    use obs_actions::types::{InputVolumeMeter, InputVolumeMeters};

    use super::Meters;

    fn frame(inputs: &[(&str, &[[f64; 3]])]) -> InputVolumeMeters {
        InputVolumeMeters {
            inputs: inputs
                .iter()
                .map(|(name, levels)| InputVolumeMeter {
                    input_name: (*name).to_owned(),
                    input_levels_mul: levels.iter().map(|l| l.to_vec()).collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn levels_are_asked_for_while_demanded_and_dropped_after() {
        let start = Instant::now();
        let mut meters = Meters::default();
        assert_eq!(meters.demand(start, false), None, "not without OBS");
        assert_eq!(meters.resubscribe(start, true), Some(true));
        assert_eq!(meters.demand(start + Duration::from_secs(5), true), None);
        let later = start + Duration::from_secs(5) + live::AUDIO_DEMAND_TTL;
        assert_eq!(
            meters.resubscribe(later - Duration::from_secs(1), true),
            None
        );
        assert_eq!(meters.resubscribe(later, true), Some(false));
    }

    #[test]
    fn a_reconnected_obs_is_asked_again() {
        let now = Instant::now();
        let mut meters = Meters::default();
        assert_eq!(meters.demand(now, true), Some(true));
        meters.obs_reconnected();
        assert_eq!(meters.resubscribe(now, true), Some(true));
    }

    #[test]
    fn frames_fold_into_one_update_per_interval_with_the_loudest_levels() {
        let interval = live::AUDIO_LEVELS_INTERVAL;
        let start = Instant::now();
        let mut meters = Meters::default();
        meters.demand(start, true);

        let first = meters.frame(&frame(&[("Mic", &[[0.5, 0.5, 0.5]])]), start);
        assert!(matches!(first, Some(AudioUpdate::Levels { .. })));

        let quiet = frame(&[("Mic", &[[0.1, 0.25, 0.25]]), ("Muted", &[[0.0, 0.0, 0.0]])]);
        assert_eq!(meters.frame(&quiet, start + interval / 2), None);
        let loud = frame(&[("Mic", &[[0.5, 0.1, 0.1]])]);
        let Some(AudioUpdate::Levels { inputs }) = meters.frame(&loud, start + interval) else {
            panic!("an update was due");
        };
        assert_eq!(inputs.len(), 1, "silent inputs are left out");
        assert_eq!(inputs[0].input, "Mic");
        assert_eq!(
            inputs[0].channels,
            [ChannelLevel {
                magnitude: -6.0,
                peak: -12.0
            }]
        );

        let silent = || frame(&[("Mic", &[[0.0, 0.0, 0.0]])]);
        assert_eq!(
            meters.frame(&silent(), start + interval * 2),
            Some(AudioUpdate::Levels { inputs: vec![] }),
            "silence is said once"
        );
        assert_eq!(meters.frame(&silent(), start + interval * 3), None);
    }
}
