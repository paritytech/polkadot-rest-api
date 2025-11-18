use axum::{Router, routing::get};

use crate::{handlers::ahm, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/ahm-info", get(ahm::ahm_info))
}
