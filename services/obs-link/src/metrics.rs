//! obs-link's metrics (exported only with OTLP set up, see the `telemetry` crate).
//! Never a switcher ID as an attribute: the number of series is the budget.

use std::sync::LazyLock;

use live::SwitcherStatus;
use opentelemetry::{
    KeyValue,
    metrics::{Counter, Histogram, UpDownCounter},
};
use tokio::sync::watch;

/// From relaying an OBS request to the browser source until OBS answers.
pub static OBS_REQUEST_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    telemetry::meter()
        .f64_histogram("noalbs.obs.request.duration")
        .with_unit("s")
        .with_description("OBS requests relayed through the browser source, until OBS answers")
        .with_boundaries(vec![0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0])
        .build()
});

/// Automatic scene switches, by `outcome` (`ok`, `error`).
pub static SCENE_SWITCHES: LazyLock<Counter<u64>> = LazyLock::new(|| {
    telemetry::meter()
        .u64_counter("noalbs.switcher.scene_switches")
        .with_description("Automatic scene switches")
        .build()
});

/// Stream server stats fetches, by `kind` and `outcome` (`ok`, `error`).
pub static STATS_FETCHES: LazyLock<Counter<u64>> = LazyLock::new(|| {
    telemetry::meter()
        .u64_counter("noalbs.stream_server.fetches")
        .with_description("Stream server stats fetches")
        .build()
});

static CONNECTIONS: LazyLock<UpDownCounter<i64>> = LazyLock::new(|| {
    telemetry::meter()
        .i64_up_down_counter("noalbs.obslink.connections")
        .with_description("Open WebSocket connections, by kind (browser source, Moblin)")
        .build()
});

static SWITCHERS: LazyLock<UpDownCounter<i64>> = LazyLock::new(|| {
    telemetry::meter()
        .i64_up_down_counter("noalbs.switchers")
        .with_description(
            "Switchers by state: identified browser source, OBS connected, streaming, paused",
        )
        .build()
});

/// One unit of an up/down counter while it's up; dropping it (also when its task
/// is aborted) takes it back.
pub struct Gauge {
    counter: &'static UpDownCounter<i64>,
    attributes: [KeyValue; 1],
    up: bool,
}

impl Gauge {
    fn new(counter: &'static UpDownCounter<i64>, attribute: KeyValue) -> Self {
        Self {
            counter,
            attributes: [attribute],
            up: false,
        }
    }

    fn set(&mut self, up: bool) {
        if up != self.up {
            self.counter.add(if up { 1 } else { -1 }, &self.attributes);
            self.up = up;
        }
    }
}

impl Drop for Gauge {
    fn drop(&mut self) {
        self.set(false);
    }
}

/// Counts an open connection until dropped.
pub fn connection(kind: &'static str) -> Gauge {
    let mut gauge = Gauge::new(&CONNECTIONS, KeyValue::new("connection.kind", kind));
    gauge.set(true);
    gauge
}

/// Counts an identified switcher by what its status says, until the task stops
/// (the connection aborts it on close).
pub async fn track_switcher(mut status: watch::Receiver<SwitcherStatus>) {
    let state = |name: &'static str| Gauge::new(&SWITCHERS, KeyValue::new("state", name));
    let mut connected = state("connected");
    let mut obs = state("obs_connected");
    let mut streaming = state("streaming");
    let mut paused = state("paused");
    connected.set(true);

    loop {
        {
            let status = status.borrow_and_update();
            obs.set(status.obs_connected);
            streaming.set(status.streaming);
            paused.set(status.paused);
        }
        if status.changed().await.is_err() {
            return;
        }
    }
}

/// `"ok"` or `"error"`, the `outcome` attribute.
pub fn outcome<T, E>(result: &Result<T, E>) -> KeyValue {
    KeyValue::new("outcome", if result.is_ok() { "ok" } else { "error" })
}
