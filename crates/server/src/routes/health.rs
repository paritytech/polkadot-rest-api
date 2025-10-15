use axum::{Router, routing::get};

use crate::{handlers::health, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/health", get(health::get_health))
}
