use crate::{routes, state::AppState};
use axum::Router;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

pub fn create_app(state: AppState) -> Router {
    let request_limit = state.config.express.request_limit;

    Router::new()
        .merge(routes::health::routes())
        .merge(routes::version::routes())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(request_limit))
        .with_state(state)
}
