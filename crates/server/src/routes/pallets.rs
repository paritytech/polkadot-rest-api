use axum::{Router, routing::get};
use config::ChainType;

use crate::{
    handlers::pallets,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry, chain_type: &ChainType) -> Router<AppState> {
    let router = Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/assets/:asset_id/asset-info",
            "get",
            get(pallets::pallets_assets_asset_info),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/staking/progress",
            "get",
            get(pallets::pallets_staking_progress),
        );

    // Only register /rc/ routes for parachains, not relay chains
    if *chain_type != ChainType::Relay {
        router.route_registered(
            registry,
            API_VERSION,
            "/rc/pallets/staking/progress",
            "get",
            get(pallets::rc_pallets_staking_progress),
        )
    } else {
        router
    }
}
