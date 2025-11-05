use axum::{Router, routing::get};

use crate::{handlers::runtime, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/runtime/spec", get(runtime::runtime_spec))
}
