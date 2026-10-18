use crate::error::Result;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// Hostname or IP address to bind the API server to.
    pub host: String,

    /// Port number to bind the API server to.
    pub port: u16,

    /// NATS server URL for connecting to the NATS message broker.
    pub nats_url: String,

    /// Timeout in milliseconds for NATS requests. This is the maximum time the
    /// API will wait for a response from NATS before considering the request
    /// failed.
    pub nats_request_timeout: u64,

    /// PostgreSQL connection URL for connecting to the database.
    pub postgres_url: String,

    /// Maximum number of connections in the PostgreSQL connection pool.
    pub postgres_max_connections: u32,

    /// Public base URL for the API.
    /// Example: https://api.noalbs.com
    pub base_url: String,

    /// Hex-encoded 32-byte secret used to derive the private-cookie encryption key.
    /// Must be exactly 64 hex characters (32 bytes).
    /// Example: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    /// This key should be kept secret and not checked into version control. It is recommended to generate a new key for each deployment.
    /// You can generate a new key using the following command:
    /// ```bash
    /// openssl rand -hex 32
    /// ```
    pub cookie_secret: String,

    /// Hex-encoded 32-byte key used for encrypting sensitive data in the database.
    /// Must be exactly 64 hex characters (32 bytes).
    /// Example: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    /// This key should be kept secret and not checked into version control. It is recommended to generate a new key for each deployment.
    /// You can generate a new key using the following command:
    /// ```bash
    /// openssl rand -hex 32
    /// ```
    pub data_encryption_key: String,

    /// Optional OAuth configurations for different platforms.
    pub oauth_twitch: Option<OAuthConfig>,
    pub oauth_kick: Option<OAuthConfig>,
    pub oauth_youtube: Option<OAuthConfig>,

    /// Stripe billing. Without it, plans can only be changed by staff and the
    /// billing endpoints answer `billing_disabled`.
    pub stripe: Option<StripeConfig>,

    /// Where the dashboard is served, for Stripe Checkout and portal return URLs.
    pub dashboard_url: String,

    /// Enables `POST /auth/dev-login`, which signs in any username without
    /// OAuth. Only for local development and tests; never enable in production.
    pub dev_login: bool,

    /// Lets "Test connection" on stream servers reach private and loopback
    /// addresses. Only for local development; keep it off in production.
    pub allow_private_stats_urls: bool,

    /// Where the OBS browser source page is served: the dashboard serves it at
    /// `/source/`. The page connects to obs-link at `/link` on the same host.
    pub browser_source_url: String,

    /// The header our proxy puts the visitor's IP in, for rate limits:
    /// `cf-connecting-ip` behind Cloudflare, `x-forwarded-for` behind Caddy alone.
    /// Unset: the connection's address (fine when nothing sits in front). Only set
    /// it when clients can't reach the API around that proxy, or they pick their IP.
    pub client_ip_header: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 12000,
            nats_url: "nats://localhost:4222".into(),
            nats_request_timeout: 5000,
            postgres_url: "postgresql://postgres:postgres@localhost/localdb".into(),
            postgres_max_connections: 10,
            base_url: "http://localhost:12000".into(),
            cookie_secret: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .into(),
            data_encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .into(),
            oauth_twitch: None,
            oauth_kick: None,
            oauth_youtube: None,
            stripe: None,
            dashboard_url: "http://localhost:5173".into(),
            dev_login: false,
            allow_private_stats_urls: false,
            browser_source_url: "http://localhost:5173/source/".into(),
            client_ip_header: None,
        }
    }
}

/// Stripe keys. Products and prices are managed in the admin section and kept in
/// the database (a test-mode key needs a database of its own: price IDs differ).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct StripeConfig {
    /// `sk_live_…` / `sk_test_…` (or a restricted key with the calls we make).
    pub secret_key: String,
    /// `whsec_…`, from the webhook endpoint (or `stripe listen` locally).
    pub webhook_secret: String,
    /// Stripe's API; tests point it at a fake.
    #[serde(default = "default_stripe_api_base")]
    pub api_base: String,
}

fn default_stripe_api_base() -> String {
    "https://api.stripe.com".into()
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    /// The platform's sign-in and API addresses (`https://id.twitch.tv`,
    /// `https://api.twitch.tv`; Kick: `https://id.kick.com`, `https://api.kick.com`;
    /// YouTube: Google's authorization and token endpoints both under `auth_base`,
    /// and `https://www.googleapis.com`). Only for tests against a fake.
    #[serde(default)]
    pub auth_base: Option<String>,
    #[serde(default)]
    pub api_base: Option<String>,
    /// YouTube only: a Google API key, to look channels up by handle (ignored
    /// chatters). Google has no app tokens.
    #[serde(default)]
    pub api_key: Option<String>,
}

/// `config.toml` (optional) plus `API_*` environment variables; see `common::settings`.
pub fn load_config() -> Result<Config> {
    let config: Config = common::settings::load("API")?;
    common::settings::require_real_key("cookie_secret", &config.cookie_secret)?;
    common::settings::require_real_key("data_encryption_key", &config.data_encryption_key)?;

    Ok(config)
}
