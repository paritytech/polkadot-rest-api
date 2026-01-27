use axum::{Router, routing::post};

use crate::{
    handlers::transaction,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

/// Create transaction routes.
pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/transaction",
            "post",
            post(transaction::submit),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/transaction/dry-run",
            "post",
            post(transaction::dry_run),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/transaction",
            "post",
            post(transaction::submit_rc),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/transaction/dry-run",
            "post",
            post(transaction::dry_run_rc),
        )
}
