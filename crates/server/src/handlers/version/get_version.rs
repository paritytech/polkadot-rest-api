// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VersionResponse {
    pub version: String,
}

#[utoipa::path(
    get,
    path = "/v1/version",
    tag = "version",
    summary = "API version",
    description = "Returns the current version of the Polkadot REST API.",
    responses(
        (status = 200, description = "API version", body = VersionResponse)
    )
)]
pub async fn get_version() -> (StatusCode, Json<VersionResponse>) {
    let response = VersionResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    (StatusCode::OK, Json(response))
}
