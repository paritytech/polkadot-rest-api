// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
    fmt::{
        FmtContext, FormatFields, FormattedFields,
        format::{self, FormatEvent, Writer},
        time::{FormatTime, SystemTime},
    },
    registry::LookupSpan,
};

use nu_ansi_term::{Color, Style};

/// Returns true when the event should display "HTTP" instead of its actual level.
/// This applies to events with target "http" at DEBUG level (2xx/3xx responses).
fn is_http_display_event(event: &Event<'_>) -> bool {
    let meta = event.metadata();
    meta.target() == "http" && *meta.level() == Level::DEBUG
}

/// Human-readable formatter that displays "HTTP" instead of "DEBUG" for HTTP events.
///
/// For HTTP events this renders the line directly (instead of delegating to the
/// standard `Format<Full>` formatter and patching the output), so the custom
/// level label is produced without fragile string replacement.
pub struct HttpAwareFormat {
    inner: format::Format<format::Full>,
    timer: SystemTime,
    use_ansi: bool,
}

impl HttpAwareFormat {
    pub fn new(strip_ansi: bool) -> Self {
        let inner = format::format()
            .with_target(true)
            .with_file(true)
            .with_line_number(true)
            .with_ansi(!strip_ansi);

        Self {
            inner,
            timer: SystemTime,
            use_ansi: !strip_ansi,
        }
    }

    /// Render an HTTP event directly, writing " HTTP" as the level label.
    ///
    /// This intentionally reimplements the rendering sequence from
    /// `Format<Full>::format_event` so that we can
    /// substitute the level string without string replacement.
    fn format_http_event<S, N>(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
        N: for<'a> FormatFields<'a> + 'static,
    {
        let meta = event.metadata();

        let dimmed = if self.use_ansi {
            Style::new().dimmed()
        } else {
            Style::new()
        };
        let bold = if self.use_ansi {
            Style::new().bold()
        } else {
            Style::new()
        };

        if self.use_ansi {
            write!(writer, "{}", dimmed.prefix())?;
            if self.timer.format_time(&mut writer).is_err() {
                writer.write_str("<unknown time>")?;
            }
            write!(writer, "{} ", dimmed.suffix())?;
        } else {
            if self.timer.format_time(&mut writer).is_err() {
                writer.write_str("<unknown time>")?;
            }
            writer.write_char(' ')?;
        }

        if self.use_ansi {
            write!(writer, "{} ", Color::Cyan.paint(" HTTP"))?;
        } else {
            writer.write_str(" HTTP ")?;
        }

        if let Some(scope) = ctx.event_scope() {
            let mut seen = false;
            for span in scope.from_root() {
                write!(writer, "{}", bold.paint(span.metadata().name()))?;
                seen = true;

                let ext = span.extensions();
                if let Some(fields) = &ext.get::<FormattedFields<N>>()
                    && !fields.is_empty()
                {
                    write!(writer, "{}{}{}", bold.paint("{"), fields, bold.paint("}"))?;
                }
                write!(writer, "{}", dimmed.paint(":"))?;
            }

            if seen {
                writer.write_char(' ')?;
            }
        }

        write!(
            writer,
            "{}{} ",
            dimmed.paint(meta.target()),
            dimmed.paint(":")
        )?;

        if let Some(filename) = meta.file() {
            write!(writer, "{}{}", dimmed.paint(filename), dimmed.paint(":"),)?;
        }
        if let Some(line_number) = meta.line() {
            write!(
                writer,
                "{}{}:{} ",
                dimmed.prefix(),
                line_number,
                dimmed.suffix(),
            )?;
        }

        ctx.format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}

impl<S, N> FormatEvent<S, N> for HttpAwareFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        if is_http_display_event(event) {
            self.format_http_event(ctx, writer, event)
        } else {
            self.inner.format_event(ctx, writer, event)
        }
    }
}

/// JSON formatter that replaces `"level":"DEBUG"` with `"level":"HTTP"` for HTTP events.
pub struct HttpAwareJsonFormat {
    inner: format::Format<format::Json>,
}

impl Default for HttpAwareJsonFormat {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpAwareJsonFormat {
    pub fn new() -> Self {
        Self {
            inner: format::format().json(),
        }
    }
}

impl<S, N> FormatEvent<S, N> for HttpAwareJsonFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        if !is_http_display_event(event) {
            return self.inner.format_event(ctx, writer, event);
        }

        let mut buf = String::new();
        let buf_writer = Writer::new(&mut buf);
        self.inner.format_event(ctx, buf_writer, event)?;

        let trimmed = buf.trim_end();
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(mut json) => {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert(
                        "level".to_string(),
                        serde_json::Value::String("http".to_string()),
                    );
                }
                let patched = serde_json::to_string(&json).map_err(|_| fmt::Error)?;
                writer.write_str(&patched)?;
                writeln!(writer)
            }
            Err(_) => {
                // Fallback: write original buffer unchanged
                writer.write_str(&buf)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tracing::{debug, error, warn};
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

    use super::*;

    #[derive(Clone)]
    struct TestWriter {
        buf: Arc<Mutex<Vec<u8>>>,
    }

    impl TestWriter {
        fn new() -> Self {
            Self {
                buf: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn output(&self) -> String {
            let buf = self.buf.lock().unwrap();
            String::from_utf8_lossy(&buf).to_string()
        }
    }

    impl std::io::Write for TestWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.buf.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for TestWriter {
        type Writer = TestWriter;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn test_http_event_shows_http_level() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("debug").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareFormat::new(true))
                    .with_ansi(false)
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        debug!(target: "http", "GET /test 200 10ms");

        let output = writer.output();
        assert!(
            output.contains("HTTP"),
            "Expected 'HTTP' in output, got: {}",
            output
        );
        assert!(
            !output.contains("DEBUG"),
            "Did not expect 'DEBUG' in output, got: {}",
            output
        );
    }

    #[test]
    fn test_non_http_debug_shows_debug() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("debug").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareFormat::new(true))
                    .with_ansi(false)
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        debug!(target: "something_else", "some debug message");

        let output = writer.output();
        assert!(
            output.contains("DEBUG"),
            "Expected 'DEBUG' in output, got: {}",
            output
        );
    }

    #[test]
    fn test_http_warn_stays_warn() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("warn").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareFormat::new(true))
                    .with_ansi(false)
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        warn!(target: "http", "GET /test 404 10ms");

        let output = writer.output();
        assert!(
            output.contains("WARN"),
            "Expected 'WARN' in output, got: {}",
            output
        );
        assert!(
            !output.contains("HTTP"),
            "Did not expect 'HTTP' in output, got: {}",
            output
        );
    }

    #[test]
    fn test_json_http_event_level() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("debug").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareJsonFormat::new())
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        debug!(target: "http", "GET /test 200 10ms");

        let output = writer.output();
        let json: serde_json::Value =
            serde_json::from_str(output.trim()).expect("valid JSON output");
        assert_eq!(
            json["level"], "http",
            "Expected level 'http', got: {}",
            json["level"]
        );
    }

    #[test]
    fn test_json_non_http_unchanged() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("debug").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareJsonFormat::new())
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        debug!(target: "other", "some debug message");

        let output = writer.output();
        let json: serde_json::Value =
            serde_json::from_str(output.trim()).expect("valid JSON output");
        assert_eq!(
            json["level"], "DEBUG",
            "Expected level 'DEBUG', got: {}",
            json["level"]
        );
    }

    #[test]
    fn test_http_event_does_not_replace_debug_in_message() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("debug").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareFormat::new(true))
                    .with_ansi(false)
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        debug!(target: "http", "GET /api/DEBUG/test 200 10ms");

        let output = writer.output();
        assert!(
            output.contains("HTTP"),
            "Expected 'HTTP' level in output, got: {}",
            output
        );
        assert!(
            output.contains("GET /api/DEBUG/test"),
            "Expected 'DEBUG' preserved in message body, got: {}",
            output
        );
    }

    #[test]
    fn test_http_event_with_ansi() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("debug").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareFormat::new(false))
                    .with_ansi(true)
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        debug!(target: "http", "GET /test 200 10ms");

        let output = writer.output();
        assert!(
            output.contains("HTTP"),
            "Expected 'HTTP' in ANSI output, got: {:?}",
            output
        );
        assert!(
            !output.contains("DEBUG"),
            "Did not expect 'DEBUG' in ANSI output, got: {:?}",
            output
        );
        assert!(
            output.contains("GET /test 200 10ms"),
            "Expected message in output, got: {:?}",
            output
        );
        assert!(
            output.contains("\x1b["),
            "Expected ANSI escape codes in output, got: {:?}",
            output
        );
    }

    #[test]
    fn test_http_error_stays_error() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("error").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareFormat::new(true))
                    .with_ansi(false)
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        error!(target: "http", "GET /test 500 10ms");

        let output = writer.output();
        assert!(
            output.contains("ERROR"),
            "Expected 'ERROR' in output, got: {}",
            output
        );
        assert!(
            !output.contains("HTTP"),
            "Did not expect 'HTTP' in output for error-level event, got: {}",
            output
        );
    }

    #[test]
    fn test_http_filter_passes_http_debug_but_not_other_debug() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("info,http=debug").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareFormat::new(true))
                    .with_ansi(false)
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        debug!(target: "http", "GET /test 200 10ms");
        debug!(target: "some_module", "internal debug info");

        let output = writer.output();
        assert!(
            output.contains("HTTP"),
            "Expected HTTP debug event to pass filter, got: {}",
            output
        );
        assert!(
            !output.contains("internal debug info"),
            "Non-HTTP debug event should be filtered out, got: {}",
            output
        );
    }

    #[test]
    fn test_json_http_event_preserves_debug_in_message() {
        let writer = TestWriter::new();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::try_new("debug").unwrap())
            .with(
                fmt::layer()
                    .event_format(HttpAwareJsonFormat::new())
                    .with_writer(writer.clone()),
            );

        let _guard = subscriber.set_default();

        debug!(target: "http", "GET /api/DEBUG/test 200 10ms");

        let output = writer.output();
        let json: serde_json::Value =
            serde_json::from_str(output.trim()).expect("valid JSON output");
        assert_eq!(
            json["level"], "http",
            "Expected level 'http', got: {}",
            json["level"]
        );
        let fields = json["fields"]["message"].as_str().unwrap_or("");
        assert!(
            fields.contains("DEBUG"),
            "Expected 'DEBUG' preserved in message field, got: {}",
            fields
        );
    }
}
