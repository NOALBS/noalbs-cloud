use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use async_nats::jetstream::{self, kv::Store};
use common::shutdown::ShutdownHandler;
use live::coordination::Lease;
use tokio::time::Instant;
use uuid::Uuid;

const BUCKET: &str = "api-leases";
const TTL: Duration = Duration::from_secs(15);

#[derive(Default)]
struct Gate(RwLock<Option<Held>>);

struct Held {
    epoch: Uuid,
    generation: u64,
    until: Instant,
}

impl Gate {
    fn epoch(&self, connected: bool, generation: u64) -> Option<Uuid> {
        let held = self.0.read().ok()?;
        let held = held.as_ref()?;
        (connected && held.generation == generation && Instant::now() < held.until)
            .then_some(held.epoch)
    }

    fn clear(&self) {
        *self.0.write().expect("leadership lock") = None;
    }
}

#[derive(Clone)]
pub struct Leadership {
    gate: Arc<Gate>,
    nats: async_nats::Client,
    connections: live::Connections,
}

async fn open_bucket(js: &jetstream::Context) -> Result<Store, async_nats::Error> {
    if let Ok(store) = js.get_key_value(BUCKET).await {
        return Ok(store);
    }
    Ok(js
        .create_key_value(jetstream::kv::Config {
            bucket: BUCKET.to_owned(),
            max_age: TTL,
            storage: jetstream::stream::StorageType::Memory,
            ..Default::default()
        })
        .await?)
}

impl Leadership {
    pub fn spawn_job<F, Work>(
        &self,
        period: Duration,
        mut shutdown: ShutdownHandler,
        mut work: F,
    ) -> tokio::task::JoinHandle<()>
    where
        F: FnMut() -> Work + Send + 'static,
        Work: Future<Output = ()> + Send,
    {
        let leader = self.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(1));
            let mut ran = None;
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => return,
                    _ = every.tick() => {
                        let Some(epoch) = leader.epoch() else { continue; };
                        if ran.is_some_and(|(previous, at): (_, Instant)| previous == epoch && at.elapsed() < period) { continue; }
                        work().await;
                        ran = Some((epoch, Instant::now()));
                    }
                }
            }
        })
    }

    pub async fn start(
        nats: async_nats::Client,
        mut connections: live::Connections,
        shutdown: ShutdownHandler,
    ) -> Result<Self, async_nats::Error> {
        tokio::time::timeout(
            Duration::from_secs(3),
            connections.wait_for(|count| *count > 0),
        )
        .await??;
        let js = jetstream::new(nats.clone());
        let store = open_bucket(&js).await?;
        Ok(Self::spawn(nats, connections, shutdown, js, store))
    }

    fn spawn(
        nats: async_nats::Client,
        mut connections: live::Connections,
        mut shutdown: ShutdownHandler,
        js: jetstream::Context,
        mut store: Store,
    ) -> Self {
        connections.borrow_and_update();
        let this = Self {
            gate: Arc::default(),
            nats,
            connections: connections.clone(),
        };
        let leader = this.clone();
        tokio::spawn(async move {
            let holder = format!(
                "{}:{}",
                std::env::var("HOSTNAME").unwrap_or_default(),
                Uuid::new_v4()
            );
            let mut lease: Option<Lease> = None;
            let mut every = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => {
                        let release = leader.epoch().is_some();
                        leader.gate.clear();
                        if release && let Some(lease) = lease { let _ = tokio::time::timeout(Duration::from_secs(1), lease.release()).await; }
                        return;
                    }
                    _ = connections.changed() => {
                        leader.gate.clear();
                        lease = None;
                        connections.borrow_and_update();
                    }
                    _ = every.tick() => {
                        let started = Instant::now();
                        let generation = *connections.borrow();
                        let renewed = if let Some(held) = lease.as_mut() {
                            tokio::time::timeout(Duration::from_secs(1), held.renew()).await.unwrap_or(false)
                        } else {
                            match tokio::time::timeout(Duration::from_secs(1), Lease::acquire(&store, "platform", &holder)).await {
                                Ok(Ok(acquired)) => { lease = acquired; lease.is_some() }
                                _ => false,
                            }
                        };
                        if renewed {
                            let mut gate = leader.gate.0.write().expect("leadership lock");
                            let epoch = gate.as_ref().map(|held| held.epoch).unwrap_or_else(Uuid::new_v4);
                            *gate = Some(Held { epoch, generation, until: started + TTL - Duration::from_secs(1) });
                        } else {
                            leader.gate.clear();
                            lease = None;
                            if let Ok(Ok(reopened)) = tokio::time::timeout(Duration::from_secs(1), open_bucket(&js)).await { store = reopened; }
                        }
                    }
                }
            }
        });
        this
    }

    pub fn epoch(&self) -> Option<Uuid> {
        self.gate.epoch(
            self.nats.connection_state() == async_nats::connection::State::Connected,
            *self.connections.borrow(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires NATS with JetStream on port 4222"]
    async fn replicas_gate_metric_reports_and_upkeep_and_stop_after_lease_loss() {
        let nats = async_nats::connect("127.0.0.1:4222").await.unwrap();
        let js = jetstream::new(nats.clone());
        let name = format!("api_leaders_{}", Uuid::new_v4().simple());
        let store = js
            .create_key_value(jetstream::kv::Config {
                bucket: name.clone(),
                max_age: TTL,
                storage: jetstream::stream::StorageType::Memory,
                ..Default::default()
            })
            .await
            .unwrap();
        let (generation, connections) = tokio::sync::watch::channel(1);
        let first = common::shutdown::ShutdownManager::new();
        let second = common::shutdown::ShutdownManager::new();
        let one = Leadership::spawn(
            nats.clone(),
            connections.clone(),
            first.handle(),
            js.clone(),
            store.clone(),
        );
        let two = Leadership::spawn(
            nats,
            connections,
            second.handle(),
            js.clone(),
            store.clone(),
        );
        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for (leader, shutdown) in [(&one, first.handle()), (&two, second.handle())] {
            let runs = runs.clone();
            leader.spawn_job(Duration::from_secs(60), shutdown, move || {
                let runs = runs.clone();
                async move {
                    runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            });
        }
        tokio::time::timeout(Duration::from_secs(8), async {
            while runs.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
        let epoch = one.epoch().or_else(|| two.epoch()).unwrap();
        assert_eq!(
            [&one, &two]
                .iter()
                .filter(|leader| crate::metrics::can_report(leader, epoch))
                .count(),
            1
        );
        store.put("platform", "replacement".into()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(7), async {
            while crate::metrics::can_report(&one, epoch) || crate::metrics::can_report(&two, epoch)
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
        store.delete("platform").await.unwrap();
        tokio::time::timeout(Duration::from_secs(8), async {
            while runs.load(std::sync::atomic::Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(!crate::metrics::can_report(&one, epoch));
        assert!(!crate::metrics::can_report(&two, epoch));
        let revision = store.entry("platform").await.unwrap().unwrap().revision;
        generation.send_replace(2);
        assert!(one.epoch().is_none() && two.epoch().is_none());
        tokio::join!(first.shutdown(), second.shutdown());
        assert_eq!(
            store.entry("platform").await.unwrap().unwrap().revision,
            revision
        );
        js.delete_key_value(name).await.unwrap();
    }

    #[test]
    fn gates_expiry_disconnect_reconnect_and_loss() {
        let gate = Gate::default();
        assert_eq!(gate.epoch(true, 1), None);
        let epoch = Uuid::new_v4();
        *gate.0.write().unwrap() = Some(Held {
            epoch,
            generation: 1,
            until: Instant::now() + TTL,
        });
        assert_eq!(gate.epoch(true, 1), Some(epoch));
        assert_eq!(gate.epoch(false, 1), None);
        assert_eq!(gate.epoch(true, 2), None);
        gate.0.write().unwrap().as_mut().unwrap().until = Instant::now();
        assert_eq!(gate.epoch(true, 1), None);
        gate.clear();
        assert_eq!(gate.epoch(true, 1), None);
    }
}
