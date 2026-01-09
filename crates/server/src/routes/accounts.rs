use crate::handlers::accounts;
use crate::routes::{API_VERSION, RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn accounts_routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        // Order matters: specific routes must come before /:blockId to avoid capturing as a blockId
        .route_registered(
            registry,
            API_VERSION,
            "/accounts/:address/asset-balances",
            "get",
            get(accounts::get_account_asset_balances),
        )
}
