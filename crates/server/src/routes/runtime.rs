use axum::{Router, routing::get};

use crate::{
    handlers::runtime,
    routes::{RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(
        registry,
        "/v1",
        "/runtime/spec",
        "get",
        get(runtime::runtime_spec),
    )
}
