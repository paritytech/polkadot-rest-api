use crate::state::AppState;
use crate::types::BlockHash;
use crate::utils::compute_block_hash_from_header_json;
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

    #[error("Failed to compute block hash: {0}")]
    HashComputationFailed(#[from] crate::utils::HashError),

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),
}

impl IntoResponse for GetBlockHeadHeaderError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetBlockHeadHeaderError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetBlockHeadHeaderError::HeaderFetchFailed(_)
            | GetBlockHeadHeaderError::HeaderFieldMissing(_)
            | GetBlockHeadHeaderError::HashComputationFailed(_) => {
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
///
/// Optimizations:
/// - Computes block hash locally from header data (saves 1 RPC call)
/// - Reuses header data instead of fetching twice (saves 1 RPC call)
pub async fn get_blocks_head_header(
    State(state): State<AppState>,
    Query(params): Query<BlockQueryParams>,
) -> Result<Json<BlockHeaderResponse>, GetBlockHeadHeaderError> {
    let (block_hash, header_json) = if params.finalized {
        let finalized_hash = state
            .legacy_rpc
            .chain_get_finalized_head()
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;
        let block_hash_typed = BlockHash::from(finalized_hash);
        let hash_str = block_hash_typed.to_string();
        let header_json = state
            .get_header_json(&hash_str)
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

        (hash_str, header_json)
    } else {
        // Canonical head (may not be finalized): get latest header
        // OPTIMIZATION: This returns the header without hash, so we compute it locally
        let header_json = state
            .rpc_client
            .request::<serde_json::Value>("chain_getHeader", rpc_params![])
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

        // OPTIMIZATION: Compute hash locally from header data
        // This saves 1 RPC call (chain_getBlockHash)
        let block_hash_typed = compute_block_hash_from_header_json(&header_json)?;
        let block_hash = block_hash_typed.to_string();

        (block_hash, header_json)
    };

    // Extract header fields from the JSON we already have
    // OPTIMIZATION: We don't fetch the header again - we already have it!
    let number_hex = header_json
        .get("number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("number".to_string()))?;

    let block_number = u64::from_str_radix(number_hex.strip_prefix("0x").unwrap_or(number_hex), 16)
        .map_err(|_| {
            GetBlockHeadHeaderError::HeaderFieldMissing("number (invalid format)".to_string())
        })?;

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
