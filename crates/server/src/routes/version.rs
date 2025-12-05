use axum::{Router, routing::get};

use crate::{
    handlers::version,
    routes::{RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(
        registry,
        "/v1",
        "/version",
        "get",
        get(version::get_version),
    )
}
