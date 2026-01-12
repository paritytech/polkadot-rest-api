use crate::handlers::accounts;
use crate::routes::{RegisterRoute, RouteRegistry, API_VERSION};
use crate::state::AppState;
use axum::{routing::get, Router};

pub fn accounts_routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new().route_registered(
        registry,
        API_VERSION,
        "/accounts/:accountId/asset-balances",
        "get",
        get(accounts::get_asset_balances),
    )
}
