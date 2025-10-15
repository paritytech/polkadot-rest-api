use axum::{
    Router,
    routing::get
};

use crate::{
    state::AppState,
    handlers::health
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health::get_health))
}