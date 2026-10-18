//! A fake Twitch, Kick and YouTube for trying the chat service locally, no accounts needed:
//!
//!     cargo run -p chat --example fake_platforms            # listens on :18100
//!
//! Point the chat service at it:
//!
//!     CHAT_TWITCH__API_BASE=http://localhost:18100  CHAT_TWITCH__AUTH_BASE=http://localhost:18100
//!     CHAT_TWITCH__EVENTSUB_URL=ws://localhost:18100/ws
//!     CHAT_KICK__API_BASE=http://localhost:18100    CHAT_KICK__AUTH_BASE=http://localhost:18100
//!     CHAT_KICK__WEB_BASE=http://localhost:18100    CHAT_KICK__PUSHER_URL=ws://localhost:18100/app/fake
//!     CHAT_YOUTUBE__API_BASE=http://localhost:18100 CHAT_YOUTUBE__AUTH_BASE=http://localhost:18100
//!     CHAT_YOUTUBE__GRPC_URL=http://localhost:18100 CHAT_YOUTUBE__API_KEY=any
//!
//! and the API's sign-in too, so linking channels and bots works without real apps:
//!
//!     API_OAUTH_TWITCH__AUTH_BASE=http://localhost:18100  API_OAUTH_TWITCH__API_BASE=http://localhost:18100
//!     API_OAUTH_KICK__AUTH_BASE=http://localhost:18100    API_OAUTH_KICK__API_BASE=http://localhost:18100
//!     API_OAUTH_YOUTUBE__AUTH_BASE=http://localhost:18100 API_OAUTH_YOUTUBE__API_BASE=http://localhost:18100
//!     API_OAUTH_YOUTUBE__API_KEY=any
//!
//! (any client ID and secret will do). Every sign-in comes back as the account
//! set with `/control/next` (by default id `100`, `kate`), granting what was asked.
//!
//! It serves Helix (conduits, shards, EventSub subscriptions, chat messages) and an
//! EventSub WebSocket, and Kick's API (subscriptions, chat, its webhook key). Kick
//! webhooks are signed with a key made at start; `GET /public/v1/public-key` hands
//! out its public half, like Kick does.
//! For Kick raids it fakes Kick's site too: `GET /api/v2/channels/{slug}` (channel ID =
//! user ID + 1000, chatroom ID = user ID + 2000) and its Pusher WebSocket at `/app/{key}`.
//! For YouTube: Google's `/o/oauth2/v2/auth` and `/token`, the Data API's channels,
//! liveBroadcasts and liveChat/messages, and the `StreamList` gRPC stream (plaintext HTTP/2 on the
//! same port). Use YouTube-like IDs with `/control/next`, e.g. `{"id":"UCkate","login":"kate"}`.
//!
//! Control:
//! - `POST /control/twitch/chat {"channel":"100","chatter":"55","login":"bob","text":"!bitrate","moderator":false,"vip":false,"subscriber":false}`
//!   (badges optional, also on Kick) sends a chat message through the conduit (to one enabled shard) for every
//!   subscription on that channel.
//! - `POST /control/twitch/reconnect` asks every session to reconnect.
//! - `POST /control/twitch/raid {"from":"100","from_name":"Kate","to":"300","to_name":"Bob","viewers":12}`
//!   raids, delivered to the `channel.raid` subscriptions of either side.
//! - `POST /control/twitch/revoke {"user_id":"100"}`: that user took our app's access away.
//! - `POST /control/next {"id":"100","login":"kate","grant":["user:read"]}`: who the next
//!   sign-in is (`grant` optional: fewer scopes than asked); `/control/account {"id","login"}`
//!   only makes a name known (for user lookups); `/control/deny` refuses the next sign-in.
//! - `POST /control/kick/chat {"channel":"200","chatter":"77","login":"bob","text":"!bitrate","targets":["http://localhost:13000/chat/webhooks/kick"],"bad_signature":false}`
//!   sends one signed webhook to each target (the same delivery: for de-dup tests).
//! - `POST /control/kick/raid {"from":"200","to_name":"Bob","to_slug":"bob","on":"channel"}` raids out of
//!   Kick channel 200 over the site WebSocket, on its `channel` topic (default), its `chatroom` one or `both`.
//! - `POST /control/youtube/live {"channel":"UCkate","live":true}` starts a broadcast with a new live
//!   chat (`false` ends it: open streams get `offlineAt`, new ones `FAILED_PRECONDITION`).
//! - `POST /control/youtube/chat {"channel":"UCkate","chatter":"UCbob","name":"bob","text":"!bitrate","moderator":false,"member":false}`
//!   says something in the channel's live chat (the owner when `chatter` is the channel).
//! - `GET /control/state`: conduits, shards, subscriptions and everything the bots said.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use aws_lc_rs::{
    encoding::AsDer,
    rand::SystemRandom,
    rsa::{KeyPair, KeySize},
    signature::{KeyPair as _, RSA_PKCS1_SHA256},
};
use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Default)]
struct Twitch {
    conduits: HashMap<String, Vec<Option<String>>>,
    sessions: HashMap<String, mpsc::UnboundedSender<String>>,
    subscriptions: Vec<Value>,
    sent: Vec<Value>,
}

#[derive(Default)]
struct Kick {
    subscriptions: Vec<Value>,
    sent: Vec<Value>,
    /// Site WebSocket connections: their topics and a way to push frames.
    pusher: HashMap<u64, (Vec<String>, mpsc::UnboundedSender<String>)>,
}

struct Fake {
    twitch: Mutex<Twitch>,
    kick: Mutex<Kick>,
    youtube: Mutex<YouTube>,
    accounts: Mutex<Accounts>,
    key: KeyPair,
    next: AtomicU64,
    http: reqwest::Client,
    port: u16,
}

type AppState = Arc<Fake>;

impl Fake {
    fn id(&self, prefix: &str) -> String {
        format!("{prefix}{}", self.next.fetch_add(1, Ordering::Relaxed))
    }

    fn public_key_pem(&self) -> String {
        let der = self.key.public_key().as_der().expect("DER");
        format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(der.as_ref())
        )
    }
}

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(18100);
    let state = Arc::new(Fake {
        twitch: Mutex::default(),
        kick: Mutex::default(),
        youtube: Mutex::default(),
        accounts: Mutex::default(),
        key: KeyPair::generate(KeySize::Rsa2048).expect("an RSA key"),
        next: AtomicU64::new(1),
        http: reqwest::Client::new(),
        port,
    });

    let app = Router::new()
        // Every platform's tokens (client credentials and refreshes).
        .route("/oauth2/token", post(token))
        .route("/oauth/token", post(token))
        .route("/token", post(token))
        .route("/oauth2/authorize", get(authorize))
        .route("/oauth/authorize", get(authorize))
        .route("/o/oauth2/v2/auth", get(authorize))
        .route("/helix/users", get(helix_users))
        .route("/public/v1/users", get(kick_users))
        .route("/public/v1/channels", get(kick_channels))
        .route(
            "/helix/eventsub/conduits",
            get(conduits)
                .post(create_conduit)
                .patch(resize_conduit)
                .delete(delete_conduit),
        )
        .route(
            "/helix/eventsub/conduits/shards",
            get(conduit_shards).patch(assign_shards),
        )
        .route(
            "/helix/eventsub/subscriptions",
            get(twitch_subscriptions)
                .post(twitch_subscribe)
                .delete(twitch_unsubscribe),
        )
        .route("/helix/chat/messages", post(twitch_send))
        .route("/ws", get(eventsub_socket))
        .route("/public/v1/public-key", get(kick_key))
        .route(
            "/public/v1/events/subscriptions",
            get(kick_subscriptions)
                .post(kick_subscribe)
                .delete(kick_unsubscribe),
        )
        .route("/public/v1/chat", post(kick_send))
        .route("/api/v2/channels/{slug}", get(kick_site_channel))
        .route("/app/{key}", get(kick_pusher_socket))
        .route("/control/twitch/chat", post(control_twitch_chat))
        .route("/control/twitch/reconnect", post(control_twitch_reconnect))
        .route("/control/twitch/raid", post(control_twitch_raid))
        .route("/control/twitch/revoke", post(control_twitch_revoke))
        .route("/control/account", post(control_account))
        .route("/control/next", post(control_next))
        .route("/control/deny", post(control_deny))
        .route("/control/kick/chat", post(control_kick_chat))
        .route("/control/kick/raid", post(control_kick_raid))
        .route("/youtube/v3/channels", get(youtube_channels))
        .route("/youtube/v3/liveBroadcasts", get(youtube_broadcasts))
        .route("/youtube/v3/liveChat/messages", post(youtube_send))
        .route(proto::STREAM_LIST, post(youtube_stream_list))
        .route("/control/youtube/live", post(control_youtube_live))
        .route("/control/youtube/chat", post(control_youtube_chat))
        .route("/control/state", get(control_state))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("port free");
    println!("fake Twitch + Kick + YouTube on :{port}");
    axum::serve(listener, app).await.expect("serve");
}

/// Both platforms' token endpoint: app tokens, codes from `authorize`, refreshes.
async fn token(
    State(fake): State<AppState>,
    uri: axum::http::Uri,
    body: String,
) -> impl IntoResponse {
    let form: HashMap<String, String> = url_form(&body);
    let twitch = uri.path() == "/oauth2/token";
    let mut accounts = fake.accounts.lock().unwrap();
    let (account, scopes) = match form.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let Some((id, login, scopes)) = form
                .get("code")
                .and_then(|code| accounts.codes.remove(code))
            else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "message": "bad code" })),
                );
            };
            (Some((id, login)), scopes)
        }
        Some("refresh_token") => {
            let Some(account) = form
                .get("refresh_token")
                .and_then(|token| accounts.refresh.remove(token))
            else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "message": "bad refresh token" })),
                );
            };
            (Some(account), Vec::new())
        }
        _ => (None, Vec::new()),
    };
    let Some(account) = account else {
        return (
            StatusCode::OK,
            Json(
                json!({ "access_token": fake.id("app_token_"), "expires_in": 3600, "token_type": "bearer" }),
            ),
        );
    };
    let access = fake.id(&format!("user_token_{}_", account.1));
    let refresh = fake.id("refresh_");
    accounts.tokens.insert(access.clone(), account.clone());
    accounts.refresh.insert(refresh.clone(), account);
    let scope = if twitch {
        json!(scopes)
    } else {
        json!(scopes.join(" "))
    };
    (
        StatusCode::OK,
        Json(json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_in": 3600,
            "token_type": "bearer",
            "scope": scope,
        })),
    )
}

fn url_form(body: &str) -> HashMap<String, String> {
    reqwest::Url::parse(&format!("http://x/?{body}"))
        .map(|url| url.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

// --- Twitch ------------------------------------------------------------------

fn conduit_json(id: &str, shards: &[Option<String>]) -> Value {
    json!({ "id": id, "shard_count": shards.len() })
}

async fn conduits(State(fake): State<AppState>) -> Json<Value> {
    let twitch = fake.twitch.lock().unwrap();
    let data: Vec<Value> = twitch
        .conduits
        .iter()
        .map(|(id, shards)| conduit_json(id, shards))
        .collect();
    Json(json!({ "data": data }))
}

async fn create_conduit(State(fake): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let count = body["shard_count"].as_u64().unwrap_or(1) as usize;
    let id = fake.id("conduit_");
    let shards = vec![None; count];
    let data = conduit_json(&id, &shards);
    fake.twitch.lock().unwrap().conduits.insert(id, shards);
    Json(json!({ "data": [data] }))
}

async fn resize_conduit(State(fake): State<AppState>, Json(body): Json<Value>) -> StatusCode {
    let mut twitch = fake.twitch.lock().unwrap();
    let Some(shards) = twitch.conduits.get_mut(body["id"].as_str().unwrap_or("")) else {
        return StatusCode::NOT_FOUND;
    };
    shards.resize(body["shard_count"].as_u64().unwrap_or(1) as usize, None);
    StatusCode::OK
}

#[derive(Deserialize)]
struct IdQuery {
    id: String,
}

async fn delete_conduit(State(fake): State<AppState>, Query(query): Query<IdQuery>) -> StatusCode {
    let mut twitch = fake.twitch.lock().unwrap();
    twitch.conduits.remove(&query.id);
    twitch
        .subscriptions
        .retain(|sub| sub["transport"]["conduit_id"] != query.id.as_str());
    StatusCode::NO_CONTENT
}

async fn assign_shards(State(fake): State<AppState>, Json(body): Json<Value>) -> impl IntoResponse {
    let mut twitch = fake.twitch.lock().unwrap();
    let conduit_id = body["conduit_id"].as_str().unwrap_or("").to_owned();
    let mut errors = Vec::new();
    for shard in body["shards"].as_array().cloned().unwrap_or_default() {
        let index: usize = shard["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .unwrap_or(usize::MAX);
        let session = shard["transport"]["session_id"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let known = twitch.sessions.contains_key(&session);
        match twitch.conduits.get_mut(&conduit_id) {
            Some(shards) if index < shards.len() && known => shards[index] = Some(session),
            _ => errors.push(
                json!({ "id": shard["id"], "message": "invalid shard or session", "code": "400" }),
            ),
        }
    }
    Json(json!({ "data": [], "errors": errors }))
}

async fn twitch_subscribe(
    State(fake): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut twitch = fake.twitch.lock().unwrap();
    let duplicate = twitch
        .subscriptions
        .iter()
        .any(|sub| sub["type"] == body["type"] && sub["condition"] == body["condition"]);
    if duplicate {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "message": "subscription already exists" })),
        );
    }
    let sub = json!({
        "id": fake.id("sub_"),
        "status": "enabled",
        "type": body["type"],
        "version": body["version"],
        "condition": body["condition"],
        "transport": body["transport"],
    });
    twitch.subscriptions.push(sub.clone());
    (StatusCode::ACCEPTED, Json(json!({ "data": [sub] })))
}

async fn twitch_subscriptions(State(fake): State<AppState>) -> Json<Value> {
    let twitch = fake.twitch.lock().unwrap();
    // Like Twitch: subscriptions nobody authorised (raids) cost 1.
    let cost = twitch
        .subscriptions
        .iter()
        .filter(|sub| sub["type"] == "channel.raid")
        .count();
    Json(json!({
        "data": twitch.subscriptions,
        "total": twitch.subscriptions.len(),
        "total_cost": cost,
        "max_total_cost": 10000,
        "pagination": {},
    }))
}

/// A conduit's shards: `enabled` while pointed at a live session.
async fn conduit_shards(
    State(fake): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let twitch = fake.twitch.lock().unwrap();
    let Some(shards) = twitch
        .conduits
        .get(query.get("conduit_id").map_or("", String::as_str))
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "message": "conduit not found" })),
        );
    };
    let data: Vec<Value> = shards
        .iter()
        .enumerate()
        .map(|(id, session)| match session {
            Some(session) => json!({ "id": id.to_string(), "status": "enabled",
                "transport": { "method": "websocket", "session_id": session } }),
            None => json!({ "id": id.to_string(), "status": "websocket_disconnected",
                "transport": { "method": "websocket" } }),
        })
        .collect();
    (
        StatusCode::OK,
        Json(json!({ "data": data, "pagination": {} })),
    )
}

async fn twitch_unsubscribe(
    State(fake): State<AppState>,
    Query(query): Query<IdQuery>,
) -> StatusCode {
    let mut twitch = fake.twitch.lock().unwrap();
    let before = twitch.subscriptions.len();
    twitch
        .subscriptions
        .retain(|sub| sub["id"] != query.id.as_str());
    if twitch.subscriptions.len() < before {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn twitch_send(State(fake): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    println!("twitch chat ← {body}");
    fake.twitch.lock().unwrap().sent.push(body);
    Json(
        json!({ "data": [{ "message_id": fake.id("msg_"), "is_sent": true, "drop_reason": null }] }),
    )
}

async fn eventsub_socket(
    State(fake): State<AppState>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| session(fake, socket))
}

fn envelope(kind: &str, payload: Value) -> String {
    json!({
        "metadata": {
            "message_id": uuid::Uuid::new_v4().to_string(),
            "message_type": kind,
            "message_timestamp": chrono::Utc::now().to_rfc3339(),
        },
        "payload": payload,
    })
    .to_string()
}

async fn session(fake: AppState, mut socket: WebSocket) {
    let session_id = fake.id("session_");
    let (tx, mut rx) = mpsc::unbounded_channel();
    fake.twitch
        .lock()
        .unwrap()
        .sessions
        .insert(session_id.clone(), tx);
    let welcome = envelope(
        "session_welcome",
        json!({ "session": { "id": session_id, "status": "connected", "keepalive_timeout_seconds": 10, "reconnect_url": null } }),
    );
    if socket.send(Message::Text(welcome.into())).await.is_ok() {
        let mut keepalive = tokio::time::interval(Duration::from_secs(5));
        keepalive.tick().await;
        loop {
            tokio::select! {
                _ = keepalive.tick() => {
                    if socket.send(Message::Text(envelope("session_keepalive", json!({})).into())).await.is_err() {
                        break;
                    }
                }
                Some(text) = rx.recv() => {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                incoming = socket.recv() => {
                    if !matches!(incoming, Some(Ok(_))) {
                        break;
                    }
                }
            }
        }
    }
    // Like Twitch: the shards pointing at a closed session are disabled.
    let mut twitch = fake.twitch.lock().unwrap();
    twitch.sessions.remove(&session_id);
    for shards in twitch.conduits.values_mut() {
        for shard in shards.iter_mut() {
            if shard.as_deref() == Some(session_id.as_str()) {
                *shard = None;
            }
        }
    }
}

#[derive(Deserialize)]
struct ChatInput {
    channel: String,
    chatter: String,
    login: String,
    text: String,
    #[serde(default)]
    moderator: bool,
    #[serde(default)]
    vip: bool,
    #[serde(default)]
    subscriber: bool,
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default)]
    bad_signature: bool,
}

async fn control_twitch_chat(
    State(fake): State<AppState>,
    Json(input): Json<ChatInput>,
) -> Json<Value> {
    let badges: Vec<Value> = [
        (input.moderator, "moderator"),
        (input.vip, "vip"),
        (input.subscriber, "subscriber"),
    ]
    .into_iter()
    .filter(|(has, _)| *has)
    .map(|(_, set_id)| json!({ "set_id": set_id, "id": "1", "info": "" }))
    .collect();
    let event = json!({
        "broadcaster_user_id": input.channel,
        "broadcaster_user_login": "channel",
        "broadcaster_user_name": "Channel",
        "chatter_user_id": input.chatter,
        "chatter_user_login": input.login,
        "chatter_user_name": input.login,
        "message_id": uuid::Uuid::new_v4().to_string(),
        "message": { "text": input.text, "fragments": [] },
        "color": "",
        "badges": badges,
        "message_type": "text",
    });
    let delivered = deliver(&fake, &event, |sub| {
        sub["type"] == "channel.chat.message"
            && sub["condition"]["broadcaster_user_id"] == input.channel.as_str()
    });
    Json(json!({ "delivered": delivered }))
}

/// Sends `event` through the conduit (to one enabled shard) for every
/// subscription `wanted` picks, like Twitch does.
fn deliver(fake: &Fake, event: &Value, wanted: impl Fn(&Value) -> bool) -> Vec<Value> {
    let twitch = fake.twitch.lock().unwrap();
    let mut delivered = Vec::new();
    for sub in twitch.subscriptions.iter().filter(|sub| wanted(sub)) {
        let conduit = sub["transport"]["conduit_id"].as_str().unwrap_or("");
        let Some(shards) = twitch.conduits.get(conduit) else {
            continue;
        };
        let enabled: Vec<String> = shards.iter().flatten().cloned().collect();
        if enabled.is_empty() {
            continue;
        }
        // Twitch picks a shard; spread them like it would.
        let session = &enabled[fake.next.fetch_add(1, Ordering::Relaxed) as usize % enabled.len()];
        let message = envelope(
            "notification",
            json!({ "subscription": sub, "event": event }),
        );
        if let Some(tx) = twitch.sessions.get(session)
            && tx.send(message).is_ok()
        {
            delivered.push(json!({ "subscription": sub["id"], "session": session }));
        }
    }
    delivered
}

#[derive(Deserialize)]
struct RaidInput {
    from: String,
    from_name: String,
    to: String,
    to_name: String,
    viewers: u64,
}

async fn control_twitch_raid(
    State(fake): State<AppState>,
    Json(input): Json<RaidInput>,
) -> Json<Value> {
    let event = json!({
        "from_broadcaster_user_id": input.from,
        "from_broadcaster_user_login": input.from_name.to_lowercase(),
        "from_broadcaster_user_name": input.from_name,
        "to_broadcaster_user_id": input.to,
        "to_broadcaster_user_login": input.to_name.to_lowercase(),
        "to_broadcaster_user_name": input.to_name,
        "viewers": input.viewers,
    });
    let delivered = deliver(&fake, &event, |sub| {
        sub["type"] == "channel.raid"
            && (sub["condition"]["to_broadcaster_user_id"] == input.to.as_str()
                || sub["condition"]["from_broadcaster_user_id"] == input.from.as_str())
    });
    Json(json!({ "delivered": delivered }))
}

#[derive(Deserialize)]
struct RevokeInput {
    user_id: String,
}

async fn control_twitch_revoke(
    State(fake): State<AppState>,
    Json(input): Json<RevokeInput>,
) -> Json<Value> {
    let event = json!({ "client_id": "any", "user_id": input.user_id, "user_login": null, "user_name": null });
    let delivered = deliver(&fake, &event, |sub| {
        sub["type"] == "user.authorization.revoke"
    });
    Json(json!({ "delivered": delivered }))
}

// --- Sign-in (both platforms) ------------------------------------------------

/// Accounts the fake knows, and who holds which code and token.
#[derive(Default)]
struct Accounts {
    by_login: HashMap<String, String>,
    /// Who the next authorization signs in as, and the scopes they grant
    /// (`None`: whatever was asked).
    next: Option<(String, String, Option<Vec<String>>)>,
    deny_next: bool,
    codes: HashMap<String, (String, String, Vec<String>)>,
    tokens: HashMap<String, (String, String)>,
    refresh: HashMap<String, (String, String)>,
}

#[derive(Deserialize)]
struct AccountInput {
    id: String,
    login: String,
    grant: Option<Vec<String>>,
}

async fn control_account(
    State(fake): State<AppState>,
    Json(input): Json<AccountInput>,
) -> StatusCode {
    let mut accounts = fake.accounts.lock().unwrap();
    accounts
        .by_login
        .insert(input.login.to_lowercase(), input.id);
    StatusCode::NO_CONTENT
}

async fn control_next(State(fake): State<AppState>, Json(input): Json<AccountInput>) -> StatusCode {
    let mut accounts = fake.accounts.lock().unwrap();
    accounts
        .by_login
        .insert(input.login.to_lowercase(), input.id.clone());
    accounts.next = Some((input.id, input.login, input.grant));
    StatusCode::NO_CONTENT
}

async fn control_deny(State(fake): State<AppState>) -> StatusCode {
    fake.accounts.lock().unwrap().deny_next = true;
    StatusCode::NO_CONTENT
}

async fn authorize(
    State(fake): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let mut accounts = fake.accounts.lock().unwrap();
    let mut back = reqwest::Url::parse(query.get("redirect_uri").map_or("", String::as_str))
        .expect("a redirect_uri");
    back.query_pairs_mut()
        .append_pair("state", query.get("state").map_or("", String::as_str));
    if std::mem::take(&mut accounts.deny_next) {
        back.query_pairs_mut().append_pair("error", "access_denied");
    } else {
        let (id, login, grant) = accounts
            .next
            .clone()
            .unwrap_or_else(|| ("100".into(), "kate".into(), None));
        let asked = query
            .get("scope")
            .map(|scope| scope.split(' ').map(str::to_owned).collect())
            .unwrap_or_default();
        let code = fake.id("code_");
        accounts
            .codes
            .insert(code.clone(), (id, login, grant.unwrap_or(asked)));
        back.query_pairs_mut().append_pair("code", &code);
    }
    axum::response::Redirect::temporary(back.as_str())
}

/// A Twitch user by login, or the one the token belongs to.
async fn helix_users(
    State(fake): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let accounts = fake.accounts.lock().unwrap();
    let user = match query.get("login") {
        Some(login) => accounts
            .by_login
            .get(&login.to_lowercase())
            .map(|id| (id.clone(), login.to_lowercase())),
        None => accounts.tokens.get(&bearer(&headers)).cloned(),
    };
    let data: Vec<Value> = user
        .into_iter()
        .map(|(id, login)| json!({ "id": id, "login": login.to_lowercase(), "display_name": login, "profile_image_url": null }))
        .collect();
    Json(json!({ "data": data }))
}

/// The Kick user the token belongs to.
async fn kick_users(State(fake): State<AppState>, headers: axum::http::HeaderMap) -> Json<Value> {
    let accounts = fake.accounts.lock().unwrap();
    let data: Vec<Value> = accounts
        .tokens
        .get(&bearer(&headers))
        .into_iter()
        .map(|(id, login)| json!({ "user_id": id.parse::<i64>().unwrap_or(0), "name": login, "email": null, "profile_picture": null }))
        .collect();
    Json(json!({ "data": data }))
}

/// A Kick channel by slug.
async fn kick_channels(
    State(fake): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let accounts = fake.accounts.lock().unwrap();
    let slug = match query.get("broadcaster_user_id") {
        Some(id) => accounts
            .by_login
            .iter()
            .find(|(_, known)| *known == id)
            .map(|(login, _)| login.clone())
            .unwrap_or_default(),
        None => query.get("slug").cloned().unwrap_or_default(),
    };
    let data: Vec<Value> = accounts
        .by_login
        .get(&slug)
        .into_iter()
        .map(|id| json!({ "broadcaster_user_id": id.parse::<i64>().unwrap_or(0), "slug": slug }))
        .collect();
    Json(json!({ "data": data }))
}

fn bearer(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .trim_start_matches("Bearer ")
        .to_owned()
}

async fn control_twitch_reconnect(State(fake): State<AppState>) -> Json<Value> {
    let twitch = fake.twitch.lock().unwrap();
    let url = format!("ws://localhost:{}/ws?reconnect=1", fake.port);
    for tx in twitch.sessions.values() {
        let _ = tx.send(envelope(
            "session_reconnect",
            json!({ "session": { "id": "x", "status": "reconnecting", "keepalive_timeout_seconds": null, "reconnect_url": url } }),
        ));
    }
    Json(json!({ "sessions": twitch.sessions.len() }))
}

// --- Kick --------------------------------------------------------------------

/// Kick's site API: the IDs its WebSocket uses.
async fn kick_site_channel(
    State(fake): State<AppState>,
    axum::extract::Path(slug): axum::extract::Path<String>,
) -> impl IntoResponse {
    let accounts = fake.accounts.lock().unwrap();
    let Some(id) = accounts
        .by_login
        .get(&slug)
        .and_then(|id| id.parse::<u64>().ok())
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "message": "Not found" })),
        );
    };
    (
        StatusCode::OK,
        Json(json!({
            "id": id + 1000,
            "user_id": id,
            "slug": slug,
            "chatroom": { "id": id + 2000, "channel_id": id + 1000 },
        })),
    )
}

/// Kick's site WebSocket (Pusher): subscriptions, pings, and raids from
/// `/control/kick/raid`.
async fn kick_pusher_socket(
    State(fake): State<AppState>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| pusher_session(fake, socket))
}

fn pusher_frame(event: &str, data: &str, channel: Option<&str>) -> String {
    let mut frame = json!({ "event": event, "data": data });
    if let Some(channel) = channel {
        frame["channel"] = json!(channel);
    }
    frame.to_string()
}

async fn pusher_session(fake: AppState, mut socket: WebSocket) {
    let id = fake.next.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::unbounded_channel();
    fake.kick
        .lock()
        .unwrap()
        .pusher
        .insert(id, (Vec::new(), tx));
    let established = pusher_frame(
        "pusher:connection_established",
        &json!({ "socket_id": format!("{id}.1"), "activity_timeout": 120 }).to_string(),
        None,
    );
    if socket.send(Message::Text(established.into())).await.is_ok() {
        loop {
            tokio::select! {
                Some(text) = rx.recv() => {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                message = socket.recv() => {
                    let Some(Ok(Message::Text(text))) = message else {
                        if matches!(message, None | Some(Err(_)) | Some(Ok(Message::Close(_)))) {
                            break;
                        }
                        continue;
                    };
                    let Ok(request) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    let topic = request["data"]["channel"].as_str().unwrap_or_default().to_owned();
                    let answer = match request["event"].as_str() {
                        Some("pusher:subscribe") => {
                            if let Some((topics, _)) = fake.kick.lock().unwrap().pusher.get_mut(&id) {
                                topics.push(topic.clone());
                            }
                            Some(pusher_frame("pusher_internal:subscription_succeeded", "{}", Some(&topic)))
                        }
                        Some("pusher:unsubscribe") => {
                            if let Some((topics, _)) = fake.kick.lock().unwrap().pusher.get_mut(&id) {
                                topics.retain(|known| *known != topic);
                            }
                            None
                        }
                        Some("pusher:ping") => Some(pusher_frame("pusher:pong", "{}", None)),
                        _ => None,
                    };
                    if let Some(answer) = answer
                        && socket.send(Message::Text(answer.into())).await.is_err()
                    {
                        break;
                    }
                }
            }
        }
    }
    fake.kick.lock().unwrap().pusher.remove(&id);
}

#[derive(Deserialize)]
struct KickRaidInput {
    from: String,
    to_name: String,
    to_slug: String,
    /// `channel` (default), `chatroom` or `both`: which topic(s) carry it.
    on: Option<String>,
}

async fn control_kick_raid(
    State(fake): State<AppState>,
    Json(input): Json<KickRaidInput>,
) -> impl IntoResponse {
    let Ok(from) = input.from.parse::<u64>() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "message": "from is a user ID" })),
        );
    };
    let from_slug = fake
        .accounts
        .lock()
        .unwrap()
        .by_login
        .iter()
        .find(|(_, id)| **id == input.from)
        .map(|(login, _)| login.clone())
        .unwrap_or_default();
    let channel = format!("channel.{}", from + 1000);
    let chatroom = format!("chatrooms.{}.v2", from + 2000);
    let topics: Vec<&str> = match input.on.as_deref() {
        Some("chatroom") => vec![&chatroom],
        Some("both") => vec![&channel, &chatroom],
        _ => vec![&channel],
    };
    let data = json!({
        "channel": { "id": from + 1000, "user_id": from, "slug": from_slug, "is_banned": false },
        "slug": input.to_slug,
        "hosted": {
            "id": 1,
            "username": input.to_name,
            "slug": input.to_slug,
            "viewers_count": 12,
            "is_live": true,
        },
    })
    .to_string();
    let kick = fake.kick.lock().unwrap();
    let mut delivered = 0;
    for (subscribed, tx) in kick.pusher.values() {
        for topic in &topics {
            if subscribed.iter().any(|known| known == topic) {
                let frame = pusher_frame(
                    r"App\Events\ChatMoveToSupportedChannelEvent",
                    &data,
                    Some(topic),
                );
                if tx.send(frame).is_ok() {
                    delivered += 1;
                }
            }
        }
    }
    (StatusCode::OK, Json(json!({ "delivered": delivered })))
}

async fn kick_key(State(fake): State<AppState>) -> Json<Value> {
    Json(json!({ "data": { "public_key": fake.public_key_pem() }, "message": "OK" }))
}

async fn kick_subscribe(State(fake): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let mut kick = fake.kick.lock().unwrap();
    let mut data = Vec::new();
    for event in body["events"].as_array().cloned().unwrap_or_default() {
        let id = fake.id("kicksub_");
        kick.subscriptions.push(json!({
            "id": id,
            "event": event["name"],
            "version": event["version"],
            "broadcaster_user_id": body["broadcaster_user_id"],
            "method": "webhook",
        }));
        data.push(json!({ "name": event["name"], "version": event["version"], "subscription_id": id, "error": null }));
    }
    Json(json!({ "data": data, "message": "OK" }))
}

async fn kick_subscriptions(State(fake): State<AppState>) -> Json<Value> {
    Json(json!({ "data": fake.kick.lock().unwrap().subscriptions, "message": "OK" }))
}

async fn kick_unsubscribe(
    State(fake): State<AppState>,
    Query(query): Query<IdQuery>,
) -> StatusCode {
    let mut kick = fake.kick.lock().unwrap();
    kick.subscriptions
        .retain(|sub| sub["id"] != query.id.as_str());
    StatusCode::NO_CONTENT
}

async fn kick_send(
    State(fake): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if auth.contains("app_token_") {
        // Posting as a user needs that user's token, not the app's.
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "message": "user token required" })),
        );
    }
    println!("kick chat ← {body}");
    fake.kick
        .lock()
        .unwrap()
        .sent
        .push(json!({ "body": body, "auth": auth }));
    (
        StatusCode::OK,
        Json(
            json!({ "data": { "is_sent": true, "message_id": fake.id("kickmsg_") }, "message": "OK" }),
        ),
    )
}

async fn control_kick_chat(
    State(fake): State<AppState>,
    Json(input): Json<ChatInput>,
) -> Json<Value> {
    let channel: i64 = input.channel.parse().unwrap_or(0);
    let chatter: i64 = input.chatter.parse().unwrap_or(0);
    let badges: Vec<Value> = [
        (input.moderator, "Moderator", "moderator"),
        (input.vip, "VIP", "vip"),
        (input.subscriber, "Subscriber", "subscriber"),
    ]
    .into_iter()
    .filter(|(has, _, _)| *has)
    .map(|(_, text, kind)| json!({ "text": text, "type": kind }))
    .collect();
    let body = json!({
        "message_id": uuid::Uuid::new_v4().to_string(),
        "replies_to": null,
        "broadcaster": { "is_anonymous": false, "user_id": channel, "username": "channel", "is_verified": false, "profile_picture": "", "channel_slug": "channel", "identity": null },
        "sender": { "is_anonymous": false, "user_id": chatter, "username": input.login, "is_verified": false, "profile_picture": "", "channel_slug": input.login.to_lowercase(),
            "identity": { "username_color": "#FF5733", "badges": badges } },
        "content": input.text,
        "emotes": [],
        "created_at": chrono::Utc::now().to_rfc3339(),
    })
    .to_string();
    let message_id = uuid::Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().to_rfc3339();
    let mut signature = vec![0; fake.key.public_modulus_len()];
    fake.key
        .sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            format!("{message_id}.{timestamp}.{body}").as_bytes(),
            &mut signature,
        )
        .expect("signing");
    let mut signature = STANDARD.encode(signature);
    if input.bad_signature {
        signature = STANDARD.encode(b"not a signature");
    }
    let mut statuses = Vec::new();
    for target in &input.targets {
        let status = fake
            .http
            .post(target)
            .header("content-type", "application/json")
            .header("Kick-Event-Message-Id", &message_id)
            .header("Kick-Event-Subscription-Id", "kicksub")
            .header("Kick-Event-Signature", &signature)
            .header("Kick-Event-Message-Timestamp", &timestamp)
            .header("Kick-Event-Type", "chat.message.sent")
            .header("Kick-Event-Version", "1")
            .body(body.clone())
            .send()
            .await
            .map(|response| response.status().as_u16())
            .unwrap_or(0);
        statuses.push(json!({ "target": target, "status": status }));
    }
    Json(json!({ "statuses": statuses }))
}

async fn control_state(State(fake): State<AppState>) -> Json<Value> {
    let twitch = fake.twitch.lock().unwrap();
    let kick = fake.kick.lock().unwrap();
    let youtube = fake.youtube.lock().unwrap();
    let conduits: HashMap<&String, &Vec<Option<String>>> = twitch.conduits.iter().collect();
    Json(json!({
        "twitch": {
            "conduits": conduits,
            "sessions": twitch.sessions.keys().collect::<Vec<_>>(),
            "subscriptions": twitch.subscriptions,
            "sent": twitch.sent,
        },
        "kick": {
            "subscriptions": kick.subscriptions,
            "sent": kick.sent,
            "pusher": kick.pusher.values().map(|(topics, _)| topics).collect::<Vec<_>>(),
        },
        "youtube": {
            "live": youtube.live,
            "lookups": youtube.lookups,
            "sent": youtube.sent,
        },
    }))
}

// --- YouTube -------------------------------------------------------------------

/// The chat service's own `StreamList` messages, so both sides agree.
#[path = "../src/youtube/proto.rs"]
#[allow(dead_code)]
mod proto;

#[derive(Default)]
struct YouTube {
    /// The live chat of each channel that is live.
    live: HashMap<String, String>,
    chats: HashMap<String, LiveChat>,
    /// `liveBroadcasts.list` calls (1 quota unit each).
    lookups: u64,
    sent: Vec<Value>,
}

struct LiveChat {
    channel: String,
    messages: Vec<proto::LiveChatMessage>,
    ended: bool,
    /// Wakes the streams when a message comes or the chat ends.
    updates: tokio::sync::watch::Sender<usize>,
}

/// The channel the token belongs to, or a handle's channel (with an API key).
async fn youtube_channels(
    State(fake): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let accounts = fake.accounts.lock().unwrap();
    let channel = match query.get("forHandle") {
        Some(handle) => {
            let login = handle.trim_start_matches('@').to_lowercase();
            accounts.by_login.get(&login).map(|id| (id.clone(), login))
        }
        None => accounts.tokens.get(&bearer(&headers)).cloned(),
    };
    let items: Vec<Value> = channel
        .into_iter()
        .map(|(id, login)| {
            json!({ "id": id, "snippet": { "title": login, "customUrl": format!("@{login}"), "thumbnails": {} } })
        })
        .collect();
    Json(json!({ "items": items }))
}

/// The token's channel's broadcast on air, if it is live.
async fn youtube_broadcasts(
    State(fake): State<AppState>,
    headers: axum::http::HeaderMap,
) -> (StatusCode, Json<Value>) {
    let Some((channel, _)) = fake
        .accounts
        .lock()
        .unwrap()
        .tokens
        .get(&bearer(&headers))
        .cloned()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": { "code": 401 } })),
        );
    };
    let mut youtube = fake.youtube.lock().unwrap();
    youtube.lookups += 1;
    let items: Vec<Value> = youtube
        .live
        .get(&channel)
        .map(|chat| json!({ "id": format!("broadcast_{chat}"), "snippet": { "liveChatId": chat } }))
        .into_iter()
        .collect();
    (StatusCode::OK, Json(json!({ "items": items })))
}

/// `liveChatMessages.insert`: recorded, and shown in the chat like YouTube does.
async fn youtube_send(
    State(fake): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let Some((bot, login)) = fake
        .accounts
        .lock()
        .unwrap()
        .tokens
        .get(&bearer(&headers))
        .cloned()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": { "code": 401 } })),
        );
    };
    let chat_id = body["snippet"]["liveChatId"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let text = body["snippet"]["textMessageDetails"]["messageText"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let mut youtube = fake.youtube.lock().unwrap();
    match youtube.chats.get(&chat_id) {
        Some(chat) if !chat.ended => {}
        _ => {
            return (
                StatusCode::FORBIDDEN,
                Json(
                    json!({ "error": { "code": 403, "errors": [{ "reason": "liveChatEnded" }] } }),
                ),
            );
        }
    }
    println!("YouTube {bot} in {chat_id}: {text}");
    youtube
        .sent
        .push(json!({ "bot": bot, "live_chat_id": chat_id, "text": text }));
    let id = fake.id("yt_msg_");
    if let Some(chat) = youtube.chats.get_mut(&chat_id) {
        chat.messages
            .push(chat_message(&id, &bot, &login, &text, false, false, false));
        chat.updates.send_replace(chat.messages.len());
    }
    (StatusCode::OK, Json(json!({ "id": id })))
}

fn chat_message(
    id: &str,
    channel: &str,
    name: &str,
    text: &str,
    owner: bool,
    moderator: bool,
    member: bool,
) -> proto::LiveChatMessage {
    proto::LiveChatMessage {
        id: Some(id.to_owned()),
        snippet: Some(proto::LiveChatMessageSnippet {
            r#type: Some(proto::TEXT_MESSAGE_EVENT),
            published_at: Some(chrono::Utc::now().to_rfc3339()),
            text_message_details: Some(proto::LiveChatTextMessageDetails {
                message_text: Some(text.to_owned()),
            }),
        }),
        author_details: Some(proto::LiveChatMessageAuthorDetails {
            channel_id: Some(channel.to_owned()),
            display_name: Some(format!("@{name}")),
            is_chat_owner: Some(owner),
            is_chat_sponsor: Some(member),
            is_chat_moderator: Some(moderator),
        }),
    }
}

/// The `StreamList` RPC: the chat's messages from `page_token` (the index of
/// the next one; all of them without), then new ones as they come.
struct StreamList(AppState);

type ListStream = std::pin::Pin<
    Box<
        dyn futures_util::Stream<Item = Result<proto::LiveChatMessageListResponse, tonic::Status>>
            + Send,
    >,
>;

impl tonic::server::ServerStreamingService<proto::LiveChatMessageListRequest> for StreamList {
    type Response = proto::LiveChatMessageListResponse;
    type ResponseStream = ListStream;
    type Future = std::future::Ready<Result<tonic::Response<ListStream>, tonic::Status>>;

    fn call(&mut self, request: tonic::Request<proto::LiveChatMessageListRequest>) -> Self::Future {
        std::future::ready(self.start(request))
    }
}

impl StreamList {
    fn start(
        &self,
        request: tonic::Request<proto::LiveChatMessageListRequest>,
    ) -> Result<tonic::Response<ListStream>, tonic::Status> {
        let token = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .trim_start_matches("Bearer ")
            .to_owned();
        let Some((caller, _)) = self.0.accounts.lock().unwrap().tokens.get(&token).cloned() else {
            return Err(tonic::Status::unauthenticated("unknown token"));
        };
        let request = request.into_inner();
        let chat_id = request.live_chat_id.unwrap_or_default();
        let youtube = self.0.youtube.lock().unwrap();
        let Some(chat) = youtube.chats.get(&chat_id) else {
            return Err(tonic::Status::not_found("liveChatNotFound"));
        };
        if chat.channel != caller {
            return Err(tonic::Status::permission_denied("not your chat"));
        }
        if chat.ended {
            return Err(tonic::Status::failed_precondition("LIVE_CHAT_ENDED"));
        }
        let from: usize = request
            .page_token
            .and_then(|token| token.parse().ok())
            .unwrap_or(0);
        let updates = chat.updates.subscribe();
        drop(youtube);
        println!("YouTube chat {chat_id} streamed from {from}");
        let fake = self.0.clone();
        let stream = futures_util::stream::unfold(
            (fake, chat_id, from, updates, false),
            |(fake, chat_id, from, mut updates, done)| async move {
                if done {
                    return None;
                }
                loop {
                    let (items, ended) = {
                        let youtube = fake.youtube.lock().unwrap();
                        let chat = youtube.chats.get(&chat_id)?;
                        (
                            chat.messages[from.min(chat.messages.len())..].to_vec(),
                            chat.ended,
                        )
                    };
                    if !items.is_empty() || ended {
                        let next = from + items.len();
                        let response = proto::LiveChatMessageListResponse {
                            offline_at: ended.then(|| chrono::Utc::now().to_rfc3339()),
                            next_page_token: Some(next.to_string()),
                            items,
                        };
                        return Some((Ok(response), (fake, chat_id, next, updates, ended)));
                    }
                    if updates.changed().await.is_err() {
                        return None;
                    }
                }
            },
        );
        Ok(tonic::Response::new(Box::pin(stream)))
    }
}

async fn youtube_stream_list(
    State(fake): State<AppState>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let mut grpc = tonic::server::Grpc::new(tonic_prost::ProstCodec::<
        proto::LiveChatMessageListResponse,
        proto::LiveChatMessageListRequest,
    >::default());
    grpc.server_streaming(StreamList(fake), request)
        .await
        .map(axum::body::Body::new)
}

#[derive(Deserialize)]
struct YouTubeLive {
    channel: String,
    live: bool,
}

/// Starts (a new broadcast, with a new chat) or ends a channel's stream.
async fn control_youtube_live(
    State(fake): State<AppState>,
    Json(input): Json<YouTubeLive>,
) -> Json<Value> {
    let mut youtube = fake.youtube.lock().unwrap();
    if let Some(chat_id) = youtube.live.remove(&input.channel)
        && let Some(chat) = youtube.chats.get_mut(&chat_id)
    {
        chat.ended = true;
        chat.updates.send_modify(|_| {});
    }
    if !input.live {
        return Json(json!({ "live": null }));
    }
    let chat_id = fake.id("chat_");
    youtube.live.insert(input.channel.clone(), chat_id.clone());
    youtube.chats.insert(
        chat_id.clone(),
        LiveChat {
            channel: input.channel,
            messages: Vec::new(),
            ended: false,
            updates: tokio::sync::watch::channel(0).0,
        },
    );
    Json(json!({ "live": chat_id }))
}

#[derive(Deserialize)]
struct YouTubeChat {
    channel: String,
    chatter: String,
    name: String,
    text: String,
    #[serde(default)]
    moderator: bool,
    #[serde(default)]
    member: bool,
}

/// A message in the channel's live chat (the owner when `chatter` is the channel).
async fn control_youtube_chat(
    State(fake): State<AppState>,
    Json(input): Json<YouTubeChat>,
) -> (StatusCode, Json<Value>) {
    let id = fake.id("yt_msg_");
    let mut youtube = fake.youtube.lock().unwrap();
    let Some(chat_id) = youtube.live.get(&input.channel).cloned() else {
        return (StatusCode::CONFLICT, Json(json!({ "error": "not live" })));
    };
    let chat = youtube.chats.get_mut(&chat_id).expect("a live chat");
    chat.messages.push(chat_message(
        &id,
        &input.chatter,
        &input.name,
        &input.text,
        input.chatter == input.channel,
        input.moderator,
        input.member,
    ));
    chat.updates.send_replace(chat.messages.len());
    (
        StatusCode::OK,
        Json(json!({ "id": id, "live_chat_id": chat_id })),
    )
}
