use axum::{Router, routing::get};
use config::ChainType;

use crate::{
    handlers::coretime,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

/// Creates routes for coretime-related endpoints.
///
/// These endpoints query coretime data based on chain type:
///
/// For coretime chains (with Broker pallet):
/// - GET /v1/coretime/info - Get coretime system information (config, sales, phases)
/// - GET /v1/coretime/leases - Get all registered leases
/// - GET /v1/coretime/renewals - Get all potential renewals
/// - GET /v1/coretime/reservations - Get all registered reservations
///
/// For relay chains (with Coretime pallet):
/// - GET /v1/coretime/info - Get minimal coretime info (broker ID, pallet version)
pub fn routes(registry: &RouteRegistry, chain_type: &ChainType) -> Router<AppState> {
    let router = Router::new();

    match chain_type {
        ChainType::Coretime => {
            // Coretime chain: full broker endpoints
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
                    "/coretime/leases",
                    "get",
                    get(coretime::coretime_leases),
                )
                .route_registered(
                    registry,
                    API_VERSION,
                    "/coretime/reservations",
                    "get",
                    get(coretime::coretime_reservations),
                )
        }
        ChainType::Relay => {
            // Relay chain: minimal coretime info endpoint
            router.route_registered(
                registry,
                API_VERSION,
                "/coretime/info",
                "get",
                get(coretime::coretime_info),
            )
        }
        _ => {
            // Other chain types: no coretime routes by default
            router
        }
    }
}
