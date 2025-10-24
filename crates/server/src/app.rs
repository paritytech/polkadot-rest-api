use crate::{routes, state::AppState};
use axum::Router;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

pub fn create_app(state: AppState) -> Router {
    let request_limit = state.config.express.request_limit;

    // Create v1 API router
    let v1_routes = Router::new()
        .merge(routes::health::routes())
        .merge(routes::runtime::routes())
        .merge(routes::version::routes());

    // Mount v1 routes under /v1 prefix
    Router::new()
        .nest("/v1", v1_routes)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(request_limit))
        .with_state(state)
}
