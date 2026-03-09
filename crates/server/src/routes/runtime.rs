// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{Router, routing::get};

use crate::{
    handlers::runtime,
    routes::{API_VERSION, RegisterRoute, RouteRegistry},
    state::AppState,
};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/runtime/spec",
            "get",
            get(runtime::runtime_spec),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/runtime/code",
            "get",
            get(runtime::runtime_code),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/runtime/metadata",
            "get",
            get(runtime::runtime_metadata),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/runtime/metadata/versions",
            "get",
            get(runtime::runtime_metadata_versions),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/runtime/metadata/:version",
            "get",
            get(runtime::runtime_metadata_versioned),
        )
}
