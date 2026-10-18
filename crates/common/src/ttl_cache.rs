//! A small in-memory map whose entries expire, for answers that are asked for far
//! more often than they change (a channel's links, a session's user).
//!
//! Nothing is invalidated: an entry just expires, so whoever uses it decides how
//! stale an answer may be (the TTL) and must not cache what has to be seen at once.

use std::{
    collections::HashMap,
    hash::Hash,
    sync::Mutex,
    time::{Duration, Instant},
};

/// A cache holding more entries than this drops the expired ones before growing.
const PRUNE_ABOVE: usize = 10_000;

/// A map whose entries expire `ttl` after they were loaded.
pub struct TtlCache<K, V> {
    ttl: Duration,
    entries: Mutex<HashMap<K, (Instant, V)>>,
}

impl<K: Hash + Eq + Clone, V: Clone> TtlCache<K, V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::default(),
        }
    }

    /// The cached value, or `load`'s (cached when it succeeds). Concurrent
    /// misses each load: that's a few queries once per TTL, not worth a lock
    /// held across an await.
    pub async fn get_or_load<E, F>(&self, key: K, load: impl FnOnce() -> F) -> Result<V, E>
    where
        F: Future<Output = Result<V, E>>,
    {
        self.get_or_load_if(key, load, |_| true).await
    }

    /// Like [`get_or_load`](Self::get_or_load), but only keeps what `keep` accepts:
    /// the rest is returned and asked again next time (a refusal or a missing
    /// session shouldn't outlive the moment it was true).
    pub async fn get_or_load_if<E, F>(
        &self,
        key: K,
        load: impl FnOnce() -> F,
        keep: impl FnOnce(&V) -> bool,
    ) -> Result<V, E>
    where
        F: Future<Output = Result<V, E>>,
    {
        if let Some(value) = self.get(&key) {
            return Ok(value);
        }
        let value = load().await?;
        if keep(&value) {
            self.insert(key, value.clone());
        }
        Ok(value)
    }

    fn get(&self, key: &K) -> Option<V> {
        let entries = self.entries.lock().ok()?;
        entries
            .get(key)
            .filter(|(loaded, _)| loaded.elapsed() < self.ttl)
            .map(|(_, value)| value.clone())
    }

    fn insert(&self, key: K, value: V) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        if entries.len() >= PRUNE_ABOVE {
            let ttl = self.ttl;
            entries.retain(|_, (loaded, _)| loaded.elapsed() < ttl);
        }
        entries.insert(key, (Instant::now(), value));
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU32, Ordering},
        time::Duration,
    };

    use super::TtlCache;

    #[tokio::test]
    async fn loads_once_until_the_entry_expires() {
        let cache = TtlCache::new(Duration::from_millis(40));
        let loads = AtomicU32::new(0);
        let load = || async { Ok::<_, ()>(loads.fetch_add(1, Ordering::SeqCst) + 1) };

        assert_eq!(cache.get_or_load("a", load).await, Ok(1));
        assert_eq!(cache.get_or_load("a", load).await, Ok(1));
        assert_eq!(cache.get_or_load("b", load).await, Ok(2));

        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(cache.get_or_load("a", load).await, Ok(3));
    }

    #[tokio::test]
    async fn failures_are_not_cached() {
        let cache = TtlCache::<&str, u32>::new(Duration::from_secs(60));
        assert_eq!(
            cache.get_or_load("a", || async { Err("down") }).await,
            Err("down")
        );
        assert_eq!(
            cache.get_or_load("a", || async { Ok::<_, &str>(7) }).await,
            Ok(7)
        );
    }

    #[tokio::test]
    async fn only_what_keep_accepts_is_cached() {
        let cache = TtlCache::new(Duration::from_secs(60));
        let loads = AtomicU32::new(0);
        let load = || async { Ok::<_, ()>(loads.fetch_add(1, Ordering::SeqCst)) };
        let even = |value: &u32| value % 2 == 0;

        // 0 is kept, so the second ask doesn't load.
        assert_eq!(cache.get_or_load_if("a", load, even).await, Ok(0));
        assert_eq!(cache.get_or_load_if("a", load, even).await, Ok(0));
        // 1 isn't kept, so it's asked again, and 2 is.
        assert_eq!(cache.get_or_load_if("b", load, even).await, Ok(1));
        assert_eq!(cache.get_or_load_if("b", load, even).await, Ok(2));
        assert_eq!(cache.get_or_load_if("b", load, even).await, Ok(2));
    }
}
