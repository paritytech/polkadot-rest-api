use crate::state::AppState;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt_rpcs::rpc_params;
use thiserror::Error;

/// Query parameters for /blocks/head/header endpoint
#[derive(Debug, Deserialize)]
pub struct BlockQueryParams {
    /// When true (default), query finalized head. When false, query canonical head.
    #[serde(default = "default_finalized")]
    pub finalized: bool,
}

fn default_finalized() -> bool {
    true
}

/// Lightweight block header information (no author/logs decoding)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockHeaderResponse {
    pub number: String,
    pub hash: String,
    pub parent_hash: String,
    pub state_root: String,
    pub extrinsics_root: String,
}

/// Error types for /blocks/head/header endpoint
#[derive(Debug, Error)]
pub enum GetBlockHeadHeaderError {
    #[error("Failed to get block header")]
    HeaderFetchFailed(#[source] subxt_rpcs::Error),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),
}

impl IntoResponse for GetBlockHeadHeaderError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetBlockHeadHeaderError::HeaderFetchFailed(_)
            | GetBlockHeadHeaderError::HeaderFieldMissing(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Handler for GET /blocks/head/header
///
/// Returns just the header of the latest block (lightweight)
///
/// Query Parameters:
/// - `finalized` (boolean, default: true): When true, returns finalized head. When false, returns canonical head.
pub async fn get_blocks_head_header(
    State(state): State<AppState>,
    Query(params): Query<BlockQueryParams>,
) -> Result<Json<BlockHeaderResponse>, GetBlockHeadHeaderError> {
    // Determine which block to fetch based on finalized parameter
    let (block_hash, block_number) = if params.finalized {
        // Get finalized head
        let finalized_hash = state
            .legacy_rpc
            .chain_get_finalized_head()
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

        // Get the header to extract block number
        let header_json = state
            .get_header_json(&format!("{:?}", finalized_hash))
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

        let block_number_hex = header_json
            .get("number")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("number".to_string()))?;

        let block_number = u64::from_str_radix(
            block_number_hex
                .strip_prefix("0x")
                .unwrap_or(block_number_hex),
            16,
        )
        .map_err(|_| {
            GetBlockHeadHeaderError::HeaderFieldMissing("number (invalid format)".to_string())
        })?;

        (format!("{:?}", finalized_hash), block_number)
    } else {
        // Get canonical head (latest block, may not be finalized)
        let header_json = state
            .rpc_client
            .request::<serde_json::Value>("chain_getHeader", rpc_params![])
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

        // Extract block number
        let block_number_hex = header_json
            .get("number")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("number".to_string()))?;

        let block_number = u64::from_str_radix(
            block_number_hex
                .strip_prefix("0x")
                .unwrap_or(block_number_hex),
            16,
        )
        .map_err(|_| {
            GetBlockHeadHeaderError::HeaderFieldMissing("number (invalid format)".to_string())
        })?;

        // Get block hash by querying chain_getBlockHash with the block number
        let block_hash = state
            .rpc_client
            .request::<String>("chain_getBlockHash", rpc_params![block_number])
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

        (block_hash, block_number)
    };

    // Fetch the header JSON
    let header_json = state
        .get_header_json(&block_hash)
        .await
        .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

    // Extract header fields (no digest/author processing for lightweight response)
    let parent_hash = header_json
        .get("parentHash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("parentHash".to_string()))?
        .to_string();

    let state_root = header_json
        .get("stateRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("stateRoot".to_string()))?
        .to_string();

    let extrinsics_root = header_json
        .get("extrinsicsRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("extrinsicsRoot".to_string()))?
        .to_string();

    // Build lightweight header response
    let response = BlockHeaderResponse {
        number: block_number.to_string(),
        hash: block_hash,
        parent_hash,
        state_root,
        extrinsics_root,
    };

    Ok(Json(response))
}
