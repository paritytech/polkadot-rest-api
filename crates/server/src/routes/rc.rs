use crate::handlers::rc::blocks;
use crate::routes::{API_VERSION, RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn rc_routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(
        registry,
        API_VERSION,
        "/rc/blocks/head",
        "get",
        get(blocks::get_rc_blocks_head),
    )
}
