//! Short-lived in-memory copies of what every chat message needs, so the vast
//! majority of messages (the ones that aren't commands) cost no database query.
//!
//! Nothing is invalidated: an entry just expires. A change (a channel linked or
//! unlinked, a new prefix, a suspension, a bot swapped) reaches chat within the
//! TTL, on every instance, without the instances having to tell each other.

use std::{sync::Arc, time::Duration};

use common::ttl_cache::TtlCache;
use db::{
    chat::{ChatLink, ChatPlatform},
    chat_commands::SwitcherCommand,
};
use uuid::Uuid;

/// How long a channel's links are trusted.
pub const LINKS_TTL: Duration = Duration::from_secs(10);
/// How long the bots' account IDs are trusted (they change when a bot is connected).
pub const BOTS_TTL: Duration = Duration::from_secs(60);

/// How long a switcher's command words (names and aliases) are trusted, to tell
/// its commands from everything else that starts with its prefix. Who may use a
/// command is never taken from here.
pub const COMMANDS_TTL: Duration = Duration::from_secs(10);

/// What the message handler looks up for every message.
pub struct Cache {
    /// The switchers a channel is linked to (none is cached too).
    pub links: TtlCache<(ChatPlatform, String), Vec<ChatLink>>,
    /// Every bot account's platform user ID, per platform.
    pub bot_ids: TtlCache<ChatPlatform, Vec<String>>,
    /// A switcher's commands, for their words only (see [`COMMANDS_TTL`]).
    pub commands: TtlCache<Uuid, Arc<Vec<SwitcherCommand>>>,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            links: TtlCache::new(LINKS_TTL),
            bot_ids: TtlCache::new(BOTS_TTL),
            commands: TtlCache::new(COMMANDS_TTL),
        }
    }
}
