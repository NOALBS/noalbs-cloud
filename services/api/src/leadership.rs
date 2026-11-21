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
    pub async fn start(
        nats: async_nats::Client,
        connections: live::Connections,
        shutdown: ShutdownHandler,
    ) -> Result<Self, async_nats::Error> {
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
                        leader.gate.clear();
                        if let Some(lease) = lease { let _ = tokio::time::timeout(Duration::from_secs(1), lease.release()).await; }
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
