// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::handlers::rc::runtime;
use crate::routes::{API_VERSION, RegisterRoute, RouteRegistry};
use crate::state::AppState;
use axum::{Router, routing::get};

pub fn routes(registry: &RouteRegistry) -> Router<AppState> {
    Router::new()
        .route_registered(
            registry,
            API_VERSION,
            "/rc/runtime/spec",
            "get",
            get(runtime::get_rc_runtime_spec),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/runtime/code",
            "get",
            get(runtime::get_rc_runtime_code),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/runtime/metadata",
            "get",
            get(runtime::get_rc_runtime_metadata),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/runtime/metadata/versions",
            "get",
            get(runtime::get_rc_runtime_metadata_versions),
        )
        .route_registered(
            registry,
            API_VERSION,
            "/rc/runtime/metadata/:version",
            "get",
            get(runtime::get_rc_runtime_metadata_versioned),
        )
}
