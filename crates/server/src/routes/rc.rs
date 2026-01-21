use crate::handlers::rc::accounts;
use crate::routes::{RegisterRoute, RouteRegistry, API_VERSION};
use crate::state::AppState;
use axum::{routing::get, Router};

pub fn rc_routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/rc/accounts/:accountId/balance-info",
            "get",
            get(accounts::get_balance_info),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/accounts/:accountId/proxy-info",
            "get",
            get(accounts::get_proxy_info),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/accounts/:accountId/staking-info",
            "get",
            get(accounts::get_staking_info),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/accounts/:accountId/vesting-info",
            "get",
            get(accounts::get_vesting_info),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/accounts/:accountId/staking-payouts",
            "get",
            get(accounts::get_staking_payouts),
        )
}
