use axum::{Router, routing::get};

use crate::{handlers::runtime, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/runtime/spec", get(runtime::runtime_spec))
        // Order matters: specific routes must come before /metadata/:version
        .route("/runtime/metadata/versions", get(runtime::runtime_metadata_versions))
        .route("/runtime/metadata/:version", get(runtime::runtime_metadata_versioned))
        .route("/runtime/metadata", get(runtime::runtime_metadata))
        .route("/runtime/code", get(runtime::runtime_code))
}
