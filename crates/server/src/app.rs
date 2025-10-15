use crate::{routes, state::AppState};
use axum::Router;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .merge(routes::health::routes())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
