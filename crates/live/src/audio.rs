//! OBS's audio levels and mute states, for the Live page's mixer.
//!
//! OBS only sends levels (`InputVolumeMeters`, every 50 ms) when asked to, so
//! obs-link turns them on only while someone watches: the API publishes on
//! [`audio_demand_subject`] every [`AUDIO_DEMAND_INTERVAL`] while a mixer is
//! open, and obs-link turns them off again [`AUDIO_DEMAND_TTL`] after the last
//! one. Levels go out on [`audio_subject`] at most every [`AUDIO_LEVELS_INTERVAL`]
//! (the loudest of the frames in between); mute and fader changes always do.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const AUDIO_DEMAND_INTERVAL: Duration = Duration::from_secs(5);
pub const AUDIO_DEMAND_TTL: Duration = Duration::from_secs(15);
pub const AUDIO_LEVELS_INTERVAL: Duration = Duration::from_millis(200);

/// Levels at or below this are silence (OBS's meter starts at -60).
pub const SILENCE_DB: f32 = -100.0;

/// [`AudioUpdate`]s from obs-link.
pub fn audio_subject(switcher_id: Uuid) -> String {
    format!("switcher.{switcher_id}.audio")
}

/// "Someone is watching the levels" from the API.
pub fn audio_demand_subject(switcher_id: Uuid) -> String {
    format!("switcher.{switcher_id}.audio.demand")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioUpdate {
    /// The inputs that made a sound since the last levels; the rest are silent.
    Levels {
        inputs: Vec<InputLevels>,
    },
    Muted {
        input: String,
        muted: bool,
    },
    Volume {
        input: String,
        volume_db: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InputLevels {
    pub input: String,
    /// One per audio channel (two for stereo).
    pub channels: Vec<ChannelLevel>,
}

/// dBFS (0 is full scale), after the fader, rounded to 0.1; [`SILENCE_DB`] at the least.
/// A muted input has only its own peak (before mute and fader), like OBS's greyed-out
/// meter, and no magnitude.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChannelLevel {
    /// The average loudness (a tick on the meter).
    pub magnitude: f32,
    /// What the bar shows.
    pub peak: f32,
}

impl ChannelLevel {
    /// From obs-websocket's multipliers (1.0 = 0 dBFS): magnitude, peak and the
    /// input's own peak. Muted inputs report 0 for the first two.
    pub fn from_mul(magnitude: f64, peak: f64, input_peak: f64) -> Self {
        let muted = peak <= 0.0 && magnitude <= 0.0;
        Self {
            magnitude: to_db(magnitude),
            peak: to_db(if muted { input_peak } else { peak }),
        }
    }

    pub fn is_silent(&self) -> bool {
        self.peak <= SILENCE_DB
    }

    /// The louder of both, per field.
    pub fn max(self, other: Self) -> Self {
        Self {
            magnitude: self.magnitude.max(other.magnitude),
            peak: self.peak.max(other.peak),
        }
    }
}

fn to_db(mul: f64) -> f32 {
    if !mul.is_finite() || mul <= 0.0 {
        return SILENCE_DB;
    }
    let db = (20.0 * mul.log10()).max(f64::from(SILENCE_DB));
    ((db * 10.0).round() / 10.0) as f32
}

#[cfg(test)]
mod tests {
    use super::{ChannelLevel, SILENCE_DB};

    #[test]
    fn multipliers_become_rounded_decibels() {
        let level = ChannelLevel::from_mul(0.5, 1.0, 1.0);
        assert_eq!(level.magnitude, -6.0);
        assert_eq!(level.peak, 0.0);
        assert!(ChannelLevel::from_mul(0.0, 0.0, 0.0).is_silent());
        assert_eq!(
            ChannelLevel::from_mul(1e-9, f64::NAN, 0.0).magnitude,
            SILENCE_DB
        );
        assert_eq!(
            serde_json::to_string(&ChannelLevel::from_mul(0.1, 0.25, 0.5)).unwrap(),
            r#"{"magnitude":-20.0,"peak":-12.0}"#
        );
    }

    #[test]
    fn muted_inputs_show_their_own_peak() {
        // As OBS reports a muted Desktop Audio.
        let level = ChannelLevel::from_mul(0.0, 0.0, 0.5);
        assert_eq!(level.peak, -6.0);
        assert_eq!(level.magnitude, SILENCE_DB);
        assert!(!level.is_silent());
    }
}
