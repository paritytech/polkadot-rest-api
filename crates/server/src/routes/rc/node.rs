// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::handlers::rc::node;
use crate::routes::{API_VERSION, RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};

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
