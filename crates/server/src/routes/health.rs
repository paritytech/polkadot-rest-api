use axum::{Router, routing::get};

use crate::{
    handlers::health,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(
        registry,
        API_VERSION,
        "/health",
        "get",
        get(health::get_health),
    )
}
