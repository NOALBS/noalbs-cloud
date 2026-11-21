use std::time::Duration;

use async_nats::jetstream::kv::Store;
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

impl Owner {
    pub async fn acquire(
        store: &Store,
        nats: &async_nats::Client,
        switcher: Uuid,
        connection: Uuid,
        lost: watch::Sender<bool>,
        mut connections: live::Connections,
    ) -> Result<Option<Self>> {
        tokio::time::timeout(
            Duration::from_secs(3),
            connections.wait_for(|count| *count > 0),
        )
        .await
        .map_err(|error| Error::Nats(error.to_string()))?
        .map_err(|error| Error::Nats(error.to_string()))?;
        connections.borrow_and_update();
        let holder = format!(
            "{}:{connection}",
            std::env::var("HOSTNAME").unwrap_or_default()
        );
        let key = switcher.to_string();
        let deadline = tokio::time::Instant::now() + live::OWNER_TTL + Duration::from_secs(3);
        loop {
            match tokio::time::timeout(Duration::from_secs(3), Lease::acquire(store, &key, &holder))
                .await
            {
                Ok(Ok(Some(lease))) => {
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
            let payload = serde_json::to_vec(&live::Takeover {
                connection_id: connection,
            })?;
            nats.publish(live::takeover_subject(switcher), payload.into())
                .await?;
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(%switcher, "Timed out waiting for the previous switcher owner; reconnect required");
                return Ok(None);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
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
}
