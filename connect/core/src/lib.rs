pub mod config;
pub mod credentials;
pub mod logs;
pub mod pairing;
pub mod programs;
pub mod relay;
pub mod supervisor;
pub mod url_security;
pub mod watchdog;

pub use config::{AppConfig, AppSettings, ConfigStore, HostPlatform, ObsLaunchConfig, Setup};
pub use pairing::{PairingClient, PairingCreated, PairingResult, PairingStatus};

pub fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

#[cfg(test)]
mod crypto_provider_tests {
    use std::time::Duration;

    use tokio_tungstenite::tungstenite::Error;

    #[test]
    fn installs_crypto_provider_idempotently() {
        super::install_crypto_provider();
        super::install_crypto_provider();

        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
        let _ = rustls::ClientConfig::builder();
    }

    #[tokio::test]
    #[ignore = "requires external network access"]
    async fn wss_connect_has_no_tls_or_io_error() {
        super::install_crypto_provider();
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            tokio_tungstenite::connect_async("wss://noalbs.com/link"),
        )
        .await
        .expect("WSS connection timed out");

        assert!(
            !matches!(&result, Err(Error::Tls(_) | Error::Io(_))),
            "WSS connection returned a TLS or IO error: {result:?}"
        );
    }
}
