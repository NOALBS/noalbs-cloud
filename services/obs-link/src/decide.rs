//! Which scene a switcher should be on, as a pure function of its settings
//! and what it currently sees. Kept free of I/O so every rule is unit-tested.
//!
//! Follows NOALBS's `switcher.rs`: the first server (by priority) with a feed
//! decides; a new feed state must hold for `retry_attempts` more checks before
//! switching, unless it's a recovery from offline with `instant_recover` or a
//! drop on a server with `instant_degrade`; while offline, the scenes of the
//! server last in use apply; and a server that has a publisher but no bitrate yet
//! ([`Reading::Previous`]) keeps the LIVE or LOW scene we were on. After
//! `offline_timeout` on a BRB scene with no feed, or `ending_timeout` on the
//! Ending scene, it stops the stream.
//!
//! On top of NOALBS, [`Rule`]s (server combinations) swap in their own scenes
//! while all of their servers have a feed; the first matching rule wins.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use live::FeedState;
use uuid::Uuid;

/// What a stream server last reported, if its stats are fresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Live,
    Low,
    Offline,
    /// A publisher is connected but the server has no bitrate for it yet (e.g.
    /// 0 kbps right after connecting): go back to, or stay on, the LIVE or LOW
    /// scene we were on. NOALBS's `SwitchType::Previous`.
    Previous,
}

/// A scene per automatic role. `None` means not set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoleScenes {
    pub live: Option<Uuid>,
    pub low: Option<Uuid>,
    pub offline: Option<Uuid>,
}

impl RoleScenes {
    fn get(&self, state: FeedState) -> Option<Uuid> {
        match state {
            FeedState::Live => self.live,
            FeedState::Low => self.low,
            FeedState::Offline => self.offline,
            FeedState::Idle => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Server {
    pub id: Uuid,
    /// Used instead of the switcher's scenes while this server is in use (and
    /// its offline scene while it was the last one in use).
    pub overrides: RoleScenes,
    /// Drop from LIVE or LOW to a worse state at once, without waiting for
    /// `retry_attempts` (while in use, or for BRB while any such server is
    /// enabled). NOALBS's `instant_degrade`, for servers that push their stats.
    pub instant_degrade: bool,
    /// Going back to LIVE always waits for `retry_attempts`, even from offline
    /// with `instant_recover`. NOALBS's `delay_normal_recovery`.
    pub delay_live_recovery: bool,
}

/// A server combination: while every one of `servers` has a feed (whatever
/// else does too), its scenes are used instead of the in-use server's. LIVE or
/// LOW is still the highest-priority server's feed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rule {
    pub id: Uuid,
    pub servers: Vec<Uuid>,
    pub scenes: RoleScenes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub auto_switch: bool,
    pub only_while_streaming: bool,
    /// Switch at once when the feed comes back from offline.
    pub instant_recover: bool,
    /// Further checks a new feed state must hold before switching (0 = at once).
    pub retry_attempts: u32,
    pub scenes: RoleScenes,
    pub starting: Option<Uuid>,
    /// Leave the Starting scene once the feed is up (for LIVE or LOW, not BRB).
    pub switch_to_live: bool,
    /// Enabled servers, highest priority first.
    pub servers: Vec<Server>,
    /// Enabled server combinations, first match wins.
    pub rules: Vec<Rule>,
    /// Stop the stream after this long on a BRB scene with no feed, while the
    /// switcher is in charge (NOALBS's offline timeout).
    pub offline_timeout: Option<Duration>,
    pub ending: Option<Uuid>,
    /// Stop the stream after this long on the Ending scene, whether or not the
    /// switcher is in charge (the streamer put it there).
    pub ending_timeout: Option<Duration>,
}

pub struct Observation<'a> {
    pub now: Instant,
    pub streaming: bool,
    pub recording: bool,
    pub current_scene: Option<Uuid>,
    /// Fresh readings by server; stale or missing servers are left out.
    pub readings: &'a HashMap<Uuid, Reading>,
}

/// The feed as the switcher sees it: the first usable server's reading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Feed {
    Live,
    Low,
    #[default]
    Offline,
    Previous,
}

/// What the switcher remembers between checks.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Memory {
    /// The feed at the last check (offline to begin with, like NOALBS).
    last_feed: Feed,
    /// Further checks in a row it has held.
    same: u32,
    /// The server last in use; its offline scene applies while all are offline.
    last_server: Option<Uuid>,
    /// The rule matched at the last check: a change must hold like a new feed.
    seen_rule: Option<Uuid>,
    /// The rule last in use; its offline scene applies before the server's.
    last_rule: Option<Uuid>,
    /// The LIVE or LOW scene last aimed for, for [`Reading::Previous`].
    previous_scene: Option<Uuid>,
    /// Since when the stream has been on a BRB scene with no feed.
    offline_since: Option<Instant>,
    /// Since when the stream has been on the Ending scene.
    ending_since: Option<Instant>,
    deadline: Option<(Instant, chrono::DateTime<chrono::Utc>)>,
    /// A scene the previous run (before a config change) owned, and its role
    /// then, while it stays on air. Lets the switcher leave e.g. the scene of a
    /// combination that was just turned off.
    handed_over: Option<(Uuid, FeedState)>,
}

impl Memory {
    pub fn stops_at(&mut self, config: &Config) -> Option<chrono::DateTime<chrono::Utc>> {
        let deadline = self
            .offline_since
            .zip(config.offline_timeout)
            .into_iter()
            .chain(self.ending_since.zip(config.ending_timeout))
            .map(|(since, timeout)| since + timeout)
            .min();
        match deadline {
            Some(deadline) => {
                if self.deadline.is_none_or(|(instant, _)| instant != deadline) {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    self.deadline = Some((
                        deadline,
                        chrono::Utc::now() + chrono::Duration::from_std(remaining).unwrap(),
                    ));
                }
                self.deadline.map(|(_, wall)| wall)
            }
            None => {
                self.deadline = None;
                None
            }
        }
    }

    /// Memory for a run that starts while `scene` (playing `role`) is on air.
    pub fn handed_over(scene: Uuid, role: FeedState) -> Self {
        Self {
            handed_over: Some((scene, role)),
            ..Self::default()
        }
    }
}

/// Why the stream is to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    OfflineTimeout,
    EndingTimeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// What to show: the feed state while streaming, `Idle` otherwise.
    pub state: FeedState,
    /// The server whose feed is being used.
    pub active_server: Option<Uuid>,
    /// The server combination whose scenes are on air: the one last switched
    /// for, while OBS shows one of its scenes. Not one that only matches (e.g.
    /// while holding, or on a scene of the streamer's own).
    pub active_rule: Option<Uuid>,
    /// Switch to this scene now.
    pub switch_to: Option<Uuid>,
    /// A timeout ran out: stop the stream.
    pub stop: Option<Stop>,
}

/// Counts how long `holds` has been true in a row (from `since`): true once it
/// reaches `timeout`, which starts it over, so a refused stop is retried after
/// another full timeout rather than every second. Off or not holding starts it
/// over too.
fn timed_out(
    timeout: Option<Duration>,
    holds: bool,
    since: &mut Option<Instant>,
    now: Instant,
) -> bool {
    match timeout.filter(|_| holds) {
        Some(timeout) => {
            let start = *since.get_or_insert(now);
            let out = now.saturating_duration_since(start) >= timeout;
            if out {
                *since = None;
            }
            out
        }
        None => {
            *since = None;
            false
        }
    }
}

/// The feed from the highest-priority server that has one: a server with a
/// publisher but no bitrate yet stops the search too, like in NOALBS.
pub fn evaluate(config: &Config, readings: &HashMap<Uuid, Reading>) -> (Feed, Option<Uuid>) {
    for server in &config.servers {
        let feed = match readings.get(&server.id) {
            Some(Reading::Live) => Feed::Live,
            Some(Reading::Low) => Feed::Low,
            Some(Reading::Previous) => Feed::Previous,
            Some(Reading::Offline) | None => continue,
        };
        return (feed, Some(server.id));
    }
    (Feed::Offline, None)
}

/// The first rule whose servers all have a feed.
pub fn matching_rule<'a>(
    config: &'a Config,
    readings: &HashMap<Uuid, Reading>,
) -> Option<&'a Rule> {
    config.rules.iter().find(|rule| {
        !rule.servers.is_empty()
            && rule.servers.iter().all(|server| {
                matches!(
                    readings.get(server),
                    Some(Reading::Live | Reading::Low | Reading::Previous)
                )
            })
    })
}

/// The rule's scene for `state`, else the server's override, else the
/// switcher's.
fn scene_for(
    config: &Config,
    state: FeedState,
    rule: Option<Uuid>,
    server: Option<Uuid>,
) -> Option<Uuid> {
    let rule = rule
        .and_then(|id| config.rules.iter().find(|r| r.id == id))
        .map(|r| r.scenes)
        .unwrap_or_default();
    let overrides = server
        .and_then(|id| config.servers.iter().find(|s| s.id == id))
        .map(|s| s.overrides)
        .unwrap_or_default();
    rule.get(state)
        .or(overrides.get(state))
        .or(config.scenes.get(state))
}

/// Which automatic role a scene plays, if any.
fn role_of(config: &Config, scene: Uuid) -> Option<FeedState> {
    [FeedState::Live, FeedState::Low, FeedState::Offline]
        .into_iter()
        .find(|&state| {
            config.scenes.get(state) == Some(scene)
                || config
                    .servers
                    .iter()
                    .any(|server| server.overrides.get(state) == Some(scene))
                || config
                    .rules
                    .iter()
                    .any(|rule| rule.scenes.get(state) == Some(scene))
        })
}

/// Which automatic role a scene plays, counting the scene handed over by the
/// previous run.
pub fn owned_role(config: &Config, memory: &Memory, scene: Uuid) -> Option<FeedState> {
    role_of(config, scene).or(memory
        .handed_over
        .filter(|&(handed, _)| handed == scene)
        .map(|(_, role)| role))
}

/// Scenes the switcher may leave: the role scenes (overrides included), the
/// scene handed over, and Starting when `switch_to_live` is on. Never e.g.
/// "Just Chatting".
fn switchable(config: &Config, memory: &Memory, scene: Uuid) -> bool {
    owned_role(config, memory, scene).is_some()
        || (config.switch_to_live && config.starting == Some(scene))
}

/// Decides whether to switch now, updating `memory` for the next check.
pub fn decide(config: &Config, observation: &Observation, memory: &mut Memory) -> Outcome {
    let mut outcome = decide_switch(config, observation, memory);
    outcome.active_rule = applied_rule(config, memory, observation.current_scene);
    outcome
}

/// The rule last switched for, while OBS shows one of its LIVE, LOW or BRB
/// scenes (blank ones falling back as when switching).
fn applied_rule(config: &Config, memory: &Memory, current: Option<Uuid>) -> Option<Uuid> {
    let rule = memory.last_rule?;
    let current = current?;
    if !config.rules.iter().any(|candidate| candidate.id == rule) {
        return None;
    }
    [FeedState::Live, FeedState::Low, FeedState::Offline]
        .into_iter()
        .any(|state| scene_for(config, state, Some(rule), memory.last_server) == Some(current))
        .then_some(rule)
}

/// Everything but `Outcome::active_rule`, which depends on the memory after it.
fn decide_switch(config: &Config, observation: &Observation, memory: &mut Memory) -> Outcome {
    let (mut feed, server) = evaluate(config, observation.readings);
    let rule = matching_rule(config, observation.readings).map(|rule| rule.id);
    let current = observation.current_scene;

    // The handed-over scene only counts until OBS leaves it.
    if memory
        .handed_over
        .is_some_and(|(scene, _)| Some(scene) != current)
    {
        memory.handed_over = None;
    }

    // After a restart, holding goes back to the LIVE or LOW scene on air.
    if memory.previous_scene.is_none()
        && let Some(scene) = current
        && matches!(
            owned_role(config, memory, scene),
            Some(FeedState::Live | FeedState::Low)
        )
    {
        memory.previous_scene = Some(scene);
    }

    let shown = match feed {
        Feed::Live => FeedState::Live,
        Feed::Low => FeedState::Low,
        Feed::Offline => FeedState::Offline,
        Feed::Previous => match memory
            .previous_scene
            .and_then(|scene| owned_role(config, memory, scene))
        {
            Some(FeedState::Low) => FeedState::Low,
            _ => FeedState::Live,
        },
    };
    let mut outcome = Outcome {
        state: if observation.streaming || observation.recording {
            shown
        } else {
            FeedState::Idle
        },
        active_server: server,
        active_rule: None,
        switch_to: None,
        stop: None,
    };

    // Like NOALBS, the switcher rests (and counts nothing) while it may not
    // switch, or while a scene it doesn't own is on air.
    let allowed = config.auto_switch && (observation.streaming || !config.only_while_streaming);

    // The offline timeout counts while streaming on a BRB scene with no feed;
    // anything else (the feed back, another scene, auto switch off) starts it
    // over. The ending timeout counts while streaming on the Ending scene,
    // whatever the switcher may do.
    let dead_air = observation.streaming
        && allowed
        && feed == Feed::Offline
        && current
            .is_some_and(|scene| owned_role(config, memory, scene) == Some(FeedState::Offline));
    let ending = observation.streaming && current.is_some() && current == config.ending;
    let now = observation.now;
    if timed_out(
        config.offline_timeout,
        dead_air,
        &mut memory.offline_since,
        now,
    ) {
        outcome.stop = Some(Stop::OfflineTimeout);
    }
    if timed_out(config.ending_timeout, ending, &mut memory.ending_since, now) {
        outcome.stop = Some(Stop::EndingTimeout);
    }

    let Some(current) = current.filter(|&scene| allowed && switchable(config, memory, scene))
    else {
        return outcome;
    };

    let in_use = server.and_then(|id| config.servers.iter().find(|s| s.id == id));
    let recover =
        config.instant_recover && memory.last_feed == Feed::Offline && feed != Feed::Offline;
    let degrade = matches!(memory.last_feed, Feed::Live | Feed::Low)
        && matches!(feed, Feed::Low | Feed::Offline)
        && feed != memory.last_feed
        && (in_use.is_some_and(|s| s.instant_degrade)
            || (feed == Feed::Offline && config.servers.iter().any(|s| s.instant_degrade)));
    let delay_live = feed == Feed::Live
        && memory.last_feed != Feed::Live
        && in_use.is_some_and(|s| s.delay_live_recovery);
    let mut force = (recover || degrade) && !delay_live;
    // Moving onto or off a rule holds like a feed change, so a flapping second
    // uplink doesn't flip the layout back and forth.
    if feed == memory.last_feed && rule == memory.seen_rule {
        memory.same += 1;
    } else {
        memory.last_feed = feed;
        memory.seen_rule = rule;
        memory.same = 0;
    }
    // Holding on another server than the one last in use: that server is
    // back, so go LIVE with it.
    if feed == Feed::Previous && memory.last_server.is_some() && server != memory.last_server {
        feed = Feed::Live;
        force = true;
    }
    if memory.same < config.retry_attempts && !force {
        return outcome;
    }
    memory.same = 0;

    let target = match feed {
        Feed::Live => scene_for(config, FeedState::Live, rule, server),
        Feed::Low => scene_for(config, FeedState::Low, rule, server),
        Feed::Offline => scene_for(
            config,
            FeedState::Offline,
            memory.last_rule,
            memory.last_server,
        ),
        Feed::Previous => memory.previous_scene,
    };
    if matches!(feed, Feed::Live | Feed::Low) && target.is_some() {
        memory.previous_scene = target;
    }
    if feed != Feed::Offline {
        memory.last_server = server;
        memory.last_rule = rule;
    }

    let leaving_starting_for_brb = feed == Feed::Offline && config.starting == Some(current);
    if let Some(target) = target
        && target != current
        && !leaving_starting_for_brb
    {
        outcome.switch_to = Some(target);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    const LIVE: u128 = 1;
    const LOW: u128 = 2;
    const BRB: u128 = 3;
    const STARTING: u128 = 4;
    const CHATTING: u128 = 5;
    const HOME_LIVE: u128 = 6;
    const HOME_BRB: u128 = 7;
    const ENDING: u128 = 8;
    const BELABOX: u128 = 100;
    const HOME: u128 = 101;

    fn config() -> Config {
        Config {
            auto_switch: true,
            only_while_streaming: true,
            instant_recover: true,
            retry_attempts: 3,
            scenes: RoleScenes {
                live: Some(id(LIVE)),
                low: Some(id(LOW)),
                offline: Some(id(BRB)),
            },
            starting: Some(id(STARTING)),
            switch_to_live: true,
            servers: vec![
                Server {
                    id: id(BELABOX),
                    ..Server::default()
                },
                Server {
                    id: id(HOME),
                    overrides: RoleScenes {
                        live: Some(id(HOME_LIVE)),
                        offline: Some(id(HOME_BRB)),
                        ..RoleScenes::default()
                    },
                    ..Server::default()
                },
            ],
            rules: Vec::new(),
            offline_timeout: None,
            ending: Some(id(ENDING)),
            ending_timeout: None,
        }
    }

    fn readings(entries: &[(u128, Reading)]) -> HashMap<Uuid, Reading> {
        entries
            .iter()
            .map(|(server, r)| (id(*server), *r))
            .collect()
    }

    /// Runs `checks` decisions in a row on `scene` and returns the last outcome.
    fn run(
        config: &Config,
        scene: u128,
        streaming: bool,
        readings: &HashMap<Uuid, Reading>,
        memory: &mut Memory,
        checks: u32,
    ) -> Outcome {
        let observation = Observation {
            now: Instant::now(),
            streaming,
            recording: false,
            current_scene: Some(id(scene)),
            readings,
        };
        let mut outcome = decide(config, &observation, memory);
        for _ in 1..checks {
            outcome = decide(config, &observation, memory);
        }
        outcome
    }

    /// Memory after a while on `scene` with these readings.
    fn settled(config: &Config, scene: u128, readings: &HashMap<Uuid, Reading>) -> Memory {
        let mut memory = Memory::default();
        run(config, scene, true, readings, &mut memory, 8);
        memory
    }

    /// The check (1-based) at which the switch comes, within 10.
    fn switches_at(
        config: &Config,
        scene: u128,
        readings: &HashMap<Uuid, Reading>,
        memory: &mut Memory,
    ) -> Option<(u32, Uuid)> {
        (1..=10).find_map(|check| {
            run(config, scene, true, readings, memory, 1)
                .switch_to
                .map(|target| (check, target))
        })
    }

    #[test]
    fn getting_worse_holds_for_retry_attempts_more_checks() {
        let config = config();
        let mut memory = settled(&config, LIVE, &readings(&[(BELABOX, Reading::Live)]));
        let low = readings(&[(BELABOX, Reading::Low)]);
        // retry_attempts = 3: the first LOW check plus 3 more, like NOALBS.
        assert_eq!(
            switches_at(&config, LIVE, &low, &mut memory),
            Some((4, id(LOW)))
        );
        assert_eq!(
            run(&config, LOW, true, &low, &mut memory, 1).state,
            FeedState::Low
        );
    }

    #[test]
    fn a_flapping_feed_restarts_the_count() {
        let config = config();
        let mut memory = settled(&config, LIVE, &readings(&[(BELABOX, Reading::Live)]));
        let low = readings(&[(BELABOX, Reading::Low)]);
        let offline = readings(&[]);

        run(&config, LIVE, true, &low, &mut memory, 3);
        assert_eq!(
            switches_at(&config, LIVE, &offline, &mut memory),
            Some((4, id(BRB)))
        );
    }

    #[test]
    fn only_a_recovery_from_offline_is_instant() {
        let mut config = config();
        let live = readings(&[(BELABOX, Reading::Live)]);

        let mut memory = settled(&config, BRB, &readings(&[]));
        assert_eq!(
            switches_at(&config, BRB, &live, &mut memory),
            Some((1, id(LIVE)))
        );

        // LOW → LIVE waits, like NOALBS.
        let mut memory = settled(&config, LOW, &readings(&[(BELABOX, Reading::Low)]));
        assert_eq!(
            switches_at(&config, LOW, &live, &mut memory),
            Some((4, id(LIVE)))
        );

        config.instant_recover = false;
        let mut memory = settled(&config, BRB, &readings(&[]));
        assert_eq!(
            switches_at(&config, BRB, &live, &mut memory),
            Some((4, id(LIVE)))
        );
    }

    /// BELABOX pushes its stats, like IRLHosting WebSocket.
    fn pushed(mut config: Config) -> Config {
        config.servers[0].instant_degrade = true;
        config.servers[0].delay_live_recovery = true;
        config
    }

    #[test]
    fn pushed_stats_drop_at_once() {
        let config = pushed(config());
        let live = readings(&[(BELABOX, Reading::Live)]);
        let low = readings(&[(BELABOX, Reading::Low)]);

        let mut memory = settled(&config, LIVE, &live);
        assert_eq!(
            switches_at(&config, LIVE, &low, &mut memory),
            Some((1, id(LOW)))
        );
        // Then LOW → BRB, at once too.
        assert_eq!(
            switches_at(&config, LOW, &readings(&[]), &mut memory),
            Some((1, id(BRB)))
        );

        // BRB at once even while another server was in use.
        let home = readings(&[(HOME, Reading::Live)]);
        let mut memory = settled(&config, HOME_LIVE, &home);
        assert_eq!(
            switches_at(&config, HOME_LIVE, &readings(&[]), &mut memory),
            Some((1, id(HOME_BRB)))
        );
        // But that server's LOW waits.
        let mut config = config;
        config.servers[1].overrides.low = Some(id(LOW));
        let mut memory = settled(&config, HOME_LIVE, &home);
        assert_eq!(
            switches_at(
                &config,
                HOME_LIVE,
                &readings(&[(HOME, Reading::Low)]),
                &mut memory
            ),
            Some((4, id(LOW)))
        );
    }

    #[test]
    fn pushed_stats_wait_to_go_live_even_from_offline() {
        let config = pushed(config());
        let live = readings(&[(BELABOX, Reading::Live)]);

        let mut memory = settled(&config, BRB, &readings(&[]));
        assert_eq!(
            switches_at(&config, BRB, &live, &mut memory),
            Some((4, id(LIVE)))
        );
        // Back to LOW from offline is still instant.
        let mut memory = settled(&config, BRB, &readings(&[]));
        assert_eq!(
            switches_at(
                &config,
                BRB,
                &readings(&[(BELABOX, Reading::Low)]),
                &mut memory
            ),
            Some((1, id(LOW)))
        );
    }

    #[test]
    fn zero_retry_attempts_switches_at_once() {
        let mut config = config();
        config.retry_attempts = 0;
        let mut memory = settled(&config, LIVE, &readings(&[(BELABOX, Reading::Live)]));
        let low = readings(&[(BELABOX, Reading::Low)]);
        assert_eq!(
            switches_at(&config, LIVE, &low, &mut memory),
            Some((1, id(LOW)))
        );
    }

    #[test]
    fn uses_the_first_server_with_a_feed_in_priority_order() {
        let config = config();
        // BELABOX is low and first: it wins over the healthy second server.
        let both = readings(&[(BELABOX, Reading::Low), (HOME, Reading::Live)]);
        assert_eq!(evaluate(&config, &both), (Feed::Low, Some(id(BELABOX))));

        // An offline first server is skipped…
        let fallback = readings(&[(BELABOX, Reading::Offline), (HOME, Reading::Live)]);
        assert_eq!(evaluate(&config, &fallback), (Feed::Live, Some(id(HOME))));
        // …but one with a publisher and no bitrate yet isn't.
        let holding = readings(&[(BELABOX, Reading::Previous), (HOME, Reading::Live)]);
        assert_eq!(
            evaluate(&config, &holding),
            (Feed::Previous, Some(id(BELABOX)))
        );
    }

    #[test]
    fn per_server_overrides_apply_and_offline_uses_the_last_server() {
        let config = config();
        let home = readings(&[(HOME, Reading::Live)]);
        let mut memory = settled(&config, BRB, &readings(&[]));
        assert_eq!(
            switches_at(&config, BRB, &home, &mut memory),
            Some((1, id(HOME_LIVE)))
        );

        // Everything goes offline: HOME was in use, so its BRB scene.
        let mut memory = settled(&config, HOME_LIVE, &home);
        assert_eq!(
            switches_at(&config, HOME_LIVE, &readings(&[]), &mut memory),
            Some((4, id(HOME_BRB)))
        );
    }

    #[test]
    fn no_bitrate_yet_keeps_or_restores_the_scene_we_were_on() {
        let config = config();
        let holding = readings(&[(BELABOX, Reading::Previous)]);

        // On LIVE: stays, and shows as LIVE.
        let mut memory = settled(&config, LIVE, &readings(&[(BELABOX, Reading::Live)]));
        assert_eq!(switches_at(&config, LIVE, &holding, &mut memory), None);
        assert_eq!(
            run(&config, LIVE, true, &holding, &mut memory, 1).state,
            FeedState::Live
        );

        // Dropped to BRB from LOW: the publisher is back, so LOW again at once.
        let mut memory = settled(&config, LOW, &readings(&[(BELABOX, Reading::Low)]));
        run(&config, LOW, true, &readings(&[]), &mut memory, 4);
        run(&config, BRB, true, &readings(&[]), &mut memory, 4);
        assert_eq!(
            switches_at(&config, BRB, &holding, &mut memory),
            Some((1, id(LOW)))
        );

        // Nothing to go back to (e.g. started on BRB): stays.
        let mut memory = Memory::default();
        assert_eq!(switches_at(&config, BRB, &holding, &mut memory), None);
    }

    #[test]
    fn no_bitrate_yet_on_another_server_goes_live_with_it() {
        let config = config();
        let mut memory = settled(&config, LIVE, &readings(&[(BELABOX, Reading::Live)]));
        let home_holding = readings(&[(BELABOX, Reading::Offline), (HOME, Reading::Previous)]);
        assert_eq!(
            switches_at(&config, LIVE, &home_holding, &mut memory),
            Some((1, id(HOME_LIVE)))
        );
    }

    #[test]
    fn never_leaves_scenes_it_does_not_own() {
        let config = config();
        let mut memory = Memory::default();
        assert_eq!(
            switches_at(&config, CHATTING, &readings(&[]), &mut memory),
            None
        );
        assert_eq!(
            run(&config, CHATTING, true, &readings(&[]), &mut memory, 1).state,
            FeedState::Offline
        );
    }

    #[test]
    fn leaves_starting_for_live_or_low_when_enabled_never_for_brb() {
        let mut config = config();
        let live = readings(&[(BELABOX, Reading::Live)]);
        let low = readings(&[(BELABOX, Reading::Low)]);
        let target = |config: &Config, readings: &HashMap<Uuid, Reading>| {
            switches_at(config, STARTING, readings, &mut Memory::default()).map(|(_, t)| t)
        };

        assert_eq!(target(&config, &live), Some(id(LIVE)));
        assert_eq!(target(&config, &low), Some(id(LOW)));
        assert_eq!(target(&config, &readings(&[])), None);

        config.switch_to_live = false;
        assert_eq!(target(&config, &live), None);
    }

    #[test]
    fn respects_auto_switch_and_only_while_streaming() {
        let mut config = config();
        let offline = readings(&[]);

        let outcome = run(&config, LIVE, false, &offline, &mut Memory::default(), 10);
        assert_eq!((outcome.state, outcome.switch_to), (FeedState::Idle, None));

        config.only_while_streaming = false;
        let mut memory = Memory::default();
        let switched = (0..10).any(|_| {
            run(&config, LIVE, false, &offline, &mut memory, 1)
                .switch_to
                .is_some()
        });
        assert!(switched);

        config.auto_switch = false;
        let mut memory = Memory::default();
        assert_eq!(switches_at(&config, LIVE, &offline, &mut memory), None);
        assert_eq!(
            run(&config, LIVE, true, &offline, &mut memory, 1).state,
            FeedState::Offline
        );
    }

    #[test]
    fn an_unset_scene_means_do_not_switch() {
        let mut config = config();
        config.scenes.low = None;
        let mut memory = settled(&config, LIVE, &readings(&[(BELABOX, Reading::Live)]));
        let low = readings(&[(BELABOX, Reading::Low)]);
        assert_eq!(switches_at(&config, LIVE, &low, &mut memory), None);
        assert_eq!(
            run(&config, LIVE, true, &low, &mut memory, 1).state,
            FeedState::Low
        );
    }

    /// One check per second from `start` for `seconds`: the seconds (from 0)
    /// at which the stream is to stop.
    fn stops(
        config: &Config,
        scene: u128,
        readings: &HashMap<Uuid, Reading>,
        memory: &mut Memory,
        start: Instant,
        seconds: u64,
    ) -> Vec<u64> {
        (0..seconds)
            .filter(|second| {
                let observation = Observation {
                    now: start + Duration::from_secs(*second),
                    streaming: true,
                    recording: false,
                    current_scene: Some(id(scene)),
                    readings,
                };
                decide(config, &observation, memory).stop.is_some()
            })
            .collect()
    }

    fn with_timeout(minutes: u64) -> Config {
        Config {
            offline_timeout: Some(Duration::from_secs(minutes * 60)),
            ..config()
        }
    }

    #[test]
    fn deadlines_are_stable_and_clear_with_the_countdowns() {
        let config = Config {
            ending_timeout: Some(Duration::from_secs(120)),
            ..with_timeout(1)
        };
        let now = Instant::now();
        let mut memory = Memory::default();
        assert_eq!(memory.stops_at(&config), None);
        memory.offline_since = Some(now);
        let deadline = memory.stops_at(&config).unwrap();
        assert_eq!(memory.stops_at(&config), Some(deadline));
        memory.ending_since = Some(now);
        assert_eq!(memory.stops_at(&config), Some(deadline));
        memory.offline_since = None;
        let ending = memory.stops_at(&config).unwrap();
        assert!(ending > deadline);
        assert_eq!(memory.stops_at(&config), Some(ending));
        memory.ending_since = None;
        assert_eq!(memory.stops_at(&config), None);
    }

    #[test]
    fn the_offline_timeout_stops_the_stream_after_that_long_on_brb() {
        let config = with_timeout(1);
        let start = Instant::now();
        let mut memory = Memory::default();
        // Then again only after another full minute, should the stop fail.
        assert_eq!(
            stops(&config, BRB, &readings(&[]), &mut memory, start, 150),
            vec![60, 121]
        );
        // A server's own BRB scene counts too.
        assert_eq!(
            stops(
                &config,
                HOME_BRB,
                &readings(&[]),
                &mut Memory::default(),
                start,
                61
            ),
            vec![60]
        );
        // Off by default.
        assert!(
            stops(
                &Config {
                    offline_timeout: None,
                    ..config.clone()
                },
                BRB,
                &readings(&[]),
                &mut Memory::default(),
                start,
                300
            )
            .is_empty()
        );
    }

    #[test]
    fn the_offline_timeout_starts_over_when_the_feed_returns() {
        let config = with_timeout(1);
        let start = Instant::now();
        let mut memory = Memory::default();
        let offline = readings(&[]);
        assert!(stops(&config, BRB, &offline, &mut memory, start, 50).is_empty());
        // Back for a moment (still on BRB, waiting for the retries).
        let later = start + Duration::from_secs(50);
        let low = readings(&[(BELABOX, Reading::Low)]);
        assert!(stops(&config, BRB, &low, &mut memory, later, 1).is_empty());
        let later = later + Duration::from_secs(1);
        assert_eq!(
            stops(&config, BRB, &offline, &mut memory, later, 61),
            vec![60]
        );
    }

    #[test]
    fn the_offline_timeout_only_counts_on_brb_while_in_charge() {
        let config = with_timeout(1);
        let start = Instant::now();
        let offline = readings(&[]);

        // Another scene (the streamer took over), or Starting.
        for scene in [CHATTING, STARTING, LIVE] {
            assert!(stops(&config, scene, &offline, &mut Memory::default(), start, 120).is_empty());
        }

        // Auto switch off.
        let off = Config {
            auto_switch: false,
            ..config.clone()
        };
        assert!(stops(&off, BRB, &offline, &mut Memory::default(), start, 120).is_empty());

        // Not streaming, even when switching then.
        let idle = Config {
            only_while_streaming: false,
            ..config.clone()
        };
        let mut memory = Memory::default();
        let stopped = (0..120).any(|second| {
            let observation = Observation {
                now: start + Duration::from_secs(second),
                streaming: false,
                recording: false,
                current_scene: Some(id(BRB)),
                readings: &offline,
            };
            decide(&idle, &observation, &mut memory).stop.is_some()
        });
        assert!(!stopped);

        // Leaving BRB for a while starts it over.
        let mut memory = Memory::default();
        assert!(stops(&config, BRB, &offline, &mut memory, start, 50).is_empty());
        let later = start + Duration::from_secs(50);
        assert!(stops(&config, CHATTING, &offline, &mut memory, later, 5).is_empty());
        let later = later + Duration::from_secs(5);
        assert_eq!(
            stops(&config, BRB, &offline, &mut memory, later, 61),
            vec![60]
        );
    }

    fn with_ending_timeout(minutes: u64) -> Config {
        Config {
            ending_timeout: Some(Duration::from_secs(minutes * 60)),
            ..config()
        }
    }

    #[test]
    fn the_ending_timeout_stops_the_stream_after_that_long_on_ending() {
        let config = with_ending_timeout(1);
        let start = Instant::now();
        // With or without a feed; again after another full minute, should the
        // stop fail.
        for feed in [readings(&[]), readings(&[(BELABOX, Reading::Live)])] {
            assert_eq!(
                stops(&config, ENDING, &feed, &mut Memory::default(), start, 150),
                vec![60, 121]
            );
        }
        // Even with auto switch off: the streamer put the scene on.
        let off = Config {
            auto_switch: false,
            ..config.clone()
        };
        assert_eq!(
            stops(
                &off,
                ENDING,
                &readings(&[]),
                &mut Memory::default(),
                start,
                61
            ),
            vec![60]
        );
        // Said as such.
        let observation = Observation {
            now: start,
            streaming: true,
            recording: false,
            current_scene: Some(id(ENDING)),
            readings: &readings(&[]),
        };
        let mut memory = Memory::default();
        decide(&config, &observation, &mut memory);
        let observation = Observation {
            now: start + Duration::from_secs(60),
            ..observation
        };
        assert_eq!(
            decide(&config, &observation, &mut memory).stop,
            Some(Stop::EndingTimeout)
        );
        // Off by default, and without an Ending scene.
        for config in [
            self::config(),
            Config {
                ending: None,
                ..config.clone()
            },
        ] {
            assert!(
                stops(
                    &config,
                    ENDING,
                    &readings(&[]),
                    &mut Memory::default(),
                    start,
                    300
                )
                .is_empty()
            );
        }
    }

    #[test]
    fn the_ending_timeout_only_counts_on_ending_while_streaming() {
        let config = with_ending_timeout(1);
        let start = Instant::now();
        let offline = readings(&[]);

        // Other scenes.
        for scene in [LIVE, CHATTING, STARTING] {
            assert!(stops(&config, scene, &offline, &mut Memory::default(), start, 120).is_empty());
        }

        // Not streaming.
        let mut memory = Memory::default();
        let stopped = (0..120).any(|second| {
            let observation = Observation {
                now: start + Duration::from_secs(second),
                streaming: false,
                recording: false,
                current_scene: Some(id(ENDING)),
                readings: &offline,
            };
            decide(&config, &observation, &mut memory).stop.is_some()
        });
        assert!(!stopped);

        // Leaving Ending for a while starts it over.
        let mut memory = Memory::default();
        assert!(stops(&config, ENDING, &offline, &mut memory, start, 50).is_empty());
        let later = start + Duration::from_secs(50);
        assert!(stops(&config, CHATTING, &offline, &mut memory, later, 5).is_empty());
        let later = later + Duration::from_secs(5);
        assert_eq!(
            stops(&config, ENDING, &offline, &mut memory, later, 61),
            vec![60]
        );
    }

    const CAM3: u128 = 102;
    const CAMS_LIVE: u128 = 9;
    const CAMS_BRB: u128 = 10;
    const ALL_LIVE: u128 = 11;
    const BOTH_RULE: u128 = 200;
    const ALL_RULE: u128 = 201;
    const HOME_CAM3_RULE: u128 = 202;

    /// BELABOX + HOME: its own LIVE and BRB scenes, LOW left to the server.
    fn both() -> Rule {
        Rule {
            id: id(BOTH_RULE),
            servers: vec![id(BELABOX), id(HOME)],
            scenes: RoleScenes {
                live: Some(id(CAMS_LIVE)),
                offline: Some(id(CAMS_BRB)),
                ..RoleScenes::default()
            },
        }
    }

    fn all() -> Rule {
        Rule {
            id: id(ALL_RULE),
            servers: vec![id(BELABOX), id(HOME), id(CAM3)],
            scenes: RoleScenes {
                live: Some(id(ALL_LIVE)),
                ..RoleScenes::default()
            },
        }
    }

    /// The usual config plus a third server and these rules, in this order.
    fn with_rules(rules: Vec<Rule>) -> Config {
        let mut config = config();
        config.servers.push(Server {
            id: id(CAM3),
            ..Server::default()
        });
        config.rules = rules;
        config
    }

    fn rule_of(config: &Config, readings: &HashMap<Uuid, Reading>) -> Option<Uuid> {
        matching_rule(config, readings).map(|rule| rule.id)
    }

    #[test]
    fn a_rule_applies_while_all_its_servers_have_a_feed() {
        let config = with_rules(vec![both()]);
        let belabox = readings(&[(BELABOX, Reading::Live)]);
        let pair = readings(&[(BELABOX, Reading::Live), (HOME, Reading::Live)]);

        // The second server comes up: its scenes after retry_attempts, like a
        // feed change, so a flapping uplink can't flip the layout.
        let mut memory = settled(&config, LIVE, &belabox);
        // Matching isn't "in use": not while holding on the old scene.
        assert_eq!(
            run(&config, LIVE, true, &pair, &mut memory, 1).active_rule,
            None
        );
        assert_eq!(
            switches_at(&config, LIVE, &pair, &mut memory),
            Some((3, id(CAMS_LIVE)))
        );
        assert_eq!(
            run(&config, CAMS_LIVE, true, &pair, &mut memory, 1).active_rule,
            Some(id(BOTH_RULE))
        );
        // Nor on a scene of the streamer's own.
        assert_eq!(
            run(&config, CHATTING, true, &pair, &mut memory, 1).active_rule,
            None
        );

        // It goes away again: back to the server's scene, after as long.
        assert_eq!(
            switches_at(&config, CAMS_LIVE, &belabox, &mut memory),
            Some((4, id(LIVE)))
        );

        // Both back from offline at once: at once, with instant_recover.
        let mut memory = settled(&config, BRB, &readings(&[]));
        assert_eq!(
            switches_at(&config, BRB, &pair, &mut memory),
            Some((1, id(CAMS_LIVE)))
        );

        // A publisher with no bitrate yet counts as a feed; offline doesn't.
        let holding = readings(&[(BELABOX, Reading::Live), (HOME, Reading::Previous)]);
        assert_eq!(rule_of(&config, &holding), Some(id(BOTH_RULE)));
        let down = readings(&[(BELABOX, Reading::Live), (HOME, Reading::Offline)]);
        assert_eq!(rule_of(&config, &down), None);
    }

    #[test]
    fn rules_match_with_more_servers_live_and_the_first_match_wins() {
        let three = readings(&[
            (BELABOX, Reading::Live),
            (HOME, Reading::Live),
            (CAM3, Reading::Live),
        ]);
        let two = readings(&[(BELABOX, Reading::Live), (HOME, Reading::Live)]);

        let config = with_rules(vec![both()]);
        assert_eq!(rule_of(&config, &three), Some(id(BOTH_RULE)));

        let config = with_rules(vec![both(), all()]);
        assert_eq!(rule_of(&config, &three), Some(id(BOTH_RULE)));

        let config = with_rules(vec![all(), both()]);
        assert_eq!(rule_of(&config, &three), Some(id(ALL_RULE)));
        assert_eq!(rule_of(&config, &two), Some(id(BOTH_RULE)));
    }

    #[test]
    fn the_top_server_decides_live_or_low_and_blank_rule_scenes_fall_back() {
        let config = with_rules(vec![both()]);
        let pair = readings(&[(BELABOX, Reading::Live), (HOME, Reading::Live)]);
        let low = readings(&[(BELABOX, Reading::Low), (HOME, Reading::Live)]);

        // BELABOX goes low while HOME is fine: LOW, and the rule has none, so
        // the switcher's LOW scene.
        let mut memory = settled(&config, CAMS_LIVE, &pair);
        assert_eq!(
            switches_at(&config, CAMS_LIVE, &low, &mut memory),
            Some((4, id(LOW)))
        );

        // No LIVE scene on the rule: the in-use server's override comes next.
        let config = with_rules(vec![Rule {
            id: id(HOME_CAM3_RULE),
            servers: vec![id(HOME), id(CAM3)],
            scenes: RoleScenes {
                offline: Some(id(CAMS_BRB)),
                ..RoleScenes::default()
            },
        }]);
        let home_cam3 = readings(&[(HOME, Reading::Live), (CAM3, Reading::Live)]);
        let mut memory = settled(&config, BRB, &readings(&[]));
        assert_eq!(
            switches_at(&config, BRB, &home_cam3, &mut memory),
            Some((1, id(HOME_LIVE)))
        );
    }

    #[test]
    fn offline_uses_the_last_rules_brb_scene() {
        let config = with_rules(vec![both()]);
        let pair = readings(&[(BELABOX, Reading::Live), (HOME, Reading::Live)]);
        let mut memory = settled(&config, CAMS_LIVE, &pair);
        assert_eq!(
            switches_at(&config, CAMS_LIVE, &readings(&[]), &mut memory),
            Some((4, id(CAMS_BRB)))
        );

        // And back from there: the switcher owns the rule's BRB scene too.
        assert_eq!(
            switches_at(&config, CAMS_BRB, &pair, &mut memory),
            Some((1, id(CAMS_LIVE)))
        );
    }

    #[test]
    fn a_scene_handed_over_by_the_last_run_can_still_be_left() {
        // On the combination's scene when it is turned off: the restarted
        // switcher no longer knows the scene from its config.
        let before = with_rules(vec![both()]);
        let pair = readings(&[(BELABOX, Reading::Live), (HOME, Reading::Live)]);
        let memory = settled(&before, CAMS_LIVE, &pair);
        assert_eq!(
            owned_role(&before, &memory, id(CAMS_LIVE)),
            Some(FeedState::Live)
        );

        let after = with_rules(Vec::new());
        assert_eq!(
            switches_at(&after, CAMS_LIVE, &pair, &mut Memory::default()),
            None
        );
        let mut handed = Memory::handed_over(id(CAMS_LIVE), FeedState::Live);
        assert_eq!(
            switches_at(&after, CAMS_LIVE, &pair, &mut handed).map(|(_, scene)| scene),
            Some(id(LIVE))
        );

        // Its BRB scene too, so the switcher comes back from it.
        let mut handed = Memory::handed_over(id(CAMS_BRB), FeedState::Offline);
        let belabox = readings(&[(BELABOX, Reading::Live)]);
        assert_eq!(
            switches_at(&after, CAMS_BRB, &belabox, &mut handed).map(|(_, scene)| scene),
            Some(id(LIVE))
        );
    }

    #[test]
    fn a_handed_over_scene_stops_counting_once_obs_leaves_it() {
        let config = with_rules(Vec::new());
        let live = readings(&[(BELABOX, Reading::Live)]);
        let mut memory = Memory::handed_over(id(CAMS_LIVE), FeedState::Live);
        // The streamer moves to a scene of their own, then back.
        run(&config, CHATTING, true, &live, &mut memory, 1);
        assert_eq!(switches_at(&config, CAMS_LIVE, &live, &mut memory), None);
    }
}
