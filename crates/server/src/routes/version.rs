use axum::{Router, routing::get};

use crate::{handlers::version, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/version", get(version::get_version))
}
