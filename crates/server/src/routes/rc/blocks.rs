use crate::handlers::rc;
use crate::handlers::rc::blocks;
use crate::routes::{API_VERSION, RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/rc/blocks",
            "get",
            get(rc::get_rc_blocks),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/blocks/head/header",
            "get",
            get(blocks::get_rc_blocks_head_header),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/blocks/:blockId/extrinsics-raw",
            "get",
            get(rc::get_rc_block_extrinsics_raw),
        )
}
