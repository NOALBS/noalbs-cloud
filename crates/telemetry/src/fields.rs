//! Span and event fields on stdout, like tracing-subscriber's default but without
//! `otel.*`: the exporter's span names and kinds would be repeated on every line
//! inside a request.

use std::fmt;

use tracing::field::{Field, Visit};
use tracing_subscriber::{
    field::RecordFields,
    fmt::{FormatFields, format::Writer},
};

pub struct StdoutFields;

impl<'writer> FormatFields<'writer> for StdoutFields {
    fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> fmt::Result {
        let mut visitor = Visitor {
            writer,
            result: Ok(()),
            first: true,
        };
        fields.record(&mut visitor);
        visitor.result
    }
}

struct Visitor<'a> {
    writer: Writer<'a>,
    result: fmt::Result,
    first: bool,
}

impl Visit for Visitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_debug(field, &value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let name = field.name();
        // `log.*` are the `log` crate's metadata, which the default leaves out too.
        if self.result.is_err() || name.starts_with("otel.") || name.starts_with("log.") {
            return;
        }
        let separator = if self.first { "" } else { " " };
        self.first = false;
        self.result = if name == "message" {
            write!(self.writer, "{separator}{value:?}")
        } else {
            let name = name.strip_prefix("r#").unwrap_or(name);
            write!(self.writer, "{separator}{name}={value:?}")
        };
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use tracing_subscriber::layer::SubscriberExt;

    use super::StdoutFields;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn leaves_out_otel_fields() {
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .without_time()
            .fmt_fields(StdoutFields)
            .with_writer(move || writer.clone());
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "request",
                otel.name = "GET /x",
                http.route = "/x",
                user.id = tracing::field::Empty,
            );
            span.record("user.id", 7);
            span.in_scope(|| tracing::info!(count = 2, "Hello"));
        });

        let line = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert_eq!(
            line.trim(),
            r#"INFO request{http.route="/x" user.id=7}: telemetry::fields::tests: Hello count=2"#
        );
    }
}
