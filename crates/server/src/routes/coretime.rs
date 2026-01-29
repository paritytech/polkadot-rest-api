use axum::{Router, routing::get};

use crate::{
    handlers::coretime,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

/// Creates routes for coretime-related endpoints.
///
/// These endpoints query the Broker pallet on coretime chains:
/// - GET /v1/coretime/leases - Get all registered leases
pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(
        registry,
        API_VERSION,
        "/coretime/leases",
        "get",
        get(coretime::coretime_leases),
    )
}
