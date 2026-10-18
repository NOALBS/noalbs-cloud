//! Logs, traces and metrics for every service.
//!
//! [`init`] always logs to stdout (`RUST_LOG` picks the levels; colours only on a
//! terminal, `LOG_FORMAT=json` for JSON lines). With `OTEL_EXPORTER_OTLP_ENDPOINT`
//! set it also exports over OTLP/HTTP: traces from spans, logs from events and the
//! metrics made with [`meter`]. The exporter reads the standard `OTEL_*` variables
//! (`OTEL_EXPORTER_OTLP_HEADERS` for the backend's credentials,
//! `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_TRACES_SAMPLER`, …), so the backend is config.
//!
//! What gets exported is kept coarse on purpose (a free tier is the budget):
//! - Logs: `info` and up, or what `LOG_EXPORT_FILTER` says (`RUST_LOG` syntax).
//! - Traces: `info` spans. Only the service's own crate starts a trace; library
//!   spans (every `db::` query) are exported only inside one, so a query outside a
//!   request doesn't become a trace of its own. Spans with the [`SCOPE`] target are
//!   never exported: they only put fields such as `switcher.id` on the logs inside
//!   them, for long-lived work like a browser source's connection. Only warnings and
//!   errors are kept as span events (the logs are exported anyway).
//! - The exporter's own crates are never exported, which would loop.
//!
//! [`nats`] carries the trace across NATS requests.

use std::io::IsTerminal;

use opentelemetry::{KeyValue, global, metrics::Meter, trace::TracerProvider as _};
use opentelemetry_appender_tracing::layer::{OpenTelemetryTracingBridge, TracingSpanAttributes};
use opentelemetry_otlp::{Compression, LogExporter, MetricExporter, SpanExporter, WithHttpConfig};
use opentelemetry_sdk::{
    Resource, logs::SdkLoggerProvider, metrics::SdkMeterProvider,
    propagation::TraceContextPropagator, trace::SdkTracerProvider,
};
use tracing::{Level, Metadata};
use tracing_subscriber::{
    EnvFilter, Layer, Registry,
    filter::{self, Directive},
    layer::{Context, SubscriberExt},
    util::SubscriberInitExt,
};

mod fields;
pub mod nats;

/// The target of spans that only add fields to the logs inside them and never
/// become trace spans: `tracing::info_span!(target: telemetry::SCOPE, "connection",
/// switcher.id = tracing::field::Empty)`.
pub const SCOPE: &str = "scope";

/// Span fields copied onto every exported log record inside the span.
const LOG_FIELDS: [&str; 5] = [
    "switcher.id",
    "workspace.id",
    "user.id",
    "connection.kind",
    "chat.platform",
];

/// Targets never exported: the exporter itself and the HTTP stack it runs on.
const NOT_EXPORTED: [&str; 6] = ["opentelemetry", "hyper", "h2", "reqwest", "tonic", "tower"];

/// Built when this was compiled from a release (the Dockerfiles pass `GIT_SHA`).
const VERSION: &str = match option_env!("GIT_SHA") {
    Some(sha) if !sha.is_empty() => sha,
    _ => env!("CARGO_PKG_VERSION"),
};

type BoxedLayer = Box<dyn Layer<Registry> + Send + Sync>;

/// Sets up logging and, when configured, the OTLP export for `service` (e.g.
/// `"obs-link"`, the crate name). Keep the guard and call [`Guard::shutdown`] last,
/// so the final batch goes out.
pub fn init(service: &'static str) -> Guard {
    global::set_text_map_propagator(TraceContextPropagator::new());

    let mut layers = vec![stdout_layer()];
    let mut guard = Guard::default();
    let mut failed = None;
    if non_empty_env("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
        match otlp(service) {
            Ok((otlp_layers, otlp_guard)) => {
                layers.extend(otlp_layers);
                guard = otlp_guard;
            }
            Err(error) => failed = Some(error),
        }
    }

    tracing_subscriber::registry().with(layers).init();

    if let Some(error) = failed {
        tracing::error!(%error, "Couldn't set up the OTLP export; logging to stdout only");
    } else if guard.tracer.is_some() {
        tracing::info!(
            version = VERSION,
            "Exporting logs, traces and metrics over OTLP"
        );
    }
    guard
}

/// The meter for this service's metrics. Without an OTLP endpoint it measures
/// nothing (OpenTelemetry's no-op meter).
pub fn meter() -> Meter {
    global::meter("noalbs")
}

/// Flushes the OTLP export when the service stops.
#[derive(Default)]
pub struct Guard {
    tracer: Option<SdkTracerProvider>,
    logger: Option<SdkLoggerProvider>,
    meter: Option<SdkMeterProvider>,
}

impl Guard {
    /// Sends what's still queued (spans first, so the logs about them follow).
    /// Blocks while it does. Flushed rather than shut down: tasks still winding
    /// down log after this, which a stopped logger would warn about on every line.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        if let Some(tracer) = self.tracer.take()
            && let Err(error) = tracer.force_flush()
        {
            eprintln!("Couldn't flush the trace export: {error}");
        }
        if let Some(logger) = self.logger.take()
            && let Err(error) = logger.force_flush()
        {
            eprintln!("Couldn't flush the log export: {error}");
        }
        if let Some(meter) = self.meter.take()
            && let Err(error) = meter.force_flush()
        {
            eprintln!("Couldn't flush the metric export: {error}");
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.stop();
    }
}

fn stdout_layer() -> BoxedLayer {
    let rust_log = std::env::var("RUST_LOG").unwrap_or_default();
    let mut filter = EnvFilter::from_default_env();
    // The exporter's own chatter (batch sizes, retries) only when asked for.
    if !rust_log.contains("opentelemetry") {
        filter = filter.add_directive(directive("opentelemetry=warn"));
    }

    let json = non_empty_env("LOG_FORMAT").is_some_and(|format| format == "json");
    let layer = tracing_subscriber::fmt::layer();
    if json {
        layer
            .json()
            .with_current_span(true)
            .with_span_list(false)
            .with_filter(filter)
            .boxed()
    } else {
        layer
            .with_ansi(std::io::stdout().is_terminal())
            .fmt_fields(fields::StdoutFields)
            .with_filter(filter)
            .boxed()
    }
}

fn otlp(service: &'static str) -> Result<(Vec<BoxedLayer>, Guard), Box<dyn std::error::Error>> {
    let mut resource = Resource::builder()
        .with_service_name(service)
        .with_attribute(KeyValue::new("service.version", VERSION));
    // The pod's (or container's) name tells replicas apart.
    if let Some(host) = non_empty_env("HOSTNAME") {
        resource = resource.with_attribute(KeyValue::new("service.instance.id", host));
    }
    let resource = resource.build();

    let spans = SpanExporter::builder()
        .with_http()
        .with_compression(Compression::Gzip)
        .build()?;
    let tracer = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(spans)
        .build();
    global::set_tracer_provider(tracer.clone());

    let logs = LogExporter::builder()
        .with_http()
        .with_compression(Compression::Gzip)
        .build()?;
    let logger = SdkLoggerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(logs)
        .build();

    let metrics = MetricExporter::builder()
        .with_http()
        .with_compression(Compression::Gzip)
        .build()?;
    let meter = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metrics)
        .build();
    global::set_meter_provider(meter.clone());

    let own_target = service.replace('-', "_");
    let trace_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer.tracer(service))
        .with_filter(filter::dynamic_filter_fn(move |metadata, cx| {
            exports_span(metadata, cx, &own_target)
        }))
        .boxed();

    let log_layer = OpenTelemetryTracingBridge::builder(&logger)
        .with_tracing_span_attributes(TracingSpanAttributes::allowlist(LOG_FIELDS))
        .build()
        .with_filter(log_filter())
        .boxed();

    let guard = Guard {
        tracer: Some(tracer),
        logger: Some(logger),
        meter: Some(meter),
    };
    Ok((vec![trace_layer, log_layer], guard))
}

/// Which spans (and span events) become trace data. See the module doc.
fn exports_span(metadata: &Metadata<'_>, cx: &Context<'_, Registry>, own_target: &str) -> bool {
    let target = metadata.target();
    if target == SCOPE || not_exported(target) {
        return false;
    }
    if metadata.is_event() {
        return *metadata.level() <= Level::WARN;
    }
    if *metadata.level() > Level::INFO {
        return false;
    }
    // Inside an exported span (this filter's view skips the others), or starting
    // one in the service itself.
    let own = target == own_target
        || target
            .strip_prefix(own_target)
            .is_some_and(|rest| rest.starts_with("::"));
    own || cx.lookup_current().is_some()
}

fn not_exported(target: &str) -> bool {
    NOT_EXPORTED.iter().any(|prefix| target.starts_with(prefix))
}

fn log_filter() -> EnvFilter {
    let mut filter = EnvFilter::builder()
        .with_default_directive(Level::INFO.into())
        .parse_lossy(non_empty_env("LOG_EXPORT_FILTER").unwrap_or_default());
    for target in NOT_EXPORTED {
        filter = filter.add_directive(directive(&format!("{target}=off")));
    }
    filter
}

fn directive(text: &str) -> Directive {
    text.parse().expect("a valid filter directive")
}

/// Like the services' settings, an empty variable counts as unset.
fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use opentelemetry_sdk::trace::InMemorySpanExporter;

    use super::*;

    #[test]
    fn only_the_service_starts_traces() {
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let layer = tracing_opentelemetry::layer()
            .with_tracer(provider.tracer("test"))
            .with_filter(filter::dynamic_filter_fn(|metadata, cx| {
                exports_span(metadata, cx, "telemetry")
            }));
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            // A library span on its own isn't a trace, not even inside a scope.
            tracing::info_span!(target: "db::streams", "lonely").in_scope(|| {});
            tracing::info_span!(target: SCOPE, "connection").in_scope(|| {
                tracing::info_span!(target: "db::streams", "in_scope").in_scope(|| {});
                // The service starts one, and library spans join it.
                tracing::info_span!("request").in_scope(|| {
                    tracing::info_span!(target: "db::streams", "query").in_scope(|| {});
                    tracing::debug_span!("detail").in_scope(|| {});
                });
            });
        });

        let mut names: Vec<_> = exporter
            .get_finished_spans()
            .unwrap()
            .into_iter()
            .map(|span| span.name.into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["query", "request"]);
    }
}
