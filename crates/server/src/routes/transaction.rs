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
            "/rc/transaction",
            "post",
            post(transaction::submit_rc),
        )
}
