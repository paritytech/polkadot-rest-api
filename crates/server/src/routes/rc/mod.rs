// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod accounts;
pub mod blocks;
pub mod node;
pub mod runtime;

use crate::routes::RouteRegistry;
use crate::state::AppState;
use axum::Router;
use polkadot_rest_api_config::ChainType;

pub fn routes(registry: &RouteRegistry, chain_type: &ChainType) -> Router<AppState> {
    let router = Router::new();
    if chain_type != &ChainType::Relay {
        router
            .merge(blocks::routes(registry))
            .merge(node::routes(registry))
            .merge(accounts::routes(registry))
            .merge(runtime::routes(registry))
    } else {
        router
    }
}
