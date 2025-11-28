use rolling_file::*;
use std::path::PathBuf;
use thiserror::Error;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("Invalid log level '{level}': {source}")]
    InvalidLogLevel {
        level: String,
        #[source]
        source: tracing_subscriber::filter::ParseError,
    },

    #[error("Failed to create log directory or file appender: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse Loki URL '{url}': {source}")]
    InvalidLokiUrl {
        url: String,
        #[source]
        source: url::ParseError,
    },

    #[error("Failed to configure Loki integration: {0}")]
    LokiError(#[from] tracing_loki::Error),
}

/// Configuration for logging initialization
pub struct LoggingConfig<'a> {
    pub level: &'a str,
    pub json_format: bool,
    pub strip_ansi: bool,
    pub write_to_file: bool,
    pub write_path: &'a str,
    pub write_max_file_size: u64,
    pub write_max_files: usize,
    pub loki_url: Option<&'a str>,
}

/// Initialize tracing/logging with the specified configuration
///
/// # Arguments
/// * `config` - LoggingConfig struct containing all logging parameters
///
/// # Examples
/// ```no_run
/// use server::logging::{self, LoggingConfig};
///
/// // Console only
/// logging::init_with_config(LoggingConfig {
///     level: "debug",
///     json_format: false,
///     strip_ansi: false,
///     write_to_file: false,
///     write_path: "./logs",
///     write_max_file_size: 5242880,
///     write_max_files: 5,
///     loki_url: None,
/// })?;
///
/// // With Loki logging (sends logs to Loki aggregation server)
/// logging::init_with_config(LoggingConfig {
///     level: "info",
///     json_format: true,
///     strip_ansi: false,
///     write_to_file: true,
///     write_path: "./logs",
///     write_max_file_size: 5242880,
///     write_max_files: 5,
///     loki_url: Some("http://localhost:3100"),
/// })?;
/// # Ok::<(), server::logging::LoggingError>(())
/// ```
///
/// # Loki Integration
/// When a Loki URL is provided, logs are sent asynchronously to the Loki server
/// with the following default labels:
/// - `service`: "polkadot-rest-api"
/// - `pid`: Current process ID
///
/// # Log Rotation
/// When a log file reaches `write_max_file_size`, it is rotated:
/// - Current: logs.log
/// - After rotation: logs.log.1, logs.log.2, etc.
/// - Keeps up to `write_max_files` rotated files
pub fn init_with_config(config: LoggingConfig) -> Result<(), LoggingError> {
    let level = config.level;
    let json_format = config.json_format;
    let strip_ansi = config.strip_ansi;
    let write_to_file = config.write_to_file;
    let write_path = config.write_path;
    let write_max_file_size = config.write_max_file_size;
    let write_max_files = config.write_max_files;
    let loki_url = config.loki_url;
    // Create filter from level
    // Resolve "http" log level to "debug" for the filter
    let filter_level = if level == "http" {
        // Translate to target filter
        "info,http=debug"
    } else {
        // Standard level
        level
    };

    let filter =
        EnvFilter::try_new(filter_level).map_err(|source| LoggingError::InvalidLogLevel {
            level: level.to_string(),
            source,
        })?;

    // Build the subscriber based on config
    let registry = tracing_subscriber::registry();

    // Create Loki layer if URL is provided
    let loki_layer = if let Some(url) = loki_url {
        let parsed_url = url::Url::parse(url).map_err(|source| LoggingError::InvalidLokiUrl {
            url: url.to_string(),
            source,
        })?;

        // Create Loki layer with default labels
        let (loki_layer, task) = tracing_loki::builder()
            .label("service", "polkadot-rest-api")?
            .extra_field("pid", format!("{}", std::process::id()))?
            .build_url(parsed_url)?;

        // Spawn the Loki task to send logs in the background
        tokio::spawn(task);

        Some(loki_layer)
    } else {
        None
    };

    if write_to_file {
        // Ensure log directory exists
        std::fs::create_dir_all(write_path)?;

        // Create size-based rolling file appender
        let log_file_path = PathBuf::from(write_path).join("logs.log");
        // write_max_files includes the current file, so subtract 1 for rotated files count
        // e.g., if write_max_files=5: logs.log (current) + logs.log.{1,2,3,4} (4 rotated)
        let rotated_files_count = write_max_files.saturating_sub(1);
        let file_appender = BasicRollingFileAppender::new(
            log_file_path,
            RollingConditionBasic::new().max_size(write_max_file_size),
            rotated_files_count,
        )?;

        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        // We need to keep the guard alive for the lifetime of the program
        // Leak it to prevent dropping
        std::mem::forget(_guard);

        if json_format {
            // JSON format for both console and file
            let console_layer = fmt::layer().json();
            let file_layer = fmt::layer().json().with_writer(non_blocking);

            if let Some(loki) = loki_layer {
                registry
                    .with(filter)
                    .with(console_layer)
                    .with(file_layer)
                    .with(loki)
                    .init();
            } else {
                registry
                    .with(filter)
                    .with(console_layer)
                    .with(file_layer)
                    .init();
            }
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

            if let Some(loki) = loki_layer {
                registry
                    .with(filter)
                    .with(console_layer)
                    .with(file_layer)
                    .with(loki)
                    .init();
            } else {
                registry
                    .with(filter)
                    .with(console_layer)
                    .with(file_layer)
                    .init();
            }
        }
    } else {
        // Console output only
        if json_format {
            let fmt_layer = fmt::layer().json();
            if let Some(loki) = loki_layer {
                registry.with(filter).with(fmt_layer).with(loki).init();
            } else {
                registry.with(filter).with(fmt_layer).init();
            }
        } else {
            let fmt_layer = fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(true)
                .with_line_number(true)
                .with_ansi(!strip_ansi);

            if let Some(loki) = loki_layer {
                registry.with(filter).with(fmt_layer).with(loki).init();
            } else {
                registry.with(filter).with(fmt_layer).init();
            }
        }
    }

    Ok(())
}

/// Initialize tracing/logging (legacy function - prefer init_with_config)
///
/// This function is provided for backward compatibility. New code should use `init_with_config`.
#[allow(clippy::too_many_arguments)]
pub fn init(
    level: &str,
    json_format: bool,
    strip_ansi: bool,
    write_to_file: bool,
    write_path: &str,
    write_max_file_size: u64,
    write_max_files: usize,
    loki_url: Option<&str>,
) -> Result<(), LoggingError> {
    init_with_config(LoggingConfig {
        level,
        json_format,
        strip_ansi,
        write_to_file,
        write_path,
        write_max_file_size,
        write_max_files,
        loki_url,
    })
}
