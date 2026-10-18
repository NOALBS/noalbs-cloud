use tokio::sync::{mpsc, watch};

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

#[derive(Clone)]
pub struct ShutdownHandler {
    /// Used to receive shutdown signal.
    notifier: watch::Receiver<bool>,
    /// Not used directly, but used to let the receiver know that this handler has been dropped.
    _shutdown_waiter: mpsc::Sender<()>,
}

impl ShutdownHandler {
    pub fn new(notifier: watch::Receiver<bool>, _shutdown_waiter: mpsc::Sender<()>) -> Self {
        Self {
            notifier,
            _shutdown_waiter,
        }
    }

    pub async fn changed(&mut self) {
        let _ = self.notifier.changed().await;
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
