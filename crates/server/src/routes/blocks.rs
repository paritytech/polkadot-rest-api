use crate::handlers::blocks;
use crate::routes::{API_VERSION, RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn blocks_routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        // Order matters: specific routes must come before /:blockId to avoid capturing as a blockId
        .route_registered(
            registry,
            API_VERSION,
            "/blocks/head/header",
            "get",
            get(blocks::get_blocks_head_header),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/blocks/head",
            "get",
            get(blocks::get_block_head),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/blocks",
            "get",
            get(blocks::get_blocks),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/blocks/:blockId",
            "get",
            get(blocks::get_block),
        )
}
