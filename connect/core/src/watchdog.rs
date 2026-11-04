use std::collections::VecDeque;
use std::time::Duration;

use crate::relay::{PcObsState, WatchdogMode};

const RESTART_WINDOW: Duration = Duration::from_secs(10 * 60);
const MAX_RESTARTS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogAction {
    None,
    Restart { resume: bool },
    StopAndReport,
}

#[derive(Debug, Default)]
pub struct Watchdog {
    restarts: VecDeque<Duration>,
}

impl Watchdog {
    pub fn decide(
        &mut self,
        now: Duration,
        state: PcObsState,
        mode: WatchdogMode,
        stream_was_active: bool,
        deliberate_close: bool,
    ) -> WatchdogAction {
        if deliberate_close {
            return WatchdogAction::None;
        }

        let should_restart = match state {
            PcObsState::Crashed => mode != WatchdogMode::Off,
            PcObsState::NotResponding => mode != WatchdogMode::Off,
            _ => false,
        };
        if !should_restart {
            return WatchdogAction::None;
        }

        while self
            .restarts
            .front()
            .is_some_and(|started| now.saturating_sub(*started) >= RESTART_WINDOW)
        {
            self.restarts.pop_front();
        }
        if self.restarts.len() >= MAX_RESTARTS {
            return WatchdogAction::StopAndReport;
        }

        self.restarts.push_back(now);
        WatchdogAction::Restart {
            resume: mode == WatchdogMode::RestartResume && stream_was_active,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Watchdog, WatchdogAction};
    use crate::relay::{PcObsState, WatchdogMode};

    #[test]
    fn deliberate_closes_and_off_mode_do_not_restart() {
        let mut watchdog = Watchdog::default();
        assert_eq!(
            watchdog.decide(
                Duration::ZERO,
                PcObsState::Crashed,
                WatchdogMode::Restart,
                true,
                true,
            ),
            WatchdogAction::None
        );
        assert_eq!(
            watchdog.decide(
                Duration::ZERO,
                PcObsState::Crashed,
                WatchdogMode::Off,
                true,
                false,
            ),
            WatchdogAction::None
        );
    }

    #[test]
    fn restart_resume_only_resumes_an_active_stream() {
        let mut watchdog = Watchdog::default();
        assert_eq!(
            watchdog.decide(
                Duration::ZERO,
                PcObsState::Crashed,
                WatchdogMode::RestartResume,
                true,
                false,
            ),
            WatchdogAction::Restart { resume: true }
        );
        assert_eq!(
            watchdog.decide(
                Duration::from_secs(1),
                PcObsState::Crashed,
                WatchdogMode::RestartResume,
                false,
                false,
            ),
            WatchdogAction::Restart { resume: false }
        );
    }

    #[test]
    fn not_responding_uses_the_restart_mode_and_restart_cap() {
        let mut watchdog = Watchdog::default();
        for index in 0..3 {
            assert_eq!(
                watchdog.decide(
                    Duration::from_secs(index),
                    PcObsState::NotResponding,
                    WatchdogMode::Restart,
                    false,
                    false,
                ),
                WatchdogAction::Restart { resume: false }
            );
        }
        assert_eq!(
            watchdog.decide(
                Duration::from_secs(3),
                PcObsState::NotResponding,
                WatchdogMode::Restart,
                false,
                false,
            ),
            WatchdogAction::StopAndReport
        );
        assert_eq!(
            watchdog.decide(
                Duration::from_secs(10 * 60 + 3),
                PcObsState::Crashed,
                WatchdogMode::Restart,
                false,
                false,
            ),
            WatchdogAction::Restart { resume: false }
        );
    }
}
