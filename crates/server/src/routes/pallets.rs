use axum::{Router, routing::get};

use crate::{
    handlers::pallets,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/:palletId/storage",
            "get",
            get(pallets::get_pallets_storage),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/:palletId/consts",
            "get",
            get(pallets::get_pallets_consts),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/:palletId/consts/:constantId",
            "get",
            get(pallets::get_pallet_const_item),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/assets/:asset_id/asset-info",
            "get",
            get(pallets::pallets_assets_asset_info),
        )
}

