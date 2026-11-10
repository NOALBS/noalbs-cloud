//! Database-wide snapshots, repeated by each API replica. Use max/last, not sum,
//! across replicas in Grafana. Callbacks only read memory, never Postgres or NATS.

use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use chrono::{TimeDelta, Utc};
use common::shutdown::ShutdownHandler;
use opentelemetry::KeyValue;

use crate::{app::ApiState, routes::admin_status::switcher_statuses};

const WINDOWS: [(&str, i64); 3] = [("1d", 1), ("7d", 7), ("30d", 30)];

#[derive(Default)]
struct Snapshot {
    users: [u64; 3],
    switchers: [u64; 3],
    devices: [u64; 3],
}

pub fn spawn_refresh(state: ApiState, mut shutdown: ShutdownHandler) {
    if !std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok_and(|value| !value.trim().is_empty()) {
        return;
    }
    let latest = Arc::new(RwLock::new(None::<Snapshot>));
    let users = latest.clone();
    let users_gauge = telemetry::meter()
        .u64_observable_gauge("noalbs.users.active")
        .with_description("Users active in a rolling window, excluding disabled accounts")
        .with_callback(move |observer| {
            if let Ok(snapshot) = users.read()
                && let Some(snapshot) = snapshot.as_ref()
            {
                for (index, (window, _)) in WINDOWS.iter().enumerate() {
                    observer.observe(snapshot.users[index], &[KeyValue::new("window", *window)]);
                }
            }
        })
        .build();
    let switchers = latest.clone();
    let switchers_gauge = telemetry::meter()
        .u64_observable_gauge("noalbs.switchers.active")
        .with_description("Switchers seen in a rolling window or currently connected")
        .with_callback(move |observer| {
            if let Ok(snapshot) = switchers.read()
                && let Some(snapshot) = snapshot.as_ref()
            {
                for (index, (window, _)) in WINDOWS.iter().enumerate() {
                    observer.observe(
                        snapshot.switchers[index],
                        &[KeyValue::new("window", *window)],
                    );
                }
            }
        })
        .build();
    let devices = latest.clone();
    let devices_gauge = telemetry::meter()
        .u64_observable_gauge("noalbs.connect.devices")
        .with_description("Unrevoked Connect devices, paired or recently seen")
        .with_callback(move |observer| {
            if let Ok(snapshot) = devices.read()
                && let Some(snapshot) = snapshot.as_ref()
            {
                for (index, state) in ["paired", "active_1d", "active_30d"].iter().enumerate() {
                    observer.observe(snapshot.devices[index], &[KeyValue::new("state", *state)]);
                }
            }
        })
        .build();
    tokio::spawn(async move {
        let _gauges = (users_gauge, switchers_gauge, devices_gauge);
        let mut every = tokio::time::interval(Duration::from_secs(300));
        loop {
            tokio::select! {
                _ = shutdown.changed() => return,
                _ = every.tick() => {
                    match refresh(&state).await {
                        Ok(snapshot) => {
                            if let Ok(mut latest) = latest.write() { *latest = Some(snapshot); }
                        }
                        Err(error) => tracing::warn!(%error, "Couldn't refresh activity metrics"),
                    }
                }
            }
        }
    });
}

async fn refresh(state: &ApiState) -> Result<Snapshot, String> {
    let statuses = switcher_statuses(state).await?;
    let connected: Vec<_> = statuses.iter().map(|(id, _)| *id).collect();
    let connected_devices: Vec<_> = statuses
        .iter()
        .filter_map(|(_, status)| status.pc.as_ref()?.device_id)
        .collect();
    let now = Utc::now();
    let mut snapshot = Snapshot::default();
    for (index, (_, days)) in WINDOWS.iter().enumerate() {
        let counts = db::admin::active_counts(&state.db, now - TimeDelta::days(*days), &connected)
            .await
            .map_err(|error| error.to_string())?;
        snapshot.users[index] = counts.users as u64;
        snapshot.switchers[index] = counts.switchers as u64;
    }
    let devices = db::admin::device_counts(&state.db, now, &connected_devices)
        .await
        .map_err(|error| error.to_string())?;
    snapshot.devices = [
        devices.paired as u64,
        devices.active_1d as u64,
        devices.active_30d as u64,
    ];
    Ok(snapshot)
}
