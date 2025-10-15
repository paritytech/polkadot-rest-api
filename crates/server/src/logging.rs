use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initialize tracing/logging with the specified level and format
///
/// # Arguments
/// * `level` - Log level (trace, debug, info, warn, error)
/// * `json_format` - If true, output logs in JSON format
///
/// # Examples
/// ```
/// init("debug", false)?; // Human-readable
/// init("info", true)?;   // JSON format
/// ```
pub fn init(level: &str, json_format: bool) -> Result<()> {
    // Create filter from level
    let filter = EnvFilter::try_new(level).unwrap_or_else(|e| {
        eprintln!(
            "Invalid log level '{}': {}. Falling back to 'info'",
            level, e
        );
        EnvFilter::new("info")
    });

    if json_format {
        // JSON format
        let fmt_layer = fmt::layer().json();

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .init();
    } else {
        // Human-readable format
        let fmt_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(false)
            .with_file(true)
            .with_line_number(true);

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .init();
    }

    Ok(())
}
