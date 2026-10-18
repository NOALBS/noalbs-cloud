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
    kv::{self, CreateErrorKind, Store},
};
use bytes::Bytes;
use chrono::Utc;
use serde::{Deserialize, Serialize};

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

/// Takes `key` if nobody holds it: `Ok(Some(revision))` when we got it.
pub async fn claim(store: &Store, key: &str, holder: &str) -> Result<Option<u64>> {
    match store.create(key, Bytes::from(holder.to_owned())).await {
        Ok(revision) => Ok(Some(revision)),
        Err(error) if error.kind() == CreateErrorKind::AlreadyExists => Ok(None),
        Err(error) => Err(Error::nats(error)),
    }
}

/// Renews a claim we hold; `Ok(None)` when it lapsed or someone else took it.
pub async fn renew(store: &Store, key: &str, holder: &str, revision: u64) -> Result<Option<u64>> {
    match store
        .update(key, Bytes::from(holder.to_owned()), revision)
        .await
    {
        Ok(revision) => Ok(Some(revision)),
        Err(error) if error.kind() == kv::UpdateErrorKind::WrongLastRevision => Ok(None),
        Err(error) => Err(Error::nats(error)),
    }
}

/// Lets go of a claim we hold (a no-op if it already lapsed).
pub async fn release(store: &Store, key: &str, revision: u64) {
    if let Err(error) = store.delete_expect_revision(key, Some(revision)).await {
        tracing::debug!(%error, key, "Claim already gone");
    }
}

/// `true` the first time `key` is seen within the bucket's age (a delivery
/// retried to another instance, a cooldown).
pub async fn first_time(store: &Store, key: &str) -> Result<bool> {
    Ok(claim(store, key, "").await?.is_some())
}

/// A job that one instance at a time runs, held while renewed.
pub struct Lease {
    store: Store,
    key: String,
    holder: String,
    revision: u64,
}

impl Lease {
    pub async fn acquire(store: &Store, key: &str, holder: &str) -> Result<Option<Self>> {
        Ok(claim(store, key, holder).await?.map(|revision| Self {
            store: store.clone(),
            key: key.to_owned(),
            holder: holder.to_owned(),
            revision,
        }))
    }

    /// Keeps the lease; `false` if it was lost (the job should stop).
    pub async fn renew(&mut self) -> bool {
        match renew(&self.store, &self.key, &self.holder, self.revision).await {
            Ok(Some(revision)) => {
                self.revision = revision;
                true
            }
            Ok(None) => false,
            Err(error) => {
                tracing::warn!(%error, key = self.key, "Couldn't renew a lease");
                false
            }
        }
    }

    pub async fn release(self) {
        release(&self.store, &self.key, self.revision).await;
    }
}

/// A token bucket shared by all instances: `capacity` messages, refilled over
/// `per`. Stored as the tokens left and when they were counted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TokenBucket {
    pub tokens: f64,
    /// Unix milliseconds.
    pub at: i64,
}

impl TokenBucket {
    /// Tries to take a token at `now`: the new state, or how long until one is free.
    pub fn take(
        previous: Option<Self>,
        capacity: f64,
        per: Duration,
        now: i64,
    ) -> std::result::Result<Self, Duration> {
        let rate = capacity / per.as_millis() as f64;
        let tokens = match previous {
            Some(bucket) => (bucket.tokens + (now - bucket.at).max(0) as f64 * rate).min(capacity),
            None => capacity,
        };
        if tokens >= 1.0 {
            Ok(Self {
                tokens: tokens - 1.0,
                at: now,
            })
        } else {
            Err(Duration::from_millis(((1.0 - tokens) / rate).ceil() as u64))
        }
    }
}

/// Waits (up to `max_wait`) for a token in the shared bucket `key`; `false`
/// if none came in time.
pub async fn take_token(
    store: &Store,
    key: &str,
    capacity: f64,
    per: Duration,
    max_wait: Duration,
) -> Result<bool> {
    let deadline = tokio::time::Instant::now() + max_wait;
    loop {
        let entry = store.entry(key).await.map_err(Error::nats)?;
        let (previous, revision) = match &entry {
            Some(entry) if entry.operation == kv::Operation::Put => (
                serde_json::from_slice::<TokenBucket>(&entry.value).ok(),
                Some(entry.revision),
            ),
            _ => (None, None),
        };
        match TokenBucket::take(previous, capacity, per, Utc::now().timestamp_millis()) {
            Ok(next) => {
                let value = Bytes::from(serde_json::to_vec(&next)?);
                let written = match revision {
                    Some(revision) => store.update(key, value, revision).await.is_ok(),
                    None => store.create(key, value).await.is_ok(),
                };
                if written {
                    return Ok(true);
                }
                // Another instance took one meanwhile: count again.
            }
            Err(wait) => {
                if tokio::time::Instant::now() + wait > deadline {
                    return Ok(false);
                }
                tokio::time::sleep(wait).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{TokenBucket, key};

    #[test]
    fn keys_only_hold_allowed_characters() {
        assert_eq!(
            key(&["kick", "01H:x/y z", "bitrate"]),
            "kick.01H_x_y_z.bitrate"
        );
    }

    #[test]
    fn a_token_bucket_empties_and_refills() {
        let per = Duration::from_secs(30);
        let mut bucket = None;
        for _ in 0..20 {
            bucket = Some(TokenBucket::take(bucket, 20.0, per, 0).unwrap());
        }
        // Empty: the next token comes after 30 s / 20 = 1.5 s.
        assert_eq!(
            TokenBucket::take(bucket, 20.0, per, 0),
            Err(Duration::from_millis(1500))
        );
        assert!(TokenBucket::take(bucket, 20.0, per, 1500).is_ok());
        // Refills up to the capacity, not beyond.
        let full = TokenBucket::take(bucket, 20.0, per, 3_600_000).unwrap();
        assert_eq!(full.tokens, 19.0);
    }
}
