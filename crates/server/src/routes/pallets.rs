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
        )
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
            "/pallets/:pallet_id/events",
            "get",
            get(pallets::get_pallet_events),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/pallets/:pallet_id/events/:event_item_id",
            "get",
            get(pallets::get_pallet_event_item),
        )
}
