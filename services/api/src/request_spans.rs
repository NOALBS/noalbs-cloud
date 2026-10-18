//! A span and a duration measurement for every request.
//!
//! The span is named after the route template (`GET /workspaces/{workspace_id}/…`),
//! never the URI: paths hold invite tokens. The extractors in `permissions.rs`
//! fill in `user.id`, `workspace.id` and `switcher.id`. It ends when the response
//! starts, so a server-sent events feed is a short span, not one per connection.

use std::{sync::LazyLock, time::Instant};

use axum::{
    extract::{MatchedPath, Request},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use opentelemetry::{KeyValue, metrics::Histogram};
use tracing::{Instrument, Span, field::Empty};

use crate::routes::ErrorCode;

/// Routes every open dashboard polls (the preview several times a second, the OBS
/// stats every 2 s), and the health check: measured, but not traced, which would
/// use up a tracing backend's free tier on its own.
const POLLED: [&str; 5] = [
    "/",
    "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/screenshot",
    "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/stats",
    "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/sources",
    "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/details",
];

static DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    telemetry::meter()
        .f64_histogram("http.server.request.duration")
        .with_unit("s")
        .with_description("Time until the response starts")
        .with_boundaries(vec![
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ])
        .build()
});

pub async fn trace_requests(request: Request, next: Next) -> Response {
    let method = request.method().as_str().to_owned();
    // Unmatched paths (404s) are all one route, so scanners can't add series.
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_owned();

    let span = if POLLED.contains(&route.as_str()) {
        Span::none()
    } else {
        tracing::info_span!(
            "request",
            otel.name = format!("{method} {route}"),
            otel.kind = "server",
            otel.status_description = Empty,
            error.type = Empty,
            http.request.method = method,
            http.route = route,
            http.response.status_code = Empty,
            user.id = Empty,
            workspace.id = Empty,
            switcher.id = Empty,
        )
    };

    let started = Instant::now();
    let response = next.run(request).instrument(span.clone()).await;
    let status = response.status();

    span.record("http.response.status_code", status.as_u16());
    let code = response.extensions().get::<ErrorCode>().map(|code| code.0);
    if let Some(code) = code {
        span.record("error.type", code);
    } else if status.is_server_error() {
        span.record("error.type", status.as_str());
    }
    if is_our_failure(status, code) {
        // Marks the span as failed, with the code as its message.
        span.record("otel.status_description", code.unwrap_or("server error"));
    }
    DURATION.record(
        started.elapsed().as_secs_f64(),
        &[
            KeyValue::new("http.request.method", method),
            KeyValue::new("http.route", route),
            // The class, not the code: fewer series.
            KeyValue::new("http.response.status_class", status_class(status.as_u16())),
        ],
    );
    response
}

/// A 5xx that is NOALBS Cloud's problem, so the span is an error. The streamer's
/// OBS, BELABOX or Moblin being offline, refusing or slow (`obs_offline`,
/// `belabox_offline`, `moblin_timeout`, …) is answered with a 5xx too, but it's
/// their setup, not a fault to chase: those keep only their `error.type`.
fn is_our_failure(status: StatusCode, code: Option<&str>) -> bool {
    const DEVICES: [&str; 3] = ["obs_", "belabox_", "moblin_"];
    status.is_server_error()
        && !code.is_some_and(|code| DEVICES.iter().any(|device| code.starts_with(device)))
}

fn status_class(code: u16) -> &'static str {
    match code {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        _ => "5xx",
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::is_our_failure;

    #[test]
    fn device_trouble_isnt_our_failure() {
        let ours = |status, code| is_our_failure(status, code);
        assert!(ours(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some("internal_error")
        ));
        assert!(ours(StatusCode::BAD_GATEWAY, Some("stripe_error")));
        assert!(ours(StatusCode::SERVICE_UNAVAILABLE, None));
        assert!(!ours(StatusCode::SERVICE_UNAVAILABLE, Some("obs_offline")));
        assert!(!ours(StatusCode::GATEWAY_TIMEOUT, Some("obs_timeout")));
        assert!(!ours(
            StatusCode::SERVICE_UNAVAILABLE,
            Some("belabox_offline")
        ));
        assert!(!ours(StatusCode::BAD_GATEWAY, Some("moblin_error")));
        assert!(!ours(StatusCode::NOT_FOUND, Some("not_found")));
    }
}
