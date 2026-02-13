// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::AppState;
use crate::utils::BlockId;
use axum::{Json, extract::Query, extract::State, http::StatusCode, response::IntoResponse};
use frame_metadata::RuntimeMetadataPrefixed;
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use thiserror::Error;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesResponse {
    chain: String,
    pallets: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AtBlockParam {
    pub at: Option<String>,
}

#[derive(Debug, Error)]
pub enum CapabilitiesError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Failed to fetch metadata: {0}")]
    RpcFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to decode metadata: {0}")]
    DecodeFailed(String),
}

impl IntoResponse for CapabilitiesError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            CapabilitiesError::InvalidBlockParam(_) | CapabilitiesError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            CapabilitiesError::RpcFailed(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            CapabilitiesError::DecodeFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

#[utoipa::path(
    get,
    path = "/v1/capabilities",
    tag = "capabilities",
    summary = "API capabilities",
    description = "Returns the chain name and list of available pallets in the runtime metadata.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Chain capabilities", body = CapabilitiesResponse),
        (status = 400, description = "Invalid block parameter"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_capabilities(
    State(state): State<AppState>,
    Query(params): Query<AtBlockParam>,
) -> Result<Json<CapabilitiesResponse>, CapabilitiesError> {
    let block_id = params.at.map(|s| s.parse::<BlockId>()).transpose()?;
    let resolved = crate::utils::resolve_block(&state, block_id).await?;

    let metadata_hex: String = state
        .rpc_client
        .request(
            "state_getMetadata",
            subxt_rpcs::client::rpc_params![&resolved.hash],
        )
        .await
        .map_err(CapabilitiesError::RpcFailed)?;

    let hex_str = metadata_hex.strip_prefix("0x").unwrap_or(&metadata_hex);
    let metadata_bytes =
        hex::decode(hex_str).map_err(|e| CapabilitiesError::DecodeFailed(e.to_string()))?;

    let metadata = RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])
        .map_err(|e| CapabilitiesError::DecodeFailed(e.to_string()))?;

    let pallets_set: HashSet<String> = crate::utils::capabilities::pallets_in_metadata(&metadata);
    let mut pallets: Vec<String> = pallets_set.into_iter().collect();
    pallets.sort();

    Ok(Json(CapabilitiesResponse {
        chain: state.chain_info.spec_name.clone(),
        pallets,
    }))
}
