//! The chat service's metrics (exported only with OTLP set up, see the
//! `telemetry` crate). Never a channel or switcher as an attribute: the number of
//! series is the budget.

use std::sync::LazyLock;

use db::chat::ChatPlatform;
use opentelemetry::{KeyValue, metrics::Counter};

static COMMANDS: LazyLock<Counter<u64>> = LazyLock::new(|| {
    telemetry::meter()
        .u64_counter("noalbs.chat.commands")
        .with_description("Chat commands run, by platform and command")
        .build()
});

static REPLIES: LazyLock<Counter<u64>> = LazyLock::new(|| {
    telemetry::meter()
        .u64_counter("noalbs.chat.replies")
        .with_description(
            "Chat messages sent, by platform and outcome (sent, error, rate_limited, no_live_chat)",
        )
        .build()
});

/// A command a chatter may run, about to run (the names come from our catalog).
pub fn command(platform: ChatPlatform, command: &str) {
    COMMANDS.add(
        1,
        &[
            KeyValue::new("chat.platform", platform.as_str()),
            KeyValue::new("chat.command", command.to_owned()),
        ],
    );
}

pub fn reply(platform: ChatPlatform, outcome: &'static str) {
    REPLIES.add(
        1,
        &[
            KeyValue::new("chat.platform", platform.as_str()),
            KeyValue::new("outcome", outcome),
        ],
    );
}
