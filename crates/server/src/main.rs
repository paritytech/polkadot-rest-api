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
    logging::init(&log_level)?;

    let app = app::create_app(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("Starting server on {}", addr);
    tracing::info!("Log level: {}", log_level);
    tracing::info!("Substrate URL: {}", substrate_url);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
