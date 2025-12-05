use axum::{Router, routing::get};

use crate::{
    handlers::ahm,
    routes::{RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(registry, "/v1", "/ahm-info", "get", get(ahm::ahm_info))
}
