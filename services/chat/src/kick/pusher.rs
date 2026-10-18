//! Kick's site WebSocket (a Pusher app), which is where raids show up: frames
//! are `{"event", "data", "channel"}` with `data` a JSON string (an object in
//! Pusher's own errors). Ported from NOALBS's `chat/kick.rs`; we only read what
//! raids need, everything else (chat messages included) is [`Frame::Other`].

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

const RAID: &str = r"App\Events\ChatMoveToSupportedChannelEvent";

/// A channel's topics, as NOALBS subscribes to them (raids come on one of them).
pub fn topics(channel_id: u64, chatroom_id: u64) -> [String; 2] {
    [
        format!("channel.{channel_id}"),
        format!("chatrooms.{chatroom_id}.v2"),
    ]
}

pub fn subscribe(topic: &str) -> String {
    json!({ "event": "pusher:subscribe", "data": { "auth": "", "channel": topic } }).to_string()
}

pub fn unsubscribe(topic: &str) -> String {
    json!({ "event": "pusher:unsubscribe", "data": { "channel": topic } }).to_string()
}

pub fn ping() -> String {
    json!({ "event": "pusher:ping", "data": {} }).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// The first frame: ping after this long without a frame.
    Connected {
        activity_timeout: Duration,
    },
    Subscribed {
        topic: String,
    },
    Pong,
    Raid(Raid),
    /// Pusher refused something (e.g. a subscription).
    Error(String),
    Other,
}

/// A channel raided out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raid {
    /// The topic it came on.
    pub topic: String,
    /// The raiding channel's slug.
    pub from_slug: String,
    /// The raided channel's name and slug.
    pub to_name: String,
    pub to_slug: String,
}

pub fn decode(text: &str) -> Frame {
    #[derive(Deserialize)]
    struct Raw {
        event: String,
        #[serde(default)]
        data: Value,
        channel: Option<String>,
    }
    let Ok(raw) = serde_json::from_str::<Raw>(text) else {
        return Frame::Other;
    };
    let data = match raw.data {
        Value::String(data) => serde_json::from_str(&data).unwrap_or(Value::Null),
        data => data,
    };
    match raw.event.as_str() {
        "pusher:connection_established" => Frame::Connected {
            activity_timeout: Duration::from_secs(
                data["activity_timeout"].as_u64().unwrap_or(120).max(1),
            ),
        },
        "pusher_internal:subscription_succeeded" => Frame::Subscribed {
            topic: raw.channel.unwrap_or_default(),
        },
        "pusher:pong" => Frame::Pong,
        "pusher:error" => Frame::Error(data["message"].as_str().unwrap_or_default().to_owned()),
        RAID => {
            #[derive(Deserialize)]
            struct Channel {
                slug: String,
            }
            #[derive(Deserialize)]
            struct Hosted {
                username: String,
                slug: String,
            }
            #[derive(Deserialize)]
            struct Event {
                channel: Channel,
                hosted: Hosted,
            }
            match serde_json::from_value::<Event>(data) {
                Ok(event) => Frame::Raid(Raid {
                    topic: raw.channel.unwrap_or_default(),
                    from_slug: event.channel.slug,
                    to_name: event.hosted.username,
                    to_slug: event.hosted.slug,
                }),
                Err(error) => {
                    tracing::warn!(%error, "Couldn't read a Kick raid");
                    Frame::Other
                }
            }
        }
        _ => Frame::Other,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Frame, Raid, decode, subscribe, topics};

    #[test]
    fn requests_are_pusher_frames() {
        assert_eq!(
            topics(668, 669),
            ["channel.668".to_owned(), "chatrooms.669.v2".to_owned()]
        );
        let frame: serde_json::Value = serde_json::from_str(&subscribe("channel.668")).unwrap();
        assert_eq!(
            frame,
            serde_json::json!({"event":"pusher:subscribe","data":{"auth":"","channel":"channel.668"}})
        );
    }

    #[test]
    fn reads_the_connection_and_its_keepalive() {
        let established = r#"{"event":"pusher:connection_established","data":"{\"socket_id\":\"1.2\",\"activity_timeout\":120}"}"#;
        assert_eq!(
            decode(established),
            Frame::Connected {
                activity_timeout: Duration::from_secs(120)
            }
        );
        assert_eq!(
            decode(r#"{"event":"pusher:pong","data":"{}"}"#),
            Frame::Pong
        );
        assert_eq!(
            decode(
                r#"{"event":"pusher_internal:subscription_succeeded","data":"{}","channel":"channel.668"}"#
            ),
            Frame::Subscribed {
                topic: "channel.668".into()
            }
        );
        assert_eq!(
            decode(r#"{"event":"pusher:error","data":{"code":4200,"message":"Try again"}}"#),
            Frame::Error("Try again".into())
        );
    }

    #[test]
    fn reads_a_raid() {
        let raid = r#"{"event":"App\\Events\\ChatMoveToSupportedChannelEvent","data":"{\"channel\":{\"id\":668,\"user_id\":676,\"slug\":\"kate\",\"is_banned\":false},\"slug\":\"bob-2\",\"hosted\":{\"id\":1,\"username\":\"Bob_2\",\"slug\":\"bob-2\",\"viewers_count\":12,\"is_live\":true}}","channel":"channel.668"}"#;
        assert_eq!(
            decode(raid),
            Frame::Raid(Raid {
                topic: "channel.668".into(),
                from_slug: "kate".into(),
                to_name: "Bob_2".into(),
                to_slug: "bob-2".into(),
            })
        );
    }

    #[test]
    fn everything_else_is_ignored() {
        let chat = r#"{"event":"App\\Events\\ChatMessageEvent","data":"{\"id\":\"x\",\"chatroom_id\":669,\"content\":\"hi\",\"type\":\"message\"}","channel":"chatrooms.669.v2"}"#;
        assert_eq!(decode(chat), Frame::Other);
        assert_eq!(decode("not json"), Frame::Other);
        // A raid that doesn't look like one is dropped, not an error.
        assert_eq!(
            decode(r#"{"event":"App\\Events\\ChatMoveToSupportedChannelEvent","data":"{}"}"#),
            Frame::Other
        );
    }
}
