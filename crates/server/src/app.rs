use crate::{logging::http_logger_middleware, routes, state::AppState};
use axum::{Router, middleware, routing::get};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

pub fn create_app(state: AppState) -> Router {
    let request_limit = state.config.express.request_limit;
    let metrics_enabled = state.config.metrics.enabled;
    let registry = &state.route_registry;

    // Create v1 API router with route registration
    // All routes are mounted unconditionally - runtime metadata validation happens in handlers
    let v1_routes = Router::new()
        .route("/", get(routes::root::root_handler))
        .merge(routes::ahm::routes(registry))
        .merge(routes::blocks::blocks_routes(registry))
        .merge(routes::health::routes(registry))
        .merge(routes::runtime::routes(registry))
        .merge(routes::version::routes(registry))
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

    app.layer(middleware::from_fn(http_logger_middleware))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(request_limit))
        .with_state(state)
}
