use std::time::Duration;

use async_nats::jetstream::kv::Store;
use futures_util::{FutureExt, StreamExt};
use live::coordination::Lease;
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};
use uuid::Uuid;

use crate::error::{Error, Result};

pub struct Owner {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

#[derive(serde::Serialize)]
struct Holder<'a> {
    hostname: String,
    #[serde(flatten)]
    takeover: &'a live::Takeover,
}

impl Owner {
    pub async fn identify(
        store: &Store,
        nats: &async_nats::Client,
        switcher: Uuid,
        connection_id: Uuid,
        identified_at_ms: i64,
        lost: watch::Sender<bool>,
        mut connections: live::Connections,
    ) -> Result<(Option<Self>, live::Takeover)> {
        tokio::time::timeout(
            Duration::from_secs(3),
            connections.wait_for(|count| *count > 0),
        )
        .await
        .map_err(|error| Error::Nats(error.to_string()))?
        .map_err(|error| Error::Nats(error.to_string()))?;
        let session_connections = connections.clone();
        tokio::select! {
            biased;
            _ = connections.changed() => Err(Error::Nats("NATS reconnected during identification; reconnect required".into())),
            result = async {
                let takeover = next_takeover(store, switcher, connection_id, identified_at_ms).await?;
                let owner = Self::acquire(store, nats, switcher, takeover.clone(), lost, session_connections).await?;
                Ok((owner, takeover))
            } => result,
        }
    }

    pub async fn acquire(
        store: &Store,
        nats: &async_nats::Client,
        switcher: Uuid,
        takeover: live::Takeover,
        lost: watch::Sender<bool>,
        connections: live::Connections,
    ) -> Result<Option<Self>> {
        if connections.has_changed().unwrap_or(true) {
            return Err(Error::Nats(
                "NATS reconnected before ownership acquisition".into(),
            ));
        }
        let mut takeovers = nats
            .subscribe(live::takeover_subject(switcher))
            .await
            .map_err(|error| Error::Nats(error.to_string()))?;
        nats.flush()
            .await
            .map_err(|error| Error::Nats(error.to_string()))?;
        let holder = serde_json::to_string(&Holder {
            hostname: std::env::var("HOSTNAME").unwrap_or_default(),
            takeover: &takeover,
        })?;
        let key = switcher.to_string();
        let deadline = tokio::time::Instant::now() + live::OWNER_TTL + Duration::from_secs(3);
        loop {
            while let Some(Some(message)) = takeovers.next().now_or_never() {
                if supersedes(&message.payload, &takeover) {
                    return Err(Error::OwnershipSuperseded);
                }
            }
            match tokio::time::timeout(Duration::from_secs(3), Lease::acquire(store, &key, &holder))
                .await
            {
                Ok(Ok(Some(lease))) => {
                    while let Some(Some(message)) = takeovers.next().now_or_never() {
                        if supersedes(&message.payload, &takeover) {
                            lease.release().await;
                            return Err(Error::OwnershipSuperseded);
                        }
                    }
                    return Ok(Some(Self::start(
                        lease,
                        lost,
                        live::OWNER_RENEW,
                        connections,
                    )));
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => return Err(Error::Nats(error.to_string())),
                Err(error) => return Err(Error::Nats(error.to_string())),
            }
            let holder = tokio::time::timeout(Duration::from_millis(200), store.get(&key))
                .await
                .map_err(|error| Error::Nats(error.to_string()))?
                .map_err(|error| Error::Nats(error.to_string()))?;
            if holder
                .as_ref()
                .is_some_and(|holder| supersedes(holder, &takeover))
            {
                return Err(Error::OwnershipSuperseded);
            }
            let payload = serde_json::to_vec(&takeover)?;
            nats.publish(live::takeover_subject(switcher), payload.into())
                .await?;
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(%switcher, "Timed out waiting for the previous switcher owner; reconnect required");
                return Ok(None);
            }
            let delay = tokio::time::sleep(Duration::from_millis(250));
            tokio::pin!(delay);
            loop {
                tokio::select! {
                    _ = &mut delay => break,
                    message = takeovers.next() => {
                        let Some(message) = message else { return Err(Error::Nats("Takeover subscription closed".into())); };
                        if supersedes(&message.payload, &takeover) {
                            return Err(Error::OwnershipSuperseded);
                        }
                    }
                }
            }
        }
    }

    fn start(
        mut lease: Lease,
        lost: watch::Sender<bool>,
        every: Duration,
        mut connections: live::Connections,
    ) -> Self {
        let (stop, mut stopped) = oneshot::channel();
        let task = crate::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = connections.changed() => {
                        lost.send_replace(true);
                        let _ = stopped.await;
                        // A restarted memory stream can reuse revisions. Never delete it.
                        return;
                    }
                    _ = &mut stopped => break,
                    _ = tokio::time::sleep(every) => {
                        if !matches!(tokio::time::timeout(Duration::from_secs(3), lease.renew()).await, Ok(true)) {
                            lost.send_replace(true);
                            // Teardown, not renewal failure, decides when release is safe.
                            let _ = stopped.await;
                            break;
                        }
                    }
                }
            }
            lease.release().await;
        });
        Self { stop, task }
    }

    pub async fn release(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

pub async fn next_takeover(
    store: &Store,
    switcher: Uuid,
    connection_id: Uuid,
    identified_at_ms: i64,
) -> Result<live::Takeover> {
    let sequence = sequence_with_deadline(async {
        store
            .put(
                format!("{switcher}.order"),
                connection_id.to_string().into(),
            )
            .await
            .map_err(|error| Error::Nats(error.to_string()))
    })
    .await?;
    Ok(live::Takeover {
        connection_id,
        sequence: Some(sequence),
        identified_at_ms: Some(identified_at_ms),
    })
}

async fn sequence_with_deadline(put: impl Future<Output = Result<u64>>) -> Result<u64> {
    tokio::time::timeout(Duration::from_millis(200), put)
        .await
        .map_err(|error| Error::Nats(error.to_string()))?
}

pub fn supersedes(payload: &[u8], current: &live::Takeover) -> bool {
    let Ok(other) = serde_json::from_slice::<live::Takeover>(payload) else {
        return false;
    };
    if other.connection_id == current.connection_id {
        return false;
    }
    if let (Some(other_sequence), Some(current_sequence)) = (other.sequence, current.sequence) {
        return (other_sequence, other.connection_id) > (current_sequence, current.connection_id);
    }
    match (other.identified_at_ms, current.identified_at_ms) {
        (Some(other_time), Some(current_time)) => {
            (other_time, other.connection_id) > (current_time, current.connection_id)
        }
        _ => true,
    }
}

pub async fn guard<T>(mut lost: watch::Receiver<bool>, work: impl Future<Output = T>) -> Option<T> {
    tokio::select! {
        biased;
        _ = lost.wait_for(|lost| *lost) => None,
        result = work => Some(result),
    }
}

pub fn spawn(
    lost: watch::Receiver<bool>,
    work: impl Future<Output = ()> + Send + 'static,
) -> JoinHandle<()> {
    crate::spawn(async move {
        guard(lost, work).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sequence_requires_a_confirmed_revision_within_deadline() {
        assert_eq!(sequence_with_deadline(async { Ok(42) }).await.unwrap(), 42);
        assert!(matches!(
            sequence_with_deadline(async { Err(Error::Nats("unavailable".into())) }).await,
            Err(Error::Nats(error)) if error == "unavailable"
        ));
        assert!(matches!(
            sequence_with_deadline(std::future::pending()).await,
            Err(Error::Nats(_))
        ));
    }

    #[test]
    fn sequence_orders_takeovers_before_timestamps() {
        let current = live::Takeover {
            connection_id: Uuid::from_u128(2),
            identified_at_ms: Some(100),
            sequence: Some(20),
        };
        let payload = |id, time, sequence| {
            serde_json::to_vec(&live::Takeover {
                connection_id: Uuid::from_u128(id),
                identified_at_ms: time,
                sequence,
            })
            .unwrap()
        };
        assert!(supersedes(&payload(1, Some(1), Some(21)), &current));
        assert!(!supersedes(&payload(3, Some(200), Some(19)), &current));
        assert!(supersedes(&payload(3, Some(1), Some(20)), &current));
        assert!(!supersedes(&payload(1, Some(200), Some(20)), &current));
        assert!(!supersedes(&payload(2, Some(200), Some(21)), &current));
        assert!(supersedes(&payload(1, None, Some(21)), &current));
        assert!(!supersedes(&payload(3, Some(99), None), &current));
        assert!(supersedes(&payload(1, Some(101), None), &current));
        let old_current = live::Takeover {
            sequence: None,
            ..current.clone()
        };
        assert!(supersedes(&payload(1, Some(101), Some(1)), &old_current));
        assert!(!supersedes(&payload(3, Some(99), Some(100)), &old_current));
        assert!(supersedes(&payload(1, None, None), &current));
        let timestamp_only = serde_json::json!({
            "connection_id": Uuid::from_u128(1),
            "identified_at_ms": 101,
        });
        let decoded: live::Takeover = serde_json::from_value(timestamp_only.clone()).unwrap();
        assert_eq!(decoded.sequence, None);
        assert!(supersedes(
            &serde_json::to_vec(&timestamp_only).unwrap(),
            &current
        ));
        assert!(!supersedes(b"", &current));
        assert!(!supersedes(b"invalid", &current));
    }

    #[test]
    fn takeover_ordering_and_legacy_payloads() {
        let current = live::Takeover {
            connection_id: Uuid::from_u128(2),
            identified_at_ms: Some(10),
            sequence: None,
        };
        let payload = |id, time| {
            serde_json::to_vec(&live::Takeover {
                connection_id: Uuid::from_u128(id),
                identified_at_ms: time,
                sequence: None,
            })
            .unwrap()
        };
        assert!(!supersedes(&payload(3, Some(9)), &current));
        assert!(supersedes(&payload(1, Some(11)), &current));
        assert!(!supersedes(&payload(1, Some(10)), &current));
        assert!(supersedes(&payload(3, Some(10)), &current));
        assert!(!supersedes(&payload(2, Some(11)), &current));
        assert!(supersedes(&payload(1, None), &current));
        assert!(!supersedes(b"invalid", &current));
        let legacy = serde_json::json!({ "connection_id": Uuid::from_u128(1) });
        assert!(supersedes(&serde_json::to_vec(&legacy).unwrap(), &current));
    }

    #[tokio::test]
    async fn lost_ownership_does_not_poll_work() {
        let (_lost, receiver) = watch::channel(true);
        assert_eq!(
            guard(receiver, async { panic!("lost owner ran work") }).await,
            None::<()>
        );
    }

    #[tokio::test]
    #[ignore = "requires NATS with JetStream on port 4222"]
    async fn reconnect_invalidates_ownership_without_deleting_a_reused_revision() {
        let nats = async_nats::connect("nats://localhost:4222").await.unwrap();
        let js = async_nats::jetstream::new(nats);
        let config = async_nats::jetstream::kv::Config {
            bucket: format!("owners_restart_{}", Uuid::new_v4().simple()),
            ..Default::default()
        };
        let store = js.create_key_value(config.clone()).await.unwrap();
        let lease = Lease::acquire(&store, "switcher", "old")
            .await
            .unwrap()
            .unwrap();
        let (generation, connections) = watch::channel(1);
        let (lost, mut receiver) = watch::channel(false);
        let old = Owner::start(lease, lost, live::OWNER_RENEW, connections);
        js.delete_key_value(&store.name).await.unwrap();
        let replacement = js.create_key_value(config).await.unwrap();
        let new = Lease::acquire(&replacement, "switcher", "new")
            .await
            .unwrap()
            .unwrap();
        generation.send_replace(2);
        tokio::time::timeout(Duration::from_secs(1), receiver.wait_for(|lost| *lost))
            .await
            .unwrap()
            .unwrap();
        old.release().await;
        assert_eq!(
            replacement.get("switcher").await.unwrap().unwrap().as_ref(),
            b"new"
        );
        new.release().await;
        js.delete_key_value(&store.name).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires NATS with JetStream on port 4222"]
    async fn successful_newer_owner_refuses_a_late_older_waiter() {
        let nats = async_nats::connect("nats://localhost:4222").await.unwrap();
        let js = async_nats::jetstream::new(nats.clone());
        let store = js
            .create_key_value(async_nats::jetstream::kv::Config {
                bucket: format!("owners_late_{}", Uuid::new_v4().simple()),
                storage: async_nats::jetstream::stream::StorageType::Memory,
                ..Default::default()
            })
            .await
            .unwrap();
        let switcher = Uuid::new_v4();
        let older = next_takeover(&store, switcher, Uuid::new_v4(), 200)
            .await
            .unwrap();
        let newer = next_takeover(&store, switcher, Uuid::new_v4(), 100)
            .await
            .unwrap();
        let (_generation, connections) = watch::channel(1);
        let (lost, _) = watch::channel(false);
        let winner = Owner::acquire(
            &store,
            &nats,
            switcher,
            newer.clone(),
            lost,
            connections.clone(),
        )
        .await
        .unwrap()
        .unwrap();
        store.delete(format!("{switcher}.order")).await.unwrap();
        let (lost, _) = watch::channel(false);
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            Owner::acquire(&store, &nats, switcher, older, lost, connections),
        )
        .await;
        winner.release().await;
        js.delete_key_value(&store.name).await.unwrap();
        assert!(
            matches!(result, Ok(Err(Error::OwnershipSuperseded))),
            "an older late subscriber must be refused, not time out and reconnect"
        );
    }

    #[tokio::test]
    #[ignore = "requires NATS with JetStream on port 4222"]
    async fn reconnect_during_identification_requires_a_fresh_sequence() {
        let nats = async_nats::connect("nats://localhost:4222").await.unwrap();
        let js = async_nats::jetstream::new(nats.clone());
        let config = async_nats::jetstream::kv::Config {
            bucket: format!("owners_identify_{}", Uuid::new_v4().simple()),
            storage: async_nats::jetstream::stream::StorageType::Memory,
            ..Default::default()
        };
        let store = js.create_key_value(config.clone()).await.unwrap();
        let switcher = Uuid::new_v4();
        let _blocker = Lease::acquire(&store, &switcher.to_string(), "blocker")
            .await
            .unwrap()
            .unwrap();
        let mut announcements = nats
            .subscribe(live::takeover_subject(switcher))
            .await
            .unwrap();
        nats.flush().await.unwrap();
        let (generation, connections) = watch::channel(1);
        let (lost, _) = watch::channel(false);
        let pending_store = store.clone();
        let pending_nats = nats.clone();
        let pending_connections = connections.clone();
        let pending = tokio::spawn(async move {
            Owner::identify(
                &pending_store,
                &pending_nats,
                switcher,
                Uuid::new_v4(),
                100,
                lost,
                pending_connections,
            )
            .await
        });
        let announcement = tokio::time::timeout(Duration::from_secs(2), announcements.next())
            .await
            .unwrap()
            .unwrap();
        let old: live::Takeover = serde_json::from_slice(&announcement.payload).unwrap();
        generation.send_replace(2);
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), pending)
                .await
                .unwrap()
                .unwrap(),
            Err(Error::Nats(_))
        ));
        js.delete_key_value(&store.name).await.unwrap();
        assert!(
            next_takeover(&store, switcher, Uuid::new_v4(), 200)
                .await
                .is_err()
        );
        let replacement = js.create_key_value(config).await.unwrap();
        let (lost, _) = watch::channel(false);
        let (owner, fresh) = Owner::identify(
            &replacement,
            &nats,
            switcher,
            Uuid::new_v4(),
            50,
            lost,
            connections,
        )
        .await
        .unwrap();
        assert_eq!(fresh.sequence, Some(1));
        assert!(fresh.sequence < old.sequence);
        assert!(owner.is_some());
        owner.unwrap().release().await;
        js.delete_key_value(&replacement.name).await.unwrap();
    }
}
