//! Short-lived in-memory copies of what every chat message needs, so the vast
//! majority of messages (the ones that aren't commands) cost no database query.
//!
//! Nothing is invalidated: an entry just expires. A change (a channel linked or
//! unlinked, a new prefix, a suspension, a bot swapped) reaches chat within the
//! TTL, on every instance, without the instances having to tell each other.

use std::time::Duration;

use common::ttl_cache::TtlCache;
use db::chat::{ChatLink, ChatPlatform};

/// How long a channel's links are trusted.
pub const LINKS_TTL: Duration = Duration::from_secs(10);
/// How long the bots' account IDs are trusted (they change when a bot is connected).
pub const BOTS_TTL: Duration = Duration::from_secs(60);

/// What the message handler looks up for every message.
pub struct Cache {
    /// The switchers a channel is linked to (none is cached too).
    pub links: TtlCache<(ChatPlatform, String), Vec<ChatLink>>,
    /// Every bot account's platform user ID, per platform.
    pub bot_ids: TtlCache<ChatPlatform, Vec<String>>,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            links: TtlCache::new(LINKS_TTL),
            bot_ids: TtlCache::new(BOTS_TTL),
        }
    }
}
