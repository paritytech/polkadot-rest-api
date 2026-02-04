use crate::handlers::rc;
use crate::handlers::rc::blocks as rc_blocks;
use crate::routes::{API_VERSION, RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/rc/blocks/head",
            "get",
            get(rc::get_rc_blocks_head),
        )
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
            get(rc_blocks::get_rc_blocks_head_header),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/blocks/:blockId/header",
            "get",
            get(rc_blocks::get_rc_block_header),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/blocks/:blockId/extrinsics/:extrinsicIndex",
            "get",
            get(rc::get_rc_extrinsic),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/blocks/:blockId/extrinsics-raw",
            "get",
            get(rc::get_rc_block_extrinsics_raw),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/blocks/:blockId",
            "get",
            get(rc_blocks::get_rc_block),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/blocks/:blockId/para-inclusions",
            "get",
            get(rc_blocks::get_rc_block_para_inclusions),
        )
}
