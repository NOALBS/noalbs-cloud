//! The live status of one switcher, shared by its connection and switcher task
//! and published to the NATS KV bucket for the dashboard.

use std::time::Duration;

use async_nats::jetstream::kv;
use chrono::{DateTime, Utc};
use live::SwitcherStatus;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Clone)]
pub struct StatusHandle(watch::Sender<SwitcherStatus>);

impl StatusHandle {
    pub fn new(initial: SwitcherStatus) -> Self {
        Self(watch::channel(initial).0)
    }

    /// Applies `change`, notifying the publisher only if something differs.
    pub fn update(&self, change: impl FnOnce(&mut SwitcherStatus)) {
        self.0.send_if_modified(|status| {
            let before = status.clone();
            change(status);
            let changed = *status != before;
            if changed {
                status.updated_at = Utc::now();
            }
            changed
        });
    }

    pub fn subscribe(&self) -> watch::Receiver<SwitcherStatus> {
        self.0.subscribe()
    }
}

/// A status kept in a KV bucket, whose `updated_at` says when it was written.
pub trait Published: Clone + serde::Serialize {
    fn stamp(&mut self, at: DateTime<Utc>);
}

impl Published for SwitcherStatus {
    fn stamp(&mut self, at: DateTime<Utc>) {
        self.updated_at = at;
    }
}

impl Published for live::BelaboxStatus {
    fn stamp(&mut self, at: DateTime<Utc>) {
        self.updated_at = at;
    }
}

impl Published for live::MoblinStatus {
    fn stamp(&mut self, at: DateTime<Utc>) {
        self.updated_at = at;
    }
}

/// Writes the status on every change and every `heartbeat` (the switcher's and
/// BELABOX's are [`live::STATUS_HEARTBEAT`]), so the key only expires if this
/// obs-link stops.
pub async fn publish<T: Published>(
    store: kv::Store,
    switcher_id: Uuid,
    heartbeat: Duration,
    mut status: watch::Receiver<T>,
) {
    let key = live::status_key(switcher_id);
    let mut heartbeat = tokio::time::interval(heartbeat);

    loop {
        let mut current = status.borrow_and_update().clone();
        // `updated_at` doubles as "last written", which viewers use to spot a
        // status that stopped being refreshed.
        current.stamp(Utc::now());

        match serde_json::to_vec(&current) {
            Ok(payload) => {
                if let Err(error) = store.put(&key, payload.into()).await {
                    tracing::warn!(%error, %switcher_id, bucket = %store.name, "Failed to write status");
                }
            }
            Err(error) => tracing::warn!(%error, "Failed to serialize a status"),
        }

        tokio::select! {
            changed = status.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            _ = heartbeat.tick() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StatusHandle;
    use live::SwitcherStatus;

    #[test]
    fn only_real_changes_notify() {
        let handle = StatusHandle::new(SwitcherStatus::connected(true));
        let mut receiver = handle.subscribe();
        receiver.borrow_and_update();

        handle.update(|status| status.obs_connected = false);
        assert!(!receiver.has_changed().unwrap());

        handle.update(|status| status.obs_connected = true);
        assert!(receiver.has_changed().unwrap());
        assert!(receiver.borrow().obs_connected);
    }
}
