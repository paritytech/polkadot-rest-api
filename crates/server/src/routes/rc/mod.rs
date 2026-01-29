pub mod accounts;
pub mod blocks;
pub mod node;

use crate::routes::RouteRegistry;
use crate::state::AppState;
use axum::Router;
use config::ChainType;

pub fn routes(registry: &RouteRegistry, chain_type: &ChainType) -> Router<AppState> {
    let router = Router::new().merge(blocks::routes(registry, chain_type));

    if *chain_type != ChainType::Relay {
        router
            .merge(node::routes(registry))
            .merge(accounts::routes(registry))
    } else {
        router
    }
}
