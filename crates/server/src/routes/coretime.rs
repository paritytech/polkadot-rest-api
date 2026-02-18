// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{Router, routing::get};
use polkadot_rest_api_config::ChainType;

use crate::{
    handlers::coretime,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

/// Creates routes for coretime-related endpoints.
///
/// Endpoints available on BOTH relay and coretime chains:
/// - GET /v1/coretime/info - Get coretime system information (config, sales, phases) in coretime chains
///   and minimal coretime info (broker ID, pallet version) in relay chains
/// - GET /v1/coretime/overview - Core overview (different response structure per chain type)
///
/// Endpoints available ONLY on coretime chains (require Broker pallet):
/// - GET /v1/coretime/leases - Get all registered leases
/// - GET /v1/coretime/regions - Get all registered regions
/// - GET /v1/coretime/renewals - Get all potential renewals
/// - GET /v1/coretime/reservations - Get all registered reservations
pub fn routes(registry: &RouteRegistry, chain_type: &ChainType) -> Router<AppState> {
    let router = Router::new();

    // /coretime/overview is available on BOTH relay and coretime chains
    let router = if *chain_type == ChainType::Relay || *chain_type == ChainType::Coretime {
        router
            .route_registered(
                registry,
                API_VERSION,
                "/coretime/info",
                "get",
                get(coretime::coretime_info),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/coretime/overview",
                "get",
                get(coretime::coretime_overview),
            )
    } else {
        router
    };

    // Other coretime endpoints are only available on coretime chains
    if *chain_type == ChainType::Coretime {
        router
            .route_registered(
                registry,
                API_VERSION,
                "/coretime/leases",
                "get",
                get(coretime::coretime_leases),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/coretime/regions",
                "get",
                get(coretime::coretime_regions),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/coretime/renewals",
                "get",
                get(coretime::coretime_renewals),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/coretime/reservations",
                "get",
                get(coretime::coretime_reservations),
            )
    } else {
        router
    }
}
