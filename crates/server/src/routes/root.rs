//! Root endpoint handler.
//!
//! Returns API information and a list of all available routes,
//! similar to substrate-api-sidecar's root endpoint.

use crate::state::AppState;
use axum::{Json, extract::State};
use serde_json::{Value, json};

/// Handler for GET /
///
/// Returns API metadata and a list of all available routes.
pub async fn root_handler(State(state): State<AppState>) -> Json<Value> {
    let routes = state.route_registry.routes();

    Json(json!({
        "docs": "https://github.com/paritytech/polkadot-rest-api",
        "github": "https://github.com/paritytech/polkadot-rest-api",
        "version": env!("CARGO_PKG_VERSION"),
        "listen": format!("{}:{}", state.config.express.bind_host, state.config.express.port),
        "routes": routes
    }))
}
