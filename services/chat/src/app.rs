use std::{future::IntoFuture, sync::Arc, time::Duration};

use async_nats::jetstream;
use axum::{Router, routing::get, routing::post};
use common::shutdown::ShutdownHandler;

use crate::{
    config::Config,
    error::{Error, Result},
    events, handler,
    kick::{self, Kick},
    kv::{self, Buckets},
    notify, reconcile, tokens,
    twitch::{self, Twitch},
    youtube::{self, YouTube},
};

/// Everything the tasks share; cheap to clone.
pub struct Shared {
    pub db: db::DbPool,
    pub nats: async_nats::Client,
    pub buckets: Buckets,
    /// What every message looks up, kept briefly (see [`crate::cache`]).
    pub cache: crate::cache::Cache,
    /// This database's chat environment: in every subject, queue group and
    /// coordination key, so environments sharing a NATS server stay apart.
    pub environment: String,
    /// Names this instance in shard claims and leases.
    pub instance_id: String,
    pub data_key: common::crypto::DataKey,
    pub control: control::Control,
    /// Recent stats samples, for `!bitrate`.
    pub stats: jetstream::stream::Stream,
    pub twitch: Option<Arc<Twitch>>,
    pub kick: Option<Arc<Kick>>,
    pub youtube: Option<Arc<YouTube>>,
}

impl Shared {
    /// A chat subject of this environment.
    pub fn subject(&self, name: &str) -> String {
        live::chat_subject(&self.environment, name)
    }

    /// A queue group of this environment.
    pub fn queue(&self, name: &str) -> String {
        format!("{name}-{}", self.environment)
    }

    /// A key in the shared buckets that belongs to this environment.
    pub fn key(&self, parts: &[&str]) -> String {
        let mut all = vec![self.environment.as_str()];
        all.extend_from_slice(parts);
        kv::key(&all)
    }

    /// Asks for a reconcile (links, bots or raid settings changed).
    pub async fn request_reconcile(&self) {
        if let Err(error) = self
            .nats
            .publish(live::chat_reconcile_subject(&self.environment), "{}".into())
            .await
        {
            tracing::warn!(%error, "Couldn't ask for a reconcile");
        }
    }
}

pub type State = Arc<Shared>;

pub async fn run(config: Config, mut shutdown: ShutdownHandler) -> Result<()> {
    // Twitch's WebSocket TLS, YouTube's gRPC and reqwest all use rustls; pick
    // its provider once.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let db = db::create_pool_with_options(&db::DbConfig {
        database_url: &config.postgres_url,
        max_connections: config.postgres_max_connections,
    })
    .await?;
    let nats = async_nats::ConnectOptions::new()
        .request_timeout(Some(Duration::from_millis(config.nats_request_timeout)))
        .connect(&config.nats_url)
        .await
        .map_err(Error::nats)?;
    let js = jetstream::new(nats.clone());
    let buckets = Buckets::open(&js).await?;
    let stores = live::Stores::open(&js).await.map_err(Error::nats)?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let instance_id = config
        .instance_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()[..12].to_owned());

    let environment = db::chat::environment_id(&db).await?;
    let state: State = Arc::new(Shared {
        environment,
        control: control::Control::new(db.clone(), nats.clone(), &stores),
        db,
        nats,
        buckets,
        cache: crate::cache::Cache::new(),
        instance_id: instance_id.clone(),
        data_key: common::crypto::DataKey::from_hex(&config.data_encryption_key)?,
        stats: stores.stats,
        twitch: config
            .twitch
            .clone()
            .map(|twitch| Arc::new(Twitch::new(twitch, http.clone()))),
        kick: config
            .kick
            .clone()
            .map(|kick| Arc::new(Kick::new(kick, http.clone()))),
        youtube: match config.youtube.clone() {
            Some(youtube) => Some(Arc::new(YouTube::new(youtube, http.clone())?)),
            None => None,
        },
    });
    tracing::info!(
        instance = instance_id,
        environment = state.environment,
        twitch = state.twitch.is_some(),
        kick = state.kick.is_some(),
        youtube = state.youtube.is_some(),
        "Chat service ready"
    );
    if state.twitch.is_none() && state.kick.is_none() && state.youtube.is_none() {
        tracing::warn!("No chat platform is configured: nothing to do");
    }

    let tasks = [
        tokio::spawn(twitch::shards::start(state.clone())),
        tokio::spawn(handler::run(state.clone())),
        tokio::spawn(reconcile::run(state.clone())),
        tokio::spawn(events::run(state.clone())),
        tokio::spawn(notify::run(state.clone())),
        tokio::spawn(tokens::keep_alive(state.clone())),
    ];
    let kick_raids = tokio::spawn(kick::raids::run(state.clone()));
    let youtube_chats = tokio::spawn(youtube::watcher::run(state.clone()));

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/chat/webhooks/kick", post(kick::webhook::handle))
        .with_state(state.clone());
    let listener =
        tokio::net::TcpListener::bind(format!("{}:{}", config.host, config.port)).await?;
    tracing::info!(host = %config.host, port = config.port, "Listening");
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .into_future(),
    );
    // `shutdown` is held until the cleanup below is done: the process exits as
    // soon as it is dropped.
    shutdown.changed().await;
    let _ = stop.send(());
    for task in tasks {
        task.abort();
    }
    // Wait for the lease holders to stop (they can't renew their leases after this).
    kick_raids.abort();
    youtube_chats.abort();
    let _ = kick_raids.await;
    let _ = youtube_chats.await;
    // Hand the shards, the Kick raids and the YouTube chats over now rather than
    // when the claims lapse.
    twitch::shards::release_all(&state).await;
    kick::raids::release(&state).await;
    youtube::watcher::release(&state).await;
    let _ = state
        .buckets
        .instances
        .delete(state.key(&[&state.instance_id]))
        .await;
    match server.await {
        Ok(result) => result?,
        Err(error) => tracing::warn!(%error, "The HTTP server task failed"),
    }
    drop(shutdown);
    Ok(())
}
