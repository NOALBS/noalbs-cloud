use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, mpsc, watch},
    time::Instant,
};

#[derive(Clone, Default)]
pub struct Readiness(Arc<ReadinessState>);

#[derive(Default)]
struct ReadinessState {
    phase: AtomicU8,
    dependency: Mutex<Option<(Instant, bool)>>,
}

impl Readiness {
    pub fn initialized(&self) {
        let _ = self
            .0
            .phase
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst);
    }
    pub fn begin_draining(&self) {
        self.0.phase.store(2, Ordering::SeqCst);
    }
    pub fn is_ready(&self) -> bool {
        self.0.phase.load(Ordering::SeqCst) == 1
    }

    pub async fn check(&self, connected: bool, database: impl Future<Output = bool>) -> bool {
        if !self.is_ready() || !connected {
            return false;
        }
        let mut cached = self.0.dependency.lock().await;
        if !self.is_ready() {
            return false;
        }
        if let Some((checked, result)) = *cached
            && checked.elapsed() < Duration::from_secs(1)
        {
            return result;
        }
        let result = matches!(
            tokio::time::timeout(Duration::from_millis(500), database).await,
            Ok(true)
        );
        *cached = Some((Instant::now(), result));
        result && self.is_ready()
    }
}

/// Leaves time for EndpointSlice/proxy updates before stopping listeners.
pub async fn run_until_stopped<E: std::fmt::Display>(
    manager: ShutdownManager,
    mut app: tokio::task::JoinHandle<Result<(), E>>,
) -> bool {
    tokio::select! {
        () = stop_requested() => {}
        ended = &mut app => {
            return match ended {
                Ok(Ok(())) => false,
                Ok(Err(error)) => { tracing::error!(%error, "Application failed"); true }
                Err(error) => { tracing::error!(%error, "Application task failed"); true }
            };
        }
    }
    manager.shutdown_handler.readiness.begin_draining();
    let delay = std::env::var("DRAIN_DELAY_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5)
        .min(10);
    tracing::info!(
        delay,
        "Draining: readiness disabled; waiting for endpoint removal"
    );
    tokio::time::sleep(Duration::from_secs(delay)).await;
    let drained = tokio::time::timeout(Duration::from_secs(25), async {
        let (_, ended) = tokio::join!(manager.shutdown(), &mut app);
        ended
    })
    .await;
    match drained {
        Ok(Ok(Ok(()))) => {
            tracing::info!("Drain completed");
            false
        }
        Ok(Ok(Err(error))) => {
            tracing::error!(%error, "Application failed during drain");
            true
        }
        Ok(Err(error)) => {
            tracing::error!(%error, "Application task failed during drain");
            true
        }
        Err(_) => {
            app.abort();
            tracing::warn!("Drain deadline reached; stopping remaining work");
            false
        }
    }
}

pub struct ShutdownManager {
    // Used to notify all listeners to shutdown.
    notifier: watch::Sender<bool>,
    /// Used to wait for all listeners to shutdown.
    shutdown_rx: mpsc::Receiver<()>,
    /// Used to get a handle to the shutdown handler.
    shutdown_handler: ShutdownHandler,
}

impl ShutdownManager {
    pub fn new() -> Self {
        let (cancellation_tx, cancellation_rx) = watch::channel(false);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let shutdown_handler = ShutdownHandler::new(cancellation_rx, shutdown_tx);

        Self {
            notifier: cancellation_tx,
            shutdown_rx,
            shutdown_handler,
        }
    }

    /// Get a handle to the shutdown handler to be waited on.
    pub fn handle(&self) -> ShutdownHandler {
        self.shutdown_handler.clone()
    }

    pub async fn shutdown(mut self) {
        let _ = self.notifier.send(true);
        drop(self.shutdown_handler);
        let _ = self.shutdown_rx.recv().await;
    }

    /// Shutdown, waiting a maximum amount of time before returning.
    pub async fn shutdown_with_timeout(self, timeout: u64) {
        let _ =
            tokio::time::timeout(tokio::time::Duration::from_secs(timeout), self.shutdown()).await;
    }
}

impl Default for ShutdownManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn readiness_is_shared_and_draining_is_terminal() {
        let ready = Readiness::default();
        let clone = ready.clone();
        assert!(!ready.is_ready());
        ready.initialized();
        assert!(clone.check(true, async { true }).await);
        assert!(
            !clone
                .check(false, async {
                    panic!("disconnected dependency was polled")
                })
                .await
        );
        let unhealthy = Readiness::default();
        unhealthy.initialized();
        assert!(!unhealthy.check(true, async { false }).await);
        clone.begin_draining();
        assert!(!ready.check(true, async { true }).await);
        ready.initialized();
        assert!(!ready.is_ready());
    }

    #[tokio::test]
    async fn draining_during_startup_cannot_become_ready() {
        let ready = Readiness::default();
        ready.begin_draining();
        ready.initialized();
        assert!(!ready.is_ready());
    }

    #[tokio::test]
    async fn readiness_coalesces_concurrent_checks_and_expires_cached_results() {
        let ready = Readiness::default();
        ready.initialized();
        let clone = ready.clone();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let check = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            true
        };
        assert_eq!(
            tokio::join!(ready.check(true, check()), clone.check(true, check())),
            (true, true)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(1010)).await;
        assert!(!ready.check(true, async { false }).await);
        assert!(
            !clone
                .check(true, async { panic!("cached failure was not reused") })
                .await
        );
        ready.begin_draining();
        assert!(
            !clone
                .check(true, async {
                    panic!("draining readiness checked a dependency")
                })
                .await
        );
    }

    #[tokio::test]
    async fn readiness_times_out_and_caches_a_stalled_dependency() {
        let ready = Readiness::default();
        assert!(
            !ready
                .check(true, async {
                    panic!("uninitialized readiness checked a dependency")
                })
                .await
        );
        ready.initialized();
        assert!(!ready.check(true, std::future::pending()).await);
        assert!(
            !ready
                .check(true, async { panic!("cached timeout was not reused") })
                .await
        );
    }

    #[tokio::test]
    async fn cloned_handler_observes_an_already_requested_shutdown() {
        let manager = ShutdownManager::new();
        let mut handler = manager.handle();
        let stopping = tokio::spawn(manager.shutdown());
        handler.changed().await;
        let mut clone = handler.clone();
        clone.changed().await;
        drop(clone);
        drop(handler);
        stopping.await.unwrap();
    }
}

#[derive(Clone)]
pub struct ShutdownHandler {
    readiness: Readiness,
    /// Used to receive shutdown signal.
    notifier: watch::Receiver<bool>,
    /// Not used directly, but used to let the receiver know that this handler has been dropped.
    _shutdown_waiter: mpsc::Sender<()>,
}

impl ShutdownHandler {
    pub fn new(notifier: watch::Receiver<bool>, _shutdown_waiter: mpsc::Sender<()>) -> Self {
        Self {
            readiness: Readiness::default(),
            notifier,
            _shutdown_waiter,
        }
    }

    pub async fn changed(&mut self) {
        let _ = self.notifier.wait_for(|stop| *stop).await;
    }

    pub fn readiness(&self) -> Readiness {
        self.readiness.clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.notifier.clone()
    }

    pub async fn is_shutdown(&mut self) -> bool {
        *self.notifier.borrow()
    }
}

/// Resolves once the process is asked to stop: Ctrl+C, or SIGTERM (Docker, Kubernetes, systemd);
/// Ctrl+Break on Windows. Each service then stops through its [`ShutdownManager`].
pub async fn stop_requested() {
    tokio::select! {
        () = ctrl_c() => {}
        () = terminate() => {}
    }
}

/// Ctrl+C (SIGINT on Unix). Never resolves if it can't be listened for.
async fn ctrl_c() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::warn!(%error, "Can't listen for Ctrl+C");
        std::future::pending::<()>().await;
    }
}

/// The platform's request to stop. Never resolves if it can't be listened for.
#[cfg(unix)]
async fn terminate() {
    use tokio::signal::unix::{SignalKind, signal};

    match signal(SignalKind::terminate()) {
        Ok(mut sigterm) => {
            sigterm.recv().await;
        }
        Err(error) => {
            tracing::warn!(%error, "Can't listen for SIGTERM");
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(windows)]
async fn terminate() {
    match tokio::signal::windows::ctrl_break() {
        Ok(mut ctrl_break) => {
            ctrl_break.recv().await;
        }
        Err(error) => {
            tracing::warn!(%error, "Can't listen for Ctrl+Break");
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(not(any(unix, windows)))]
async fn terminate() {
    std::future::pending::<()>().await;
}
