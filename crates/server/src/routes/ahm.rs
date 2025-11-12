use crate::handlers::ahm;
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn routes() -> Router<AppState> {
    Router::new().route("/ahm-info", get(ahm::get_ahm_info))
}
