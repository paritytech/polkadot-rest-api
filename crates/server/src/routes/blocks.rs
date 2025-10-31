use crate::handlers::blocks;
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn blocks_routes() -> Router<AppState> {
    Router::new().route("/blocks/:blockId", get(blocks::get_block))
}
