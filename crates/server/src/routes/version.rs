use axum::{Router, routing::get};

use crate::{
    handlers::version,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(
        registry,
        API_VERSION,
        "/version",
        "get",
        get(version::get_version),
    )
}
