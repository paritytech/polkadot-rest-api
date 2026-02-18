// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{
    Router,
    routing::{get, post},
};
use polkadot_rest_api_config::ChainType;

use crate::{
    handlers::transaction,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

/// Create transaction routes.
pub fn routes(registry: &RouteRegistry, chain_type: &ChainType) -> Router<AppState> {
    let router = Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/transaction",
            "post",
            post(transaction::submit),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/transaction/dry-run",
            "post",
            post(transaction::dry_run),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/transaction/fee-estimate",
            "post",
            post(transaction::fee_estimate),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/transaction/material",
            "get",
            get(transaction::material),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/transaction/material/:metadataVersion",
            "get",
            get(transaction::material_versioned),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/transaction/metadata-blob",
            "post",
            post(transaction::metadata_blob),
        );

    // Only register /rc/ routes for parachains, not relay chains
    if *chain_type != ChainType::Relay {
        router
            .route_registered(
                registry,
                API_VERSION,
                "/rc/transaction",
                "post",
                post(transaction::submit_rc),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/rc/transaction/dry-run",
                "post",
                post(transaction::dry_run_rc),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/rc/transaction/fee-estimate",
                "post",
                post(transaction::fee_estimate_rc),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/rc/transaction/material",
                "get",
                get(transaction::material_rc),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/rc/transaction/material/:metadataVersion",
                "get",
                get(transaction::material_versioned_rc),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/rc/transaction/metadata-blob",
                "post",
                post(transaction::metadata_blob_rc),
            )
    } else {
        router
    }
}
