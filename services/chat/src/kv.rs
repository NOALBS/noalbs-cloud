//! What the chat service instances share, in NATS KV buckets whose entries
//! expire on their own (`max_age`), so a crashed instance's claims lapse:
//!
//! | Bucket | Key | Written |
//! |---|---|---|
//! | [`INSTANCES`] | instance ID | heartbeat every [`HEARTBEAT`] |
//! | [`SHARDS`] | `shard.<n>` → instance ID | claimed with create, renewed every [`HEARTBEAT`] |
//! | [`LOCKS`] | `reconcile`, … → instance ID | [`Lease`]s for once-at-a-time jobs |
//! | [`SEEN`] | a delivery's ID | once, to drop redelivered webhooks |
//! | [`COOLDOWNS`] | `<channel>.<command>` | once per [`COOLDOWN`] |
//! | [`SEND_LIMITS`] | `<platform>.<bot>.<channel>` | a token bucket, compare-and-set |
//! | [`YOUTUBE`] | `chat.<channel>` → live chat ID | by the YouTube watcher while it follows that chat, every [`HEARTBEAT`] |
//! | [`YOUTUBE_STATUS`] | `watcher`, `error` | for Admin → Status (see [`live::CHAT_YOUTUBE_STATUS_BUCKET`]) |

use std::time::Duration;

use async_nats::jetstream::{
    self,
    kv::{self, Store},
};
pub use live::coordination::{Lease, release};

use crate::error::{Error, Result};

pub const INSTANCES: &str = live::CHAT_INSTANCES_BUCKET;
pub const SHARDS: &str = live::CHAT_SHARDS_BUCKET;
pub const LOCKS: &str = "chat-locks";
pub const SEEN: &str = "chat-seen";
pub const COOLDOWNS: &str = "chat-cooldowns";
pub const SEND_LIMITS: &str = "chat-send-limits";
pub const YOUTUBE: &str = "chat-youtube";
pub const YOUTUBE_STATUS: &str = live::CHAT_YOUTUBE_STATUS_BUCKET;

/// How often claims and heartbeats are renewed; they lapse after [`CLAIM_TTL`].
pub const HEARTBEAT: Duration = Duration::from_secs(10);
pub const CLAIM_TTL: Duration = Duration::from_secs(30);
/// A public chatter can use each command once per this, per channel.
pub const COOLDOWN: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Buckets {
    pub instances: Store,
    pub shards: Store,
    pub locks: Store,
    pub seen: Store,
    pub cooldowns: Store,
    pub send_limits: Store,
    pub youtube: Store,
    pub youtube_status: Store,
}

impl Buckets {
    pub async fn open(js: &jetstream::Context) -> Result<Self> {
        Ok(Self {
            instances: bucket(js, INSTANCES, CLAIM_TTL, "Live chat service instances").await?,
            shards: bucket(
                js,
                SHARDS,
                CLAIM_TTL,
                "Twitch conduit shards claimed by instances",
            )
            .await?,
            locks: bucket(js, LOCKS, CLAIM_TTL, "Chat service leases").await?,
            seen: bucket(
                js,
                SEEN,
                Duration::from_secs(600),
                "Chat deliveries already handled",
            )
            .await?,
            cooldowns: bucket(js, COOLDOWNS, COOLDOWN, "Chat command cooldowns").await?,
            send_limits: bucket(
                js,
                SEND_LIMITS,
                Duration::from_secs(120),
                "Chat send rate limits",
            )
            .await?,
            youtube: bucket(
                js,
                YOUTUBE,
                Duration::from_secs(60),
                "YouTube live chats being followed",
            )
            .await?,
            youtube_status: bucket(
                js,
                YOUTUBE_STATUS,
                Duration::from_secs(24 * 60 * 60),
                "How YouTube chat is doing",
            )
            .await?,
        })
    }
}

/// The buckets live in NATS's memory, so a NATS that restarted has none of
/// them, and every claim, lease, cooldown and "seen before" fails until they
/// exist again. Makes them again whenever we're back (runs until aborted).
pub async fn keep_buckets(js: jetstream::Context, connections: live::Connections) {
    live::reopen(connections, "chat buckets", || Buckets::open(&js)).await;
}

async fn bucket(
    js: &jetstream::Context,
    name: &str,
    max_age: Duration,
    description: &str,
) -> Result<Store> {
    if let Ok(store) = js.get_key_value(name).await {
        return Ok(store);
    }
    js.create_key_value(kv::Config {
        bucket: name.to_owned(),
        description: description.to_owned(),
        history: 1,
        max_age,
        storage: jetstream::stream::StorageType::Memory,
        ..Default::default()
    })
    .await
    .map_err(Error::nats)
}

/// A NATS KV key from any text: KV keys only allow `[-/_=.a-zA-Z0-9]`.
pub fn key(parts: &[&str]) -> String {
    live::chat_key(parts)
}

pub async fn claim(store: &Store, key: &str, holder: &str) -> Result<Option<u64>> {
    live::coordination::claim(store, key, holder)
        .await
        .map_err(Into::into)
}

pub async fn renew(store: &Store, key: &str, holder: &str, revision: u64) -> Result<Option<u64>> {
    live::coordination::renew(store, key, holder, revision)
        .await
        .map_err(Into::into)
}

pub async fn first_time(store: &Store, key: &str) -> Result<bool> {
    live::coordination::first_time(store, key)
        .await
        .map_err(Into::into)
}

pub async fn take_token(
    store: &Store,
    key: &str,
    capacity: f64,
    per: Duration,
    max_wait: Duration,
) -> Result<bool> {
    live::coordination::take_token(store, key, capacity, per, max_wait)
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn keys_only_hold_allowed_characters() {
        assert_eq!(
            key(&["kick", "01H:x/y z", "bitrate"]),
            "kick.01H_x_y_z.bitrate"
        );
    }

    #[tokio::test]
    #[ignore = "requires NATS with JetStream on port 4222"]
    async fn shared_coordination_maps_errors_and_preserves_chat_buckets() {
        let client = async_nats::connect("127.0.0.1:4222").await.unwrap();
        let js = async_nats::jetstream::new(client);
        let name = format!("chat_test_{}", uuid::Uuid::new_v4().simple());
        let store = super::bucket(&js, &name, super::CLAIM_TTL, "test")
            .await
            .unwrap();
        assert!(super::first_time(&store, "message").await.unwrap());
        assert!(!super::first_time(&store, "message").await.unwrap());
        let mut lease = super::Lease::acquire(&store, "job", "chat")
            .await
            .unwrap()
            .unwrap();
        assert!(lease.renew().await);
        lease.release().await;
        js.delete_key_value(&name).await.unwrap();
        assert!(matches!(
            super::claim(&store, "job", "chat").await,
            Err(crate::error::Error::Coordination(_))
        ));
    }
}
