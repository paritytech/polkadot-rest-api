use axum::{Router, routing::get};

use crate::{
    handlers::health,
    routes::{RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(registry, "/v1", "/health", "get", get(health::get_health))
}
