use server::{app, state, logging};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
enum MainError {
    #[error("Failed to initialize application state: {0}")]
    StateInitFailed(#[from] state::StateError),

    #[error("Failed to initialize logging: {0}")]
    LoggingInitFailed(#[from] logging::LoggingError),

    #[error("Invalid bind host address: {0}")]
    InvalidBindHost(#[from] std::net::AddrParseError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    let state = state::AppState::new().await?;
    // Extract values we need before cloning state
    let log_level = state.config.log.level.clone();
    let log_json = state.config.log.json;
    let log_strip_ansi = state.config.log.strip_ansi;
    let log_write = state.config.log.write;
    let log_write_path = state.config.log.write_path.clone();
    let log_write_max_file_size = state.config.log.write_max_file_size;
    let log_write_max_files = state.config.log.write_max_files;
    let bind_host = state.config.express.bind_host.clone();
    let port = state.config.express.port;
    let keep_alive_timeout = state.config.express.keep_alive_timeout;
    let substrate_url = state.config.substrate.url.clone();
    let multi_chain_urls = state.config.substrate.multi_chain_urls.clone();
    let chain_info = state.chain_info.clone();
    let metrics_enabled = state.config.metrics.enabled;
    let metrics_host = state.config.metrics.prom_host.clone();
    let metrics_port = state.config.metrics.prom_port;
    let metrics_prefix = state.config.metrics.prometheus_prefix.clone();
    let loki_host = state.config.metrics.loki_host.clone();
    let loki_port = state.config.metrics.loki_port;

    // Build Loki URL if metrics are enabled (Loki is part of metrics/observability stack)
    let loki_url = if metrics_enabled {
        Some(format!("http://{}:{}", loki_host, loki_port))
    } else {
        None
    };

    logging::init(
        &log_level,
        log_json,
        log_strip_ansi,
        log_write,
        &log_write_path,
        log_write_max_file_size,
        log_write_max_files,
        loki_url.as_deref(),
    )?;

    // Parse bind_host to IpAddr
    let ip: IpAddr = bind_host.parse()?;

    // Security warning for binding to all interfaces
    if bind_host == "0.0.0.0" || bind_host == "::" {
        tracing::warn!(
            "Server is binding to {} (all interfaces). Ensure this is intentional for security reasons.",
            bind_host
        );
    }

    let app = app::create_app(state);
    let addr = SocketAddr::new(ip, port);
    // Initialize metrics if enabled
    if metrics_enabled {
        server::metrics::init(&metrics_prefix);
        tracing::info!(
            "Prometheus metrics enabled at http://{}:{}/metrics (prefix: {})",
            metrics_host,
            metrics_port,
            metrics_prefix
        );
        tracing::info!(
            "Prometheus metrics (JSON) enabled at http://{}:{}/metrics.json",
            metrics_host,
            metrics_port
        );
    }

    tracing::warn!("Starting server on {}", addr);
    tracing::info!("Log level: {}", log_level);
    if log_write {
        tracing::info!("File logging enabled: {}/logs.log", log_write_path);
    }
    tracing::info!("Primary substrate URL: {}", substrate_url);
    tracing::info!(
        "Chain: {} (type: {:?}, spec version: {})",
        chain_info.spec_name,
        chain_info.chain_type,
        chain_info.spec_version
    );

    if !multi_chain_urls.is_empty() {
        tracing::info!("Multi-chain configuration:");
        for chain_url in &multi_chain_urls {
            tracing::info!("  - {} (type: {:?})", chain_url.url, chain_url.chain_type);
        }
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Configure TCP keepalive on the listener's socket
    let socket = socket2::Socket::from(listener.into_std()?);
    let keepalive =
        socket2::TcpKeepalive::new().with_time(Duration::from_millis(keep_alive_timeout));
    socket.set_tcp_keepalive(&keepalive)?;
    let listener = tokio::net::TcpListener::from_std(socket.into())?;

    axum::serve(listener, app).await?;

    Ok(())
}
