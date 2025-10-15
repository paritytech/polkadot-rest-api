use axum::Router;
use crate::{
    state::AppState,
    routes
};

/// Add traceLayer and cors

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::routes())
        .with_state(state)
}
