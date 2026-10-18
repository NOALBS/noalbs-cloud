//! EventSub WebSocket messages (the parts we use), decoded without side effects.
//! <https://dev.twitch.tv/docs/eventsub/handling-websocket-events/>

use serde::Deserialize;

use crate::inbound::{Chatter, InboundMessage};

#[derive(Debug, Deserialize)]
struct Envelope {
    metadata: Metadata,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    message_id: String,
    message_type: String,
}

#[derive(Debug, Deserialize)]
struct SessionPayload {
    session: Session,
}

#[derive(Debug, Deserialize)]
struct Session {
    id: String,
    keepalive_timeout_seconds: Option<u64>,
    reconnect_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NotificationPayload {
    subscription: Subscription,
    event: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Subscription {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub condition: serde_json::Map<String, serde_json::Value>,
}

impl Subscription {
    pub fn condition(&self, key: &str) -> Option<&str> {
        self.condition.get(key).and_then(|value| value.as_str())
    }
}

#[derive(Debug, Deserialize)]
struct ChatMessageEvent {
    broadcaster_user_id: String,
    chatter_user_id: String,
    chatter_user_login: String,
    chatter_user_name: String,
    message_id: String,
    message: ChatText,
    #[serde(default)]
    badges: Vec<Badge>,
}

#[derive(Debug, Deserialize)]
struct RaidEvent {
    from_broadcaster_user_id: String,
    to_broadcaster_user_login: String,
    to_broadcaster_user_name: String,
}

#[derive(Debug, Deserialize)]
struct ChatText {
    text: String,
}

#[derive(Debug, Deserialize)]
struct Badge {
    set_id: String,
}

/// A WebSocket message from Twitch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// The session is up: assign `session_id` to our shard within 10 s.
    Welcome {
        session_id: String,
        keepalive: Option<u64>,
    },
    Keepalive,
    /// Twitch is moving us: connect to `url`; the old socket closes after its welcome.
    Reconnect {
        url: String,
    },
    /// A chat message (already in our shape).
    Chat {
        message_id: String,
        message: InboundMessage,
    },
    /// A channel we follow raided another (outgoing raids only).
    Raid {
        message_id: String,
        from_id: String,
        /// The raided channel's display name and login (for its URL).
        to_name: String,
        to_login: String,
    },
    /// A user took our app's access away.
    AuthRevoked {
        message_id: String,
        user_id: String,
    },
    /// Another subscribed event we don't handle.
    Event {
        message_id: String,
        subscription: Subscription,
        event: serde_json::Value,
    },
    /// Twitch dropped a subscription (e.g. `authorization_revoked`).
    Revocation {
        subscription: Subscription,
    },
    Other(String),
}

pub fn decode(text: &str) -> serde_json::Result<Message> {
    let envelope: Envelope = serde_json::from_str(text)?;
    let metadata = envelope.metadata;
    Ok(match metadata.message_type.as_str() {
        "session_welcome" => {
            let payload: SessionPayload = serde_json::from_value(envelope.payload)?;
            Message::Welcome {
                session_id: payload.session.id,
                keepalive: payload.session.keepalive_timeout_seconds,
            }
        }
        "session_keepalive" => Message::Keepalive,
        "session_reconnect" => {
            let payload: SessionPayload = serde_json::from_value(envelope.payload)?;
            match payload.session.reconnect_url {
                Some(url) => Message::Reconnect { url },
                None => Message::Other("session_reconnect without a URL".into()),
            }
        }
        "notification" => {
            let payload: NotificationPayload = serde_json::from_value(envelope.payload)?;
            if payload.subscription.kind == "channel.chat.message" {
                let bot_id = payload.subscription.condition("user_id").map(str::to_owned);
                let event: ChatMessageEvent = serde_json::from_value(payload.event)?;
                let has = |sets: &[&str]| {
                    event
                        .badges
                        .iter()
                        .any(|badge| sets.contains(&badge.set_id.as_str()))
                };
                let moderator = event.chatter_user_id == event.broadcaster_user_id
                    || has(&["moderator", "lead_moderator", "broadcaster"]);
                let vip = has(&["vip"]);
                // Founders (the first subscribers) show that badge instead.
                let subscriber = has(&["subscriber", "founder"]);
                Message::Chat {
                    message_id: metadata.message_id,
                    message: InboundMessage {
                        platform: db::chat::ChatPlatform::Twitch,
                        channel_id: event.broadcaster_user_id,
                        bot_id,
                        message_id: event.message_id,
                        chatter: Chatter {
                            id: event.chatter_user_id,
                            login: event.chatter_user_login,
                            display_name: event.chatter_user_name,
                            moderator,
                            vip,
                            subscriber,
                        },
                        text: event.message.text,
                    },
                }
            } else if payload.subscription.kind == "channel.raid" {
                let event: RaidEvent = serde_json::from_value(payload.event)?;
                Message::Raid {
                    message_id: metadata.message_id,
                    from_id: event.from_broadcaster_user_id,
                    to_name: event.to_broadcaster_user_name,
                    to_login: event.to_broadcaster_user_login,
                }
            } else if payload.subscription.kind == "user.authorization.revoke" {
                #[derive(Deserialize)]
                struct Revoked {
                    user_id: String,
                }
                let event: Revoked = serde_json::from_value(payload.event)?;
                Message::AuthRevoked {
                    message_id: metadata.message_id,
                    user_id: event.user_id,
                }
            } else {
                Message::Event {
                    message_id: metadata.message_id,
                    subscription: payload.subscription,
                    event: payload.event,
                }
            }
        }
        "revocation" => {
            #[derive(Deserialize)]
            struct RevocationPayload {
                subscription: Subscription,
            }
            let payload: RevocationPayload = serde_json::from_value(envelope.payload)?;
            Message::Revocation {
                subscription: payload.subscription,
            }
        }
        other => Message::Other(other.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::{Message, decode};

    #[test]
    fn welcome_keepalive_and_reconnect() {
        let welcome = r#"{"metadata":{"message_id":"a","message_type":"session_welcome","message_timestamp":"2026-09-25T10:00:00Z"},
            "payload":{"session":{"id":"AQoQexAWVYKSTIu4ec_2VAxyuhAB","status":"connected","connected_at":"2026-09-25T10:00:00Z","keepalive_timeout_seconds":10,"reconnect_url":null}}}"#;
        assert_eq!(
            decode(welcome).unwrap(),
            Message::Welcome {
                session_id: "AQoQexAWVYKSTIu4ec_2VAxyuhAB".into(),
                keepalive: Some(10)
            }
        );
        let keepalive =
            r#"{"metadata":{"message_id":"b","message_type":"session_keepalive"},"payload":{}}"#;
        assert_eq!(decode(keepalive).unwrap(), Message::Keepalive);
        let reconnect = r#"{"metadata":{"message_id":"c","message_type":"session_reconnect"},
            "payload":{"session":{"id":"x","status":"reconnecting","keepalive_timeout_seconds":null,"reconnect_url":"wss://eventsub.wss.twitch.tv?reconnect=1"}}}"#;
        assert_eq!(
            decode(reconnect).unwrap(),
            Message::Reconnect {
                url: "wss://eventsub.wss.twitch.tv?reconnect=1".into()
            }
        );
    }

    #[test]
    fn chat_messages_carry_the_bot_and_moderator_badge() {
        let text = r#"{"metadata":{"message_id":"m1","message_type":"notification","subscription_type":"channel.chat.message"},
            "payload":{"subscription":{"id":"sub1","status":"enabled","type":"channel.chat.message","version":"1",
                "condition":{"broadcaster_user_id":"100","user_id":"1"},"transport":{"method":"conduit","conduit_id":"c1"}},
              "event":{"broadcaster_user_id":"100","broadcaster_user_login":"kate","broadcaster_user_name":"Kate",
                "chatter_user_id":"55","chatter_user_login":"mod_bob","chatter_user_name":"Mod_Bob",
                "message_id":"cc106a89","message":{"text":"!switch live","fragments":[]},
                "color":"","badges":[{"set_id":"moderator","id":"1","info":""}],"message_type":"text"}}}"#;
        let Message::Chat {
            message_id,
            message,
        } = decode(text).unwrap()
        else {
            panic!("a chat message");
        };
        assert_eq!(message_id, "m1");
        assert_eq!(message.channel_id, "100");
        assert_eq!(message.bot_id.as_deref(), Some("1"));
        assert_eq!(message.message_id, "cc106a89");
        assert_eq!(message.chatter.login, "mod_bob");
        assert!(message.chatter.moderator);
        assert!(!message.chatter.vip);
        assert!(!message.chatter.subscriber);
        assert_eq!(message.text, "!switch live");
    }

    #[test]
    fn vip_subscriber_and_founder_badges() {
        let chatter = |badges: &str| {
            let text = format!(
                r#"{{"metadata":{{"message_id":"m1","message_type":"notification"}},
                "payload":{{"subscription":{{"id":"sub1","type":"channel.chat.message",
                    "condition":{{"broadcaster_user_id":"100","user_id":"1"}}}},
                  "event":{{"broadcaster_user_id":"100","chatter_user_id":"55","chatter_user_login":"bob",
                    "chatter_user_name":"Bob","message_id":"x","message":{{"text":"!bitrate"}},
                    "badges":{badges}}}}}}}"#
            );
            let Message::Chat { message, .. } = decode(&text).unwrap() else {
                panic!("a chat message");
            };
            message.chatter
        };
        let vip = chatter(r#"[{"set_id":"vip","id":"1","info":""}]"#);
        assert!(vip.vip && !vip.subscriber && !vip.moderator);
        let sub = chatter(r#"[{"set_id":"subscriber","id":"3012","info":"14"}]"#);
        assert!(sub.subscriber && !sub.vip);
        assert!(chatter(r#"[{"set_id":"founder","id":"0","info":"3"}]"#).subscriber);
        assert!(chatter(r#"[{"set_id":"lead_moderator","id":"1","info":""}]"#).moderator);
        let other = chatter(r#"[{"set_id":"premium","id":"1","info":""}]"#);
        assert!(!other.vip && !other.subscriber && !other.moderator);
    }

    #[test]
    fn revocations_and_other_events() {
        let revoked = r#"{"metadata":{"message_id":"r","message_type":"revocation"},
            "payload":{"subscription":{"id":"sub1","status":"authorization_revoked","type":"channel.chat.message",
                "condition":{"broadcaster_user_id":"100","user_id":"1"}}}}"#;
        let Message::Revocation { subscription } = decode(revoked).unwrap() else {
            panic!("a revocation");
        };
        assert_eq!(subscription.status, "authorization_revoked");
        assert_eq!(subscription.condition("broadcaster_user_id"), Some("100"));

        let raid = r#"{"metadata":{"message_id":"e","message_type":"notification"},
            "payload":{"subscription":{"id":"sub2","type":"channel.raid","condition":{"from_broadcaster_user_id":"100","to_broadcaster_user_id":""}},
              "event":{"from_broadcaster_user_id":"100","from_broadcaster_user_login":"kate","from_broadcaster_user_name":"Kate",
                "to_broadcaster_user_id":"300","to_broadcaster_user_login":"bob","to_broadcaster_user_name":"Bob","viewers":12}}}"#;
        assert_eq!(
            decode(raid).unwrap(),
            Message::Raid {
                message_id: "e".into(),
                from_id: "100".into(),
                to_name: "Bob".into(),
                to_login: "bob".into()
            }
        );
        let revoke = r#"{"metadata":{"message_id":"v","message_type":"notification"},
            "payload":{"subscription":{"id":"sub3","type":"user.authorization.revoke","condition":{"client_id":"abc"}},
              "event":{"client_id":"abc","user_id":"100","user_login":"kate","user_name":"Kate"}}}"#;
        assert_eq!(
            decode(revoke).unwrap(),
            Message::AuthRevoked {
                message_id: "v".into(),
                user_id: "100".into()
            }
        );
        let other = r#"{"metadata":{"message_id":"o","message_type":"notification"},
            "payload":{"subscription":{"id":"sub4","type":"channel.follow","condition":{}},"event":{}}}"#;
        assert!(
            matches!(decode(other).unwrap(), Message::Event { subscription, .. } if subscription.kind == "channel.follow")
        );
    }
}
