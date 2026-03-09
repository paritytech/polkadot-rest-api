// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{Router, routing::get};

use crate::{
    handlers::node,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/node/network",
            "get",
            get(node::get_node_network),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/node/transaction-pool",
            "get",
            get(node::get_node_transaction_pool),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/node/version",
            "get",
            get(node::get_node_version),
        )
}
