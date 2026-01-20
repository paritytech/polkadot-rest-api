use crate::handlers::pallets;
use crate::routes::{API_VERSION, RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};

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
}
