use crate::handlers::blocks;
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn blocks_routes() -> Router<AppState> {
    Router::new()
        // Order matters: specific routes must come before /:blockId to avoid capturing as a blockId
        .route("/blocks/head/header", get(blocks::get_blocks_head_header))
        .route("/blocks/:blockId", get(blocks::get_block))
}
