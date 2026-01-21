use axum::{Router, routing::get};

use crate::{
    handlers::pallets,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(
        registry,
        API_VERSION,
        "/pallets/assets/:asset_id/asset-info",
        "get",
        get(pallets::pallets_assets_asset_info),
    )
}
