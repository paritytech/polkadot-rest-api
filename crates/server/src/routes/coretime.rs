use axum::{Router, routing::get};
use config::ChainType;

use crate::{
    handlers::coretime,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

/// Creates routes for coretime-related endpoints.
///
/// These endpoints query the Broker pallet on coretime chains:
/// - GET /v1/coretime/leases - Get all registered leases
/// - GET /v1/coretime/reservations - Get all registered reservations
///
/// Routes are only registered when connected to a coretime chain.
pub fn routes(registry: &RouteRegistry, chain_type: &ChainType) -> Router<AppState> {
    let router = Router::new();
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
                "/coretime/reservations",
                "get",
                get(coretime::coretime_reservations),
            )
    } else {
        router
    }
}
