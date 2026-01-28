use crate::handlers::rc::accounts;
use crate::routes::{API_VERSION, RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};
use config::ChainType;

pub fn rc_routes(registry: &RouteRegistry, chain_type: &ChainType) -> Router<AppState> {
    let router = Router::new();
    // Only register /rc/ routes for parachains, not relay chains
    if *chain_type != ChainType::Relay {
        router
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
    } else {
        router
    }
}
