use axum::{Router, routing::get};

use crate::{
    handlers::rc::node,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/rc/node/network",
            "get",
            get(node::get_rc_node_network),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/node/transaction-pool",
            "get",
            get(node::get_rc_node_transaction_pool),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/node/version",
            "get",
            get(node::get_rc_node_version),
        )
}
