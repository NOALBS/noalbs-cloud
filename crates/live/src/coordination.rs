//! Compare-and-set coordination over caller-managed NATS KV buckets.
//!
//! Bucket retention sets claim, lease and deduplication lifetimes. Callers must
//! stop protected work whenever renewal does not confirm ownership.

use std::time::Duration;

use async_nats::jetstream::kv::{self, CreateErrorKind, Store};
use chrono::Utc;
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("NATS error: {0}")]
    Nats(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    fn nats(error: impl std::fmt::Display) -> Self {
        Self::Nats(error.to_string())
    }
}

/// Takes `key` if nobody holds it: `Ok(Some(revision))` when we got it.
pub async fn claim(store: &Store, key: &str, holder: &str) -> Result<Option<u64>> {
    match store.create(key, holder.to_owned().into()).await {
        Ok(revision) => Ok(Some(revision)),
        Err(error) if error.kind() == CreateErrorKind::AlreadyExists => Ok(None),
        Err(error) => Err(Error::nats(error)),
    }
}

/// Renews a claim we hold; `Ok(None)` when it lapsed or someone else took it.
/// Only `Ok(Some(revision))` confirms ownership; stop protected work otherwise.
pub async fn renew(store: &Store, key: &str, holder: &str, revision: u64) -> Result<Option<u64>> {
    match store.update(key, holder.to_owned().into(), revision).await {
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
    revision: Option<u64>,
}

impl Lease {
    pub async fn acquire(store: &Store, key: &str, holder: &str) -> Result<Option<Self>> {
        Ok(claim(store, key, holder).await?.map(|revision| Self {
            store: store.clone(),
            key: key.to_owned(),
            holder: holder.to_owned(),
            revision: Some(revision),
        }))
    }

    /// Only `true` confirms ownership. Any failure permanently loses this lease;
    /// the holder must stop its work and acquire a new lease before resuming.
    #[must_use = "stop protected work unless renewal confirms ownership"]
    pub async fn renew(&mut self) -> bool {
        let Some(revision) = self.revision.take() else {
            return false;
        };
        match renew(&self.store, &self.key, &self.holder, revision).await {
            Ok(Some(revision)) => {
                self.revision = Some(revision);
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
        if let Some(revision) = self.revision {
            release(&self.store, &self.key, revision).await;
        }
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
                let value = serde_json::to_vec(&next)?.into();
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

    use super::TokenBucket;

    async fn bucket() -> (
        async_nats::jetstream::Context,
        async_nats::jetstream::kv::Store,
    ) {
        let client = async_nats::connect("127.0.0.1:4222").await.unwrap();
        let js = async_nats::jetstream::new(client);
        let store = js
            .create_key_value(async_nats::jetstream::kv::Config {
                bucket: format!("coordination_{}", uuid::Uuid::new_v4().simple()),
                max_age: Duration::from_secs(2),
                storage: async_nats::jetstream::stream::StorageType::Memory,
                ..Default::default()
            })
            .await
            .unwrap();
        (js, store)
    }

    #[tokio::test]
    #[ignore = "requires NATS with JetStream on port 4222"]
    async fn claims_dedup_and_revision_fencing() {
        let (js, store) = bucket().await;
        let revision = super::claim(&store, "owner", "one").await.unwrap().unwrap();
        assert!(
            super::claim(&store, "owner", "two")
                .await
                .unwrap()
                .is_none()
        );
        let next = super::renew(&store, "owner", "one", revision)
            .await
            .unwrap()
            .unwrap();
        assert!(
            super::renew(&store, "owner", "two", revision)
                .await
                .unwrap()
                .is_none()
        );
        super::release(&store, "owner", revision).await;
        assert!(
            super::claim(&store, "owner", "two")
                .await
                .unwrap()
                .is_none()
        );
        super::release(&store, "owner", next).await;
        assert!(
            super::claim(&store, "owner", "two")
                .await
                .unwrap()
                .is_some()
        );
        assert!(super::first_time(&store, "delivery").await.unwrap());
        assert!(!super::first_time(&store, "delivery").await.unwrap());
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(super::first_time(&store, "delivery").await.unwrap());
        js.delete_key_value(&store.name).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires NATS with JetStream on port 4222"]
    async fn leases_fail_closed_on_conflict_expiry_and_backend_error() {
        let (js, store) = bucket().await;
        let mut lease = super::Lease::acquire(&store, "job", "one")
            .await
            .unwrap()
            .unwrap();
        assert!(
            super::Lease::acquire(&store, "job", "two")
                .await
                .unwrap()
                .is_none()
        );
        assert!(lease.renew().await);
        store.put("job", "other".into()).await.unwrap();
        assert!(!lease.renew().await);
        assert!(!lease.renew().await);
        lease.release().await;
        assert_eq!(store.get("job").await.unwrap().unwrap().as_ref(), b"other");
        let mut expired = super::Lease::acquire(&store, "expired", "one")
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        let successor = super::Lease::acquire(&store, "expired", "two")
            .await
            .unwrap()
            .unwrap();
        assert!(!expired.renew().await);
        successor.release().await;
        let mut broken = super::Lease::acquire(&store, "broken", "one")
            .await
            .unwrap()
            .unwrap();
        js.delete_key_value(&store.name).await.unwrap();
        assert!(!broken.renew().await);
        assert!(!broken.renew().await);
        assert!(super::claim(&store, "missing", "one").await.is_err());
        assert!(super::first_time(&store, "missing").await.is_err());
    }

    #[tokio::test]
    #[ignore = "requires NATS with JetStream on port 4222"]
    async fn shared_tokens_exhaust_wait_and_refill() {
        let (js, store) = bucket().await;
        let per = Duration::from_millis(100);
        let (one, two) = tokio::join!(
            super::take_token(&store, "limit", 1.0, per, Duration::ZERO),
            super::take_token(&store, "limit", 1.0, per, Duration::ZERO)
        );
        assert_ne!(one.unwrap(), two.unwrap());
        assert!(
            super::take_token(&store, "limit", 1.0, per, Duration::from_secs(1))
                .await
                .unwrap()
        );
        js.delete_key_value(&store.name).await.unwrap();
        assert!(
            super::take_token(&store, "limit", 1.0, per, Duration::ZERO)
                .await
                .is_err()
        );
    }

    #[test]
    fn empty_bucket_starts_full_and_backward_clock_does_not_refill() {
        let per = Duration::from_secs(1);
        let bucket = TokenBucket::take(None, 2.0, per, 100).unwrap();
        assert_eq!(bucket.tokens, 1.0);
        assert_eq!(
            TokenBucket::take(Some(bucket), 2.0, per, 0).unwrap().tokens,
            0.0
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
