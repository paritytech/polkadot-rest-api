use axum::{Router, routing::get};

use crate::{
    handlers::runtime,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/runtime/spec",
            "get",
            get(runtime::runtime_spec),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/runtime/code",
            "get",
            get(runtime::runtime_code),
        )
}
