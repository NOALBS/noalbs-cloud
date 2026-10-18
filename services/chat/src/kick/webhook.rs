//! `POST /chat/webhooks/kick`, served by every instance (the load balancer
//! picks one). A delivery is checked, recorded as seen (Kick retries, possibly
//! to another instance) and published for handling; Kick gets its 200 at once.
//! <https://docs.kick.com/events/webhook-security>

use axum::{
    body::Bytes,
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::{
    app::State,
    inbound::{self, Chatter, InboundMessage},
    kv,
};

/// Deliveries older (or newer) than this are refused, so a captured one can't be replayed later.
const MAX_AGE: chrono::Duration = chrono::Duration::minutes(5);

pub async fn handle(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let Some(kick) = state.kick.clone() else {
        return StatusCode::NOT_FOUND;
    };
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    let (Some(message_id), Some(timestamp), Some(signature)) = (
        header("Kick-Event-Message-Id"),
        header("Kick-Event-Message-Timestamp"),
        header("Kick-Event-Signature"),
    ) else {
        return StatusCode::BAD_REQUEST;
    };
    if !is_fresh(timestamp, Utc::now()) {
        return StatusCode::BAD_REQUEST;
    }
    let key = match kick.public_key().await {
        Ok(key) => key,
        Err(error) => {
            tracing::warn!(%error, "Couldn't get Kick's webhook key");
            // Kick retries; maybe the key is reachable by then.
            return StatusCode::SERVICE_UNAVAILABLE;
        }
    };
    if !verify(&key, message_id, timestamp, &body, signature) {
        tracing::warn!("Refused a Kick webhook with a bad signature");
        return StatusCode::UNAUTHORIZED;
    }
    match kv::first_time(&state.buckets.seen, &kv::key(&["kick", message_id])).await {
        Ok(true) => {}
        Ok(false) => return StatusCode::OK,
        Err(error) => tracing::warn!(%error, "Couldn't check for a repeated Kick delivery"),
    }

    if header("Kick-Event-Type") == Some(super::CHAT_MESSAGE) {
        match parse_chat(&body) {
            Ok(message) => inbound::publish(&state, &message).await,
            Err(error) => tracing::warn!(%error, "Couldn't read a Kick chat message"),
        }
    }
    StatusCode::OK
}

/// Whether the delivery's timestamp (RFC 3339, or Unix seconds) is recent.
fn is_fresh(timestamp: &str, now: DateTime<Utc>) -> bool {
    let at = DateTime::parse_from_rfc3339(timestamp)
        .map(|at| at.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            timestamp
                .parse::<i64>()
                .ok()
                .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        });
    at.is_some_and(|at| (now - at).abs() <= MAX_AGE)
}

/// Checks Kick's RSA-SHA256 signature over `message_id.timestamp.body`.
pub fn verify(
    public_key_der: &[u8],
    message_id: &str,
    timestamp: &str,
    body: &[u8],
    signature_b64: &str,
) -> bool {
    let Ok(signature) = STANDARD.decode(signature_b64.trim()) else {
        return false;
    };
    let mut signed = Vec::with_capacity(message_id.len() + timestamp.len() + body.len() + 2);
    signed.extend_from_slice(message_id.as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(timestamp.as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(body);
    aws_lc_rs::signature::UnparsedPublicKey::new(
        &aws_lc_rs::signature::RSA_PKCS1_2048_8192_SHA256,
        public_key_der,
    )
    .verify(&signed, &signature)
    .is_ok()
}

/// The DER inside a PEM block (`-----BEGIN PUBLIC KEY-----` …).
pub fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("-----"))
        .collect();
    STANDARD.decode(body).ok()
}

#[derive(Deserialize)]
struct ChatEvent {
    message_id: String,
    broadcaster: User,
    sender: User,
    content: String,
}

#[derive(Deserialize)]
struct User {
    user_id: i64,
    username: String,
    #[serde(default)]
    channel_slug: Option<String>,
    #[serde(default)]
    identity: Option<Identity>,
}

#[derive(Deserialize)]
struct Identity {
    #[serde(default)]
    badges: Vec<Badge>,
}

#[derive(Deserialize)]
struct Badge {
    #[serde(rename = "type")]
    kind: String,
}

pub fn parse_chat(body: &[u8]) -> serde_json::Result<InboundMessage> {
    let event: ChatEvent = serde_json::from_slice(body)?;
    let has = |kinds: &[&str]| {
        event.sender.identity.as_ref().is_some_and(|identity| {
            identity
                .badges
                .iter()
                .any(|badge| kinds.contains(&badge.kind.as_str()))
        })
    };
    let moderator =
        event.sender.user_id == event.broadcaster.user_id || has(&["moderator", "broadcaster"]);
    let vip = has(&["vip"]);
    // Founders (the first subscribers) show that badge instead.
    let subscriber = has(&["subscriber", "founder"]);
    Ok(InboundMessage {
        platform: db::chat::ChatPlatform::Kick,
        channel_id: event.broadcaster.user_id.to_string(),
        bot_id: None,
        message_id: event.message_id,
        chatter: Chatter {
            id: event.sender.user_id.to_string(),
            login: event
                .sender
                .channel_slug
                .unwrap_or_else(|| event.sender.username.to_lowercase()),
            display_name: event.sender.username,
            moderator,
            vip,
            subscriber,
        },
        text: event.content,
    })
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::{
        encoding::AsDer,
        rand::SystemRandom,
        rsa::{KeyPair, KeySize},
        signature::{KeyPair as _, RSA_PKCS1_SHA256},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use chrono::{TimeZone, Utc};

    use super::{is_fresh, parse_chat, pem_to_der, verify};

    const BODY: &str = r##"{"message_id":"01JH5","replies_to":null,
        "broadcaster":{"is_anonymous":false,"user_id":200,"username":"Kate_Kick","is_verified":false,"profile_picture":"","channel_slug":"kate-kick","identity":null},
        "sender":{"is_anonymous":false,"user_id":77,"username":"ModBob","is_verified":false,"profile_picture":"","channel_slug":"modbob",
            "identity":{"username_color":"#FF5733","badges":[{"text":"Moderator","type":"moderator"}]}},
        "content":"!bitrate","emotes":[],"created_at":"2026-09-25T10:00:00Z"}"##;

    fn signed(key: &KeyPair, id: &str, timestamp: &str, body: &str) -> String {
        let mut signature = vec![0; key.public_modulus_len()];
        key.sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            format!("{id}.{timestamp}.{body}").as_bytes(),
            &mut signature,
        )
        .unwrap();
        STANDARD.encode(signature)
    }

    #[test]
    fn signatures_are_checked_with_a_pem_key() {
        let key = KeyPair::generate(KeySize::Rsa2048).unwrap();
        let spki = key.public_key().as_der().unwrap();
        let pem = format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(spki.as_ref())
        );
        let der = pem_to_der(&pem).unwrap();
        let signature = signed(&key, "01JH5", "2026-09-25T10:00:00Z", BODY);

        assert!(verify(
            &der,
            "01JH5",
            "2026-09-25T10:00:00Z",
            BODY.as_bytes(),
            &signature
        ));
        // Anything changed breaks it.
        assert!(!verify(
            &der,
            "01JH6",
            "2026-09-25T10:00:00Z",
            BODY.as_bytes(),
            &signature
        ));
        assert!(!verify(
            &der,
            "01JH5",
            "2026-09-25T10:00:01Z",
            BODY.as_bytes(),
            &signature
        ));
        assert!(!verify(
            &der,
            "01JH5",
            "2026-09-25T10:00:00Z",
            b"{}",
            &signature
        ));
        assert!(!verify(
            &der,
            "01JH5",
            "2026-09-25T10:00:00Z",
            BODY.as_bytes(),
            "bm9wZQ=="
        ));
    }

    #[test]
    fn stale_deliveries_are_refused() {
        let now = Utc.with_ymd_and_hms(2026, 9, 25, 10, 0, 0).unwrap();
        assert!(is_fresh("2026-09-25T09:57:00Z", now));
        assert!(is_fresh(&now.timestamp().to_string(), now));
        assert!(!is_fresh("2026-09-25T09:54:00Z", now));
        assert!(!is_fresh("yesterday", now));
    }

    #[test]
    fn chat_messages_become_inbound_messages() {
        let message = parse_chat(BODY.as_bytes()).unwrap();
        assert_eq!(message.channel_id, "200");
        assert_eq!(message.message_id, "01JH5");
        assert_eq!(message.chatter.id, "77");
        assert_eq!(message.chatter.login, "modbob");
        assert_eq!(message.chatter.display_name, "ModBob");
        assert!(message.chatter.moderator);
        assert!(!message.chatter.vip);
        assert!(!message.chatter.subscriber);
        assert_eq!(message.text, "!bitrate");
    }

    #[test]
    fn vip_subscriber_and_founder_badges() {
        let chatter = |badges: &str| {
            let body = BODY.replace(r#"[{"text":"Moderator","type":"moderator"}]"#, badges);
            parse_chat(body.as_bytes()).unwrap().chatter
        };
        let vip = chatter(r#"[{"text":"VIP","type":"vip"}]"#);
        assert!(vip.vip && !vip.subscriber && !vip.moderator);
        let sub = chatter(r#"[{"text":"Subscriber","type":"subscriber","count":3}]"#);
        assert!(sub.subscriber && !sub.vip);
        assert!(chatter(r#"[{"text":"Founder","type":"founder"}]"#).subscriber);
        let other = chatter(r#"[{"text":"OG","type":"og"}]"#);
        assert!(!other.vip && !other.subscriber && !other.moderator);
    }
}
