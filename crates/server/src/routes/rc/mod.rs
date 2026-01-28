pub mod blocks;
pub mod node;

use crate::routes::RouteRegistry;
use crate::state::AppState;
use axum::Router;

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .merge(blocks::routes(registry))
        .merge(node::routes(registry))
}
