use crate::handlers::blocks;
use crate::routes::{RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn blocks_routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        // Order matters: specific routes must come before /:blockId to avoid capturing as a blockId
        .route_registered(
            registry,
            "/v1",
            "/blocks/head/header",
            "get",
            get(blocks::get_blocks_head_header),
        )
        .route_registered(
            registry,
            "/v1",
            "/blocks/{blockId}",
            "get",
            get(blocks::get_block),
        )
}
