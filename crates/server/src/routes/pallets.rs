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
            "/pallets/:palletId/dispatchables",
            "get",
            get(pallets::get_pallets_dispatchables),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/:palletId/dispatchables/:dispatchableId",
            "get",
            get(pallets::get_pallet_dispatchable_item),
        )
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
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/pool-assets/:asset_id/asset-info",
            "get",
            get(pallets::pallets_pool_assets_asset_info),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/foreign-assets",
            "get",
            get(pallets::pallets_foreign_assets),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/staking/progress",
            "get",
            get(pallets::pallets_staking_progress),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/staking/validators",
            "get",
            get(pallets::pallets_staking_validators),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/:pallet_id/consts",
            "get",
            get(pallets::pallets_constants),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/:pallet_id/consts/:constant_item_id",
            "get",
            get(pallets::pallets_constant_item),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/nomination-pools/info",
            "get",
            get(pallets::pallets_nomination_pools_info),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/nomination-pools/:pool_id",
            "get",
            get(pallets::pallets_nomination_pools_pool),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/asset-conversion/liquidity-pools",
            "get",
            get(pallets::get_liquidity_pools),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/asset-conversion/next-available-id",
            "get",
            get(pallets::get_next_available_id),
        );

    // Only register /rc/ routes for parachains, not relay chains
    if *chain_type != ChainType::Relay {
        router
            .route_registered(
                registry,
                API_VERSION,
                "/rc/pallets/staking/progress",
                "get",
                get(pallets::rc_pallets_staking_progress),
            )
            .route_registered(
                registry,
                API_VERSION,
                "/rc/pallets/staking/validators",
                "get",
                get(pallets::rc_pallets_staking_validators),
            )
    } else {
        router
    }
}
