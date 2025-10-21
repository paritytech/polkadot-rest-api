use anyhow::Result;
use std::path::Path;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize tracing/logging with the specified level and format
///
/// # Arguments
/// * `level` - Log level (trace, debug, info, warn, error)
/// * `json_format` - If true, output logs in JSON format
/// * `strip_ansi` - If true, disable ANSI color codes in logs
/// * `write_to_file` - If true, write logs to a file
/// * `write_path` - Directory path to write log files
/// * `_write_max_file_size` - Maximum file size in bytes (currently unused, see TODO)
///
/// # Examples
/// ```
/// init("debug", false, false, false, "./logs", 5242880)?; // Console only
/// init("info", true, false, true, "./logs", 5242880)?;    // JSON format to console and file
/// ```
///
/// # TODO
/// - Implement size-based log rotation using write_max_file_size
/// - Currently uses a single log file without rotation
pub fn init(
    level: &str,
    json_format: bool,
    strip_ansi: bool,
    write_to_file: bool,
    write_path: &str,
    _write_max_file_size: u64,
) -> Result<()> {
    // Create filter from level
    let filter = EnvFilter::try_new(level).unwrap_or_else(|e| {
        eprintln!(
            "Invalid log level '{}': {}. Falling back to 'info'",
            level, e
        );
        EnvFilter::new("info")
    });

    // Build the subscriber based on config
    let registry = tracing_subscriber::registry();

    if write_to_file {
        // Ensure log directory exists
        let log_dir = Path::new(write_path);
        std::fs::create_dir_all(log_dir)?;

        // Create file appender
        // TODO: Implement size-based rotation. Currently using rolling::never which creates a single file.
        let file_appender = tracing_appender::rolling::never(write_path, "logs.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        // We need to keep the guard alive for the lifetime of the program
        // Leak it to prevent dropping
        std::mem::forget(_guard);

        if json_format {
            // JSON format for both console and file
            let console_layer = fmt::layer().json();
            let file_layer = fmt::layer().json().with_writer(non_blocking);

            registry
                .with(filter)
                .with(console_layer)
                .with(file_layer)
                .init();
        } else {
            // Human-readable format for both console and file
            let console_layer = fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(true)
                .with_line_number(true)
                .with_ansi(!strip_ansi);

            let file_layer = fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(true)
                .with_line_number(true)
                .with_ansi(false) // Never use ANSI in files
                .with_writer(non_blocking);

            registry
                .with(filter)
                .with(console_layer)
                .with(file_layer)
                .init();
        }
    } else {
        // Console output only
        if json_format {
            let fmt_layer = fmt::layer().json();
            registry.with(filter).with(fmt_layer).init();
        } else {
            let fmt_layer = fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(true)
                .with_line_number(true)
                .with_ansi(!strip_ansi);

            registry.with(filter).with(fmt_layer).init();
        }
    }

    Ok(())
}
