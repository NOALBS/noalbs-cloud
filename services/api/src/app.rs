use axum::{Json, extract::FromRef};
use axum_extra::extract::cookie::Key;
use common::shutdown;
use tracing::info;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use crate::{
    config::Config,
    error::Result,
    oauth::OAuthProviders,
    routes::{
        ErrorBody, admin, admin_actions, admin_organizations, admin_status, auth, belabox, billing,
        chat, chat_bots, feed, history, invites, markers, me, members, moblin, my_workspaces,
        notices, organization_audit, organization_ops, organizations, plans, stream_servers,
        support, switchers, workspaces,
    },
};

#[derive(Clone)]
pub struct ApiState {
    pub db: db::DbPool,
    pub nats: async_nats::Client,
    pub http: reqwest::Client,
    pub oauth: OAuthProviders,
    /// Whether `POST /auth/dev-login` is enabled.
    pub dev_login: bool,
    /// Whether stream server tests may reach private addresses.
    pub allow_private_stats_urls: bool,
    /// Encrypts stored secrets such as OBS passwords.
    pub data_key: common::crypto::DataKey,
    pub links: BrowserSourceLinks,
    /// What obs-link writes: statuses (switcher, BELABOX, Moblin) and the recent stats
    /// samples replayed when a live view opens.
    pub stores: live::Stores,
    /// Stripe billing; `None` when it isn't configured.
    pub billing: Option<std::sync::Arc<crate::billing::Billing>>,
    /// Where the dashboard is served (no trailing slash), for Stripe return URLs.
    pub dashboard_url: String,
    /// The chat environment (`db::chat::environment_id`), for chat subjects.
    pub chat_environment: String,
    /// Recent OBS stills, shared between people watching the same switcher.
    pub screenshots: std::sync::Arc<crate::screenshots::Cache>,
    /// A few seconds of memory for the session and permission lookups every request starts with.
    pub auth: std::sync::Arc<crate::auth_cache::AuthCache>,
    /// In-memory rate limits (`crate::rate_limit`).
    pub limits: std::sync::Arc<crate::rate_limit::RateLimits>,
    /// Where the visitor's IP is, behind a proxy (config `client_ip_header`).
    pub client_ip_header: Option<axum::http::HeaderName>,
    cookie_key: Key,
}

impl ApiState {
    /// Tells obs-link that a switcher's settings changed, so it reloads them.
    pub async fn notify_config(&self, switcher_id: uuid::Uuid) {
        if let Err(error) = self
            .nats
            .publish(live::config_subject(switcher_id), "{}".into())
            .await
        {
            tracing::warn!(%error, %switcher_id, "Failed to notify obs-link of a config change");
        }
    }

    /// Tells the chat service that chat links or bots changed, so it subscribes
    /// or unsubscribes channels.
    pub async fn notify_chat(&self) {
        if let Err(error) = self
            .nats
            .publish(
                live::chat_reconcile_subject(&self.chat_environment),
                "{}".into(),
            )
            .await
        {
            tracing::warn!(%error, "Failed to notify the chat service of a link change");
        }
    }

    /// Tells obs-link to reload every switcher in a workspace, e.g. after a plan
    /// change pauses or resumes some.
    pub async fn notify_workspace(&self, workspace_id: uuid::Uuid) {
        // The plan decides which chat bot speaks.
        self.notify_chat().await;
        match db::switchers::list_summaries(&self.db, workspace_id).await {
            Ok(switchers) => {
                for switcher in switchers {
                    self.notify_config(switcher.id).await;
                }
            }
            Err(error) => {
                tracing::warn!(%error, %workspace_id, "Couldn't list switchers to notify")
            }
        }
    }

    /// Remote control of switchers' OBS, BELABOX and Moblin, shared with the chat service.
    pub fn control(&self) -> control::Control {
        control::Control::new(self.db.clone(), self.nats.clone(), &self.stores)
    }

    /// The switcher's live status, or `None` if its browser source isn't connected.
    pub async fn status_of(&self, switcher_id: uuid::Uuid) -> Option<live::SwitcherStatus> {
        self.control().status(switcher_id).await
    }
}

/// Builds the URL a streamer adds to OBS as a browser source. The page finds
/// obs-link itself, at `/link` on its own host.
#[derive(Clone)]
pub struct BrowserSourceLinks {
    page: String,
}

impl BrowserSourceLinks {
    pub fn new(page: &str) -> Self {
        Self {
            page: page.to_owned(),
        }
    }

    /// The page URL with the connect token in the fragment, which browsers never
    /// send to a server.
    pub fn url(&self, connect_token: &str) -> String {
        let mut url =
            reqwest::Url::parse(&self.page).expect("browser_source_url is validated at startup");
        url.set_fragment(Some(connect_token));
        url.into()
    }

    /// The Assistant URL a switcher's Moblin connects to: obs-link at
    /// `/moblin/<switcher ID>` on the browser source's host, over `ws`/`wss`.
    pub fn moblin_url(&self, switcher_id: uuid::Uuid) -> String {
        let page =
            reqwest::Url::parse(&self.page).expect("browser_source_url is validated at startup");
        let scheme = if page.scheme() == "https" {
            "wss"
        } else {
            "ws"
        };
        let host = page.host_str().unwrap_or("localhost");
        match page.port() {
            Some(port) => format!("{scheme}://{host}:{port}/moblin/{switcher_id}"),
            None => format!("{scheme}://{host}/moblin/{switcher_id}"),
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "NOALBS Cloud API",
        description = "API behind the NOALBS Cloud dashboard."
    ),
    components(schemas(
        ErrorBody,
        // Sent over the live feed (server-sent events), not as JSON responses.
        live::SwitcherStatus,
        live::StatsSample,
        live::SwitcherEvent,
        live::BelaboxStatus,
        live::MoblinStatus,
        live::AudioUpdate,
    ))
)]
struct ApiDoc;

/// Every API route, with the OpenAPI description collected from the handlers.
fn api_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(auth::router())
        .merge(me::router())
        .merge(admin::router())
        .merge(admin_actions::router())
        .merge(admin_organizations::router())
        .merge(admin_status::router())
        .merge(notices::router())
        .merge(workspaces::router())
        .merge(my_workspaces::router())
        .merge(members::router())
        .merge(invites::router())
        .merge(organizations::router())
        .merge(organization_ops::router())
        .merge(organization_audit::router())
        .merge(plans::router())
        .merge(billing::router())
        .merge(switchers::router())
        .merge(stream_servers::router())
        .merge(chat::router())
        .merge(chat_bots::router())
        .merge(feed::router())
        .merge(belabox::router())
        .merge(moblin::router())
        .merge(history::router())
        .merge(markers::router())
        .merge(support::router())
}

/// The OpenAPI document for the API, e.g. for generating the dashboard's types.
pub fn openapi() -> utoipa::openapi::OpenApi {
    api_router().into_openapi()
}

impl FromRef<ApiState> for Key {
    fn from_ref(state: &ApiState) -> Self {
        state.cookie_key.clone()
    }
}

pub async fn run(config: Config, mut shutdown_handle: shutdown::ShutdownHandler) -> Result<()> {
    tracing::info!(host = %config.host, port = config.port, "Running API");

    let db = db::create_pool_with_options(&db::DbConfig {
        database_url: &config.postgres_url,
        max_connections: config.postgres_max_connections,
    })
    .await?;
    let nats = async_nats::ConnectOptions::new()
        .request_timeout(Some(tokio::time::Duration::from_millis(
            config.nats_request_timeout,
        )))
        .connect(&config.nats_url)
        .await?;

    let js = async_nats::jetstream::new(nats.clone());
    let stores = live::Stores::open(&js)
        .await
        .map_err(|error| crate::error::Error::Nats(error.to_string()))?;

    history::spawn_upkeep(db.clone(), shutdown_handle.clone());
    crate::privacy::spawn_upkeep(db.clone(), shutdown_handle.clone());
    let billing = billing(&config);

    let chat_environment = db::chat::environment_id(&db).await?;
    let state = ApiState {
        db,
        nats,
        stores,
        http: reqwest::Client::new(),
        oauth: OAuthProviders::from_config(&config),
        dev_login: config.dev_login,
        allow_private_stats_urls: config.allow_private_stats_urls,
        data_key: common::crypto::DataKey::from_hex(&config.data_encryption_key).map_err(
            |error| config::ConfigError::Message(format!("data_encryption_key: {error}")),
        )?,
        links: browser_source_links(&config)?,
        billing,
        dashboard_url: dashboard_url(&config)?,
        chat_environment,
        screenshots: std::sync::Arc::default(),
        auth: std::sync::Arc::new(crate::auth_cache::AuthCache::new()),
        limits: std::sync::Arc::default(),
        client_ip_header: client_ip_header(&config)?,
        cookie_key: private_cookie_key(&config.cookie_secret)?,
    };

    if config.dev_login {
        tracing::warn!("Dev login is enabled; never enable this in production");
    }

    billing::spawn_upkeep(state.clone(), shutdown_handle.clone());
    crate::rate_limit::spawn_upkeep(state.limits.clone(), shutdown_handle.clone());

    let (router, openapi) = api_router().split_for_parts();
    let app = router
        .route("/", axum::routing::get(root))
        .route(
            "/openapi.json",
            axum::routing::get(move || async move { Json(openapi) }),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::rate_limit::limit_requests,
        ))
        // Outermost, so rate-limited requests are measured too.
        .layer(axum::middleware::from_fn(
            crate::request_spans::trace_requests,
        ))
        .with_state(state);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_handle.changed().await;
        info!("Shutdown signal received. Stopping API server...");
    })
    .await?;

    Ok(())
}

async fn root() -> &'static str {
    ""
}

fn client_ip_header(config: &Config) -> Result<Option<axum::http::HeaderName>> {
    let Some(name) = config.client_ip_header.as_deref() else {
        return Ok(None);
    };
    let name = axum::http::HeaderName::try_from(name).map_err(|error| {
        config::ConfigError::Message(format!("client_ip_header is not a header name: {error}"))
    })?;
    Ok(Some(name))
}

fn browser_source_links(config: &Config) -> Result<BrowserSourceLinks> {
    reqwest::Url::parse(&config.browser_source_url).map_err(|error| {
        config::ConfigError::Message(format!("browser_source_url is not a URL: {error}"))
    })?;
    Ok(BrowserSourceLinks::new(&config.browser_source_url))
}

fn private_cookie_key(cookie_secret: &str) -> Result<Key> {
    if cookie_secret.len() != 64 || !cookie_secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(config::ConfigError::Message(
            "cookie_secret must be exactly 64 hexadecimal characters".into(),
        )
        .into());
    }

    // The configuration value is a 32-byte secret encoded as hexadecimal. Key
    // expansion derives the 64-byte key required for private cookie encryption.
    Ok(Key::derive_from(cookie_secret.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::private_cookie_key;

    #[test]
    fn private_cookie_key_requires_a_32_byte_hex_secret() {
        assert!(private_cookie_key(&"a".repeat(64)).is_ok());
        assert!(private_cookie_key("not-a-valid-cookie-secret").is_err());
    }
}

#[cfg(test)]
mod link_tests {
    use super::BrowserSourceLinks;

    #[test]
    fn browser_source_url_carries_only_the_token() {
        let links = BrowserSourceLinks::new("https://app.noalbs.com/source/");
        assert_eq!(
            links.url("abc_123"),
            "https://app.noalbs.com/source/#abc_123"
        );
    }

    #[test]
    fn moblin_url_is_obs_link_on_the_same_host() {
        let id = uuid::Uuid::nil();
        let links = BrowserSourceLinks::new("https://app.noalbs.com/source/");
        assert_eq!(
            links.moblin_url(id),
            format!("wss://app.noalbs.com/moblin/{id}")
        );
        let dev = BrowserSourceLinks::new("http://192.168.1.20:5173/source/");
        assert_eq!(
            dev.moblin_url(id),
            format!("ws://192.168.1.20:5173/moblin/{id}")
        );
    }
}

fn dashboard_url(config: &Config) -> Result<String> {
    reqwest::Url::parse(&config.dashboard_url).map_err(|error| {
        config::ConfigError::Message(format!("dashboard_url is not a URL: {error}"))
    })?;
    Ok(config.dashboard_url.trim_end_matches('/').to_owned())
}

/// Stripe billing, when configured.
fn billing(config: &Config) -> Option<std::sync::Arc<crate::billing::Billing>> {
    let Some(stripe) = &config.stripe else {
        tracing::info!("Stripe isn't configured; billing is off");
        return None;
    };
    Some(std::sync::Arc::new(crate::billing::Billing::new(
        reqwest::Client::new(),
        stripe,
    )))
}
