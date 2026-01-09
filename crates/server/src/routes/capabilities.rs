use axum::{Router, routing::get};

use crate::{
    handlers::capabilities,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(
        registry,
        API_VERSION,
        "/capabilities",
        "get",
        get(capabilities::get_capabilities),
    )
}
