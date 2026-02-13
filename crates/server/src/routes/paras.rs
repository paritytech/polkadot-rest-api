// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Routes for parachain-related endpoints.
//!
//! These routes are only available when connected to a parachain node,
//! not when connected to a relay chain.

use axum::{Router, routing::get};
use config::ChainType;

use crate::{
    handlers::paras,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

/// Create routes for parachain endpoints.
///
/// These routes are only registered when the connected chain is NOT a relay chain,
/// as they require the `parachainInfo` pallet which only exists on parachains.
pub fn routes(registry: &RouteRegistry, chain_type: &ChainType) -> Router<AppState> {
    let router = Router::new();

    // Only register paras routes for parachains, not relay chains
    if *chain_type != ChainType::Relay {
        router.route_registered(
            registry,
            API_VERSION,
            "/paras/:number/inclusion",
            "get",
            get(paras::get_paras_inclusion),
        )
    } else {
        router
    }
}
