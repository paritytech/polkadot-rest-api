// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_chains: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/v1/health",
    tag = "health",
    summary = "Health check",
    description = "Returns the health status of the API server.",
    responses(
        (status = 202, description = "API is healthy", body = HealthResponse)
    )
)]
pub async fn get_health(State(_state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let response = HealthResponse {
        status: "ok".to_string(),
        connected_chains: None,
        uptime: None,
    };

    (StatusCode::ACCEPTED, Json(response))
}
