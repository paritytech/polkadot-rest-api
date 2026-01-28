use axum::{
    Router,
    routing::{get, post},
};
use config::ChainType;

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
            post(|state, body| transaction::submit(state, body, false)),
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
        );

    // Only register /rc/ routes for parachains, not relay chains
    if *chain_type != ChainType::Relay {
        router
            .route_registered(
                registry,
                API_VERSION,
                "/rc/transaction",
                "post",
                post(|state, body| transaction::submit(state, body, true)),
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
    } else {
        router
    }
}
