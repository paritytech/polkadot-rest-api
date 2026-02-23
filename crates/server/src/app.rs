// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{logging::http_logger_middleware, openapi::ApiDoc, routes, state::AppState};
use axum::{
    Router,
    http::{StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use include_dir::{Dir, include_dir};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};
use utoipa::OpenApi;

static DOCS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../docs/dist");

async fn serve_docs(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches("/docs");
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match DOCS_DIR.get_file(path) {
        Some(file) => {
            let mime = match path.rsplit('.').next() {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "application/javascript",
                Some("ico") => "image/x-icon",
                Some("txt") => "text/plain",
                _ => "application/octet-stream",
            };
            ([(header::CONTENT_TYPE, mime)], file.contents()).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub fn create_app(state: AppState) -> Router {
    let request_limit = state.config.express.request_limit;
    let metrics_enabled = state.config.metrics.enabled;
    let registry = &state.route_registry;

    let rc_routes = Router::new()
        .merge(routes::accounts::accounts_routes(registry))
        .merge(routes::blocks::blocks_routes(registry))
        .merge(routes::pallets::routes(
            registry,
            &state.chain_info.chain_type,
        ))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::rc_format::rc_format_middleware,
        ));

    // Create v1 API router with route registration
    // All routes are mounted unconditionally - runtime metadata validation happens in handlers
    let v1_routes = Router::new()
        .route("/", get(routes::root::root_handler))
        .merge(routes::ahm::routes(registry))
        .merge(routes::capabilities::routes(registry))
        .merge(routes::coretime::routes(
            registry,
            &state.chain_info.chain_type,
        ))
        .merge(routes::health::routes(registry))
        .merge(routes::node::routes(registry))
        .merge(routes::paras::routes(
            registry,
            &state.chain_info.chain_type,
        ))
        .merge(routes::rc::routes(registry, &state.chain_info.chain_type))
        .merge(routes::runtime::routes(registry))
        .merge(routes::transaction::routes(
            registry,
            &state.chain_info.chain_type,
        ))
        .merge(routes::version::routes(registry))
        .with_state(state.clone())
        .merge(rc_routes);

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
    let mut app = Router::new()
        .nest("/v1", v1_routes)
        .route(
            "/api-docs/openapi.json",
            get(|| async { axum::Json(ApiDoc::openapi()) }),
        )
        // Serve embedded docs static site at /docs
        .route("/docs", get(serve_docs))
        .route("/docs/*path", get(serve_docs));

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
