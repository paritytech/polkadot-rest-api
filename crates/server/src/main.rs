mod app;
mod handlers;
mod logging;
mod routes;
mod state;

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = state::AppState::new().await?;
    // Extract values we need before cloning state
    let log_level = state.config.log.level.clone();
    let port = state.config.express.port;
    let substrate_url = state.config.substrate.url.clone();
    let multi_chain_urls = state.config.substrate.multi_chain_urls.clone();
    logging::init(&log_level)?;

    let app = app::create_app(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("Starting server on {}", addr);
    tracing::info!("Log level: {}", log_level);
    tracing::info!("Primary substrate URL: {}", substrate_url);

    if !multi_chain_urls.is_empty() {
        tracing::info!("Multi-chain configuration:");
        for chain_url in &multi_chain_urls {
            tracing::info!("  - {} (type: {:?})", chain_url.url, chain_url.chain_type);
        }
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
