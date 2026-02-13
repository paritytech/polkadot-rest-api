// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for GET /rc/blocks/{blockId}/header
//!
//! Returns the header of a specific relay chain block by block hash or block number.

use crate::handlers::blocks::common::convert_digest_items_to_logs;
use crate::handlers::blocks::types::convert_digest_logs_to_sidecar_format;
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

use crate::handlers::blocks::common::RcBlockHeaderResponse;

/// Error types for /rc/blocks/{blockId}/header endpoint
#[derive(Debug, Error)]
pub enum GetRcBlockHeaderError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Failed to get block header: {0}")]
    HeaderFetchFailed(String),

    #[error("Relay chain API is not configured. Please configure SAS_SUBSTRATE_MULTI_CHAIN_URL")]
    RelayChainNotConfigured,
}

impl IntoResponse for GetRcBlockHeaderError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcBlockHeaderError::RelayChainNotConfigured
            | GetRcBlockHeaderError::InvalidBlockParam(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcBlockHeaderError::HeaderFetchFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Handler for GET /rc/blocks/{blockId}/header
///
/// Returns the header of a relay chain block by block hash or block number.
///
/// # Path Parameters
/// - `blockId`: Block identifier (height number or block hash)
#[utoipa::path(
    get,
    path = "/v1/rc/blocks/{blockId}/header",
    tag = "rc",
    summary = "RC get block header",
    description = "Returns the header of a relay chain block by block hash or block number.",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash")
    ),
    responses(
        (status = 200, description = "Relay chain block header", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_block_header(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
) -> Result<Response, GetRcBlockHeaderError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(GetRcBlockHeaderError::RelayChainNotConfigured)?;

    let block_id_parsed = block_id.parse::<utils::BlockId>()?;

    let block_num_or_ref = match block_id_parsed {
        utils::BlockId::Number(n) => subxt::client::BlockNumberOrRef::Number(n),
        utils::BlockId::Hash(h) => {
            subxt::client::BlockNumberOrRef::BlockRef(subxt::backend::BlockRef::from_hash(h))
        }
    };

    let client_at_block = relay_client
        .at_block(block_num_or_ref)
        .await
        .map_err(|e| GetRcBlockHeaderError::HeaderFetchFailed(e.to_string()))?;

    let header = client_at_block
        .block_header()
        .await
        .map_err(|e| GetRcBlockHeaderError::HeaderFetchFailed(e.to_string()))?;

    let digest_logs = convert_digest_items_to_logs(&header.digest.logs);
    let digest_logs_formatted = convert_digest_logs_to_sidecar_format(digest_logs);

    let response = RcBlockHeaderResponse {
        parent_hash: format!("{:#x}", header.parent_hash),
        number: header.number.to_string(),
        state_root: format!("{:#x}", header.state_root),
        extrinsics_root: format!("{:#x}", header.extrinsics_root),
        digest: json!({
            "logs": digest_logs_formatted
        }),
    };

    Ok(Json(response).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PARENT_HASH: &str =
        "0xb5531541d3c407569749190350c19784baee799e1e4b9ea52471e75150cd3ec1";
    const TEST_STATE_ROOT: &str =
        "0xa50e5b59f2978c4c6b3c5a54f905711d323741e0b512c8280835c75e8e9afb43";
    const TEST_EXTRINSICS_ROOT: &str =
        "0xefbdd47ab4826ccd29911e04f8b93df56ef43241c3c75f610f0171152a48c6b1";
    const TEST_BLOCK_NUMBER: u64 = 29639318;

    #[test]
    fn test_response_has_correct_structure() {
        let response = RcBlockHeaderResponse {
            parent_hash: TEST_PARENT_HASH.to_string(),
            number: TEST_BLOCK_NUMBER.to_string(),
            state_root: TEST_STATE_ROOT.to_string(),
            extrinsics_root: TEST_EXTRINSICS_ROOT.to_string(),
            digest: json!({
                "logs": [
                    {"preRuntime": ["0x42414245", "0x032d010000"]},
                    {"consensus": ["0x42454546", "0x03c41b16c2"]},
                    {"seal": ["0x42414245", "0x8e3d333ec6"]}
                ]
            }),
        };

        let json = serde_json::to_value(&response).unwrap();

        // Verify all required fields are present
        assert!(json.get("parentHash").is_some());
        assert!(json.get("number").is_some());
        assert!(json.get("stateRoot").is_some());
        assert!(json.get("extrinsicsRoot").is_some());
        assert!(json.get("digest").is_some());

        // Verify field types
        assert!(json["parentHash"].is_string());
        assert!(json["number"].is_string());
        assert!(json["stateRoot"].is_string());
        assert!(json["extrinsicsRoot"].is_string());
        assert!(json["digest"]["logs"].is_array());

        // Verify hash fields start with 0x
        assert!(json["parentHash"].as_str().unwrap().starts_with("0x"));
        assert!(json["stateRoot"].as_str().unwrap().starts_with("0x"));
        assert!(json["extrinsicsRoot"].as_str().unwrap().starts_with("0x"));

        // Verify number is a valid decimal string
        let number_str = json["number"].as_str().unwrap();
        assert!(number_str.parse::<u64>().is_ok());
    }
}
