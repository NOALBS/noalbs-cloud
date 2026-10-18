use crate::error::Result;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub host: String,
    pub port: u16,

    pub nats_url: String,
    pub nats_request_timeout: u64,

    pub postgres_url: String,
    pub postgres_max_connections: u32,

    /// Hex-encoded 32-byte key that decrypts stored OBS passwords and BELABOX
    /// remote keys. Must match the API's `data_encryption_key`.
    pub data_encryption_key: String,

    /// BELABOX Cloud's endpoint for remote clients (the fake in
    /// `examples/fake_belabox.rs` for local tests).
    pub belabox_url: String,

    /// Lets stream servers' stats URLs point at private addresses, which are
    /// refused otherwise (local development only). Matches the API's setting.
    pub allow_private_stats_urls: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 11000,
            nats_url: "nats://localhost:4222".into(),
            nats_request_timeout: 5000,
            postgres_url: "postgres://postgres:postgres@localhost/localdb".into(),
            postgres_max_connections: 10,
            data_encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .into(),
            belabox_url: "wss://remote.belabox.net/ws/remote".into(),
            allow_private_stats_urls: false,
        }
    }
}

/// `config.toml` (optional) plus `OBS_LINK_*` environment variables; see `common::settings`.
pub fn load_config() -> Result<Config> {
    let config: Config = common::settings::load("OBS_LINK")?;
    common::settings::require_real_key("data_encryption_key", &config.data_encryption_key)?;

    Ok(config)
}
