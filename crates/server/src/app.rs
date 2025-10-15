use crate::{routes, state::AppState};
use tower_http::{
    trace::TraceLayer,
    cors::CorsLayer
};
use axum::Router;

/// Add traceLayer and cors

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::routes())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
