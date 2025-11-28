use crate::{logging::http_logger_middleware, routes, state::AppState};
use axum::{Router, middleware};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

pub fn create_app(state: AppState) -> Router {
    let request_limit = state.config.express.request_limit;
    let metrics_enabled = state.config.metrics.enabled;

    // Create v1 API router
    let v1_routes = Router::new()
        .merge(routes::ahm::routes())
        .merge(routes::blocks::blocks_routes())
        .merge(routes::health::routes())
        .merge(routes::runtime::routes())
        .merge(routes::version::routes())
        .with_state(state.clone());

    // Apply metrics middleware if enabled (needs to be after with_state)
    let v1_routes = if metrics_enabled {
        v1_routes.layer(middleware::from_fn_with_state(
            state.clone(),
            crate::metrics::metrics_middleware,
        ))
    } else {
        v1_routes
    };

    // Build root router
    let mut app = Router::new().nest("/v1", v1_routes);

    // Add metrics endpoints if enabled (separate from v1 routes, no prefix)
    if metrics_enabled {
        app = app.merge(routes::metrics::routes());
    }

    app.layer(CorsLayer::permissive())
        .layer(middleware::from_fn(http_logger_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(request_limit))
        .with_state(state)
}
