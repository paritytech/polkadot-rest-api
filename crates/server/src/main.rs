mod app;
mod state;
mod routes;
mod handlers;

use std::net::SocketAddr;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let state = state::AppState::new().await?;

    let app = app::create_app(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app).await?;

    Ok(())
}
