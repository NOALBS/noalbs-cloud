//! Carries a trace across NATS (W3C `traceparent` in the message headers), so a
//! dashboard click or chat command and the obs-link work it causes are one trace.
//!
//! Senders use [`request`] / [`publish`] instead of the client's own; receivers
//! make a span and [`follow`] the sender's.

use async_nats::{
    Client, HeaderMap, Message, PublishError, RequestError, Subject, subject::ToSubject,
};
use opentelemetry::{
    global,
    propagation::{Extractor, Injector},
    trace::TraceContextExt,
};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub use async_nats::HeaderMap as Headers;

/// A span for handling a NATS message as part of the sender's trace, or
/// `Span::none()` if the sender wasn't traced (an untraced message starts no
/// trace). A macro so the span's target is the caller's crate, which may start
/// traces (see the crate doc):
/// `telemetry::follow_span!(message.headers.as_ref(), "belabox.command", kind = "start")`.
#[macro_export]
macro_rules! follow_span {
    ($headers:expr, $name:literal $(, $($fields:tt)*)?) => {{
        let headers: Option<&$crate::nats::Headers> = $headers;
        if $crate::nats::traced(headers) {
            let span = ::tracing::info_span!($name, otel.kind = "server" $(, $($fields)*)?);
            $crate::nats::follow(&span, headers);
            span
        } else {
            ::tracing::Span::none()
        }
    }};
}

/// The current span's trace context as NATS headers (empty outside a trace).
pub fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    let cx = Span::current().context();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&cx, &mut Inject(&mut headers));
    });
    headers
}

/// Makes `span` a child of the sender's span, if the message came with one.
/// Returns whether it did.
pub fn follow(span: &Span, headers: Option<&HeaderMap>) -> bool {
    let Some(headers) = headers else {
        return false;
    };
    let cx = global::get_text_map_propagator(|propagator| propagator.extract(&Extract(headers)));
    if !cx.span().span_context().is_valid() {
        return false;
    }
    span.set_parent(cx).is_ok()
}

/// Whether the message carries a trace to follow.
pub fn traced(headers: Option<&HeaderMap>) -> bool {
    headers.is_some_and(|headers| headers.get(TRACEPARENT).is_some())
}

/// `client.request`, with the current trace.
pub async fn request(
    client: &Client,
    subject: impl ToSubject,
    payload: impl Into<bytes::Bytes>,
) -> Result<Message, RequestError> {
    client
        .request_with_headers(subject, headers(), payload.into())
        .await
}

/// `client.publish`, with the current trace.
pub async fn publish(
    client: &Client,
    subject: impl ToSubject,
    payload: impl Into<bytes::Bytes>,
) -> Result<(), PublishError> {
    let subject: Subject = subject.to_subject();
    client
        .publish_with_headers(subject, headers(), payload.into())
        .await
}

const TRACEPARENT: &str = "traceparent";

struct Inject<'a>(&'a mut HeaderMap);

impl Injector for Inject<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key, value);
    }
}

struct Extract<'a>(&'a HeaderMap);

impl Extractor for Extract<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|value| value.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(name, _)| name.as_ref()).collect()
    }
}

#[cfg(test)]
mod tests {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracerProvider};
    use tracing::Span;
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    #[test]
    fn a_trace_crosses_nats_headers() {
        global::set_text_map_propagator(TraceContextPropagator::new());
        let provider = SdkTracerProvider::builder().build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("test")));

        tracing::subscriber::with_default(subscriber, || {
            let sender = tracing::info_span!("sender");
            let headers = sender.in_scope(headers);
            assert!(traced(Some(&headers)));

            let receiver = tracing::info_span!(parent: None, "receiver");
            assert!(follow(&receiver, Some(&headers)));
            assert_eq!(
                receiver.context().span().span_context().trace_id(),
                sender.context().span().span_context().trace_id(),
            );
        });
    }

    #[test]
    fn nothing_to_follow_outside_a_trace() {
        global::set_text_map_propagator(TraceContextPropagator::new());
        let headers = headers();
        assert!(!traced(Some(&headers)));
        assert!(!follow(&Span::none(), Some(&headers)));
        assert!(!follow(&Span::none(), None));
    }
}
