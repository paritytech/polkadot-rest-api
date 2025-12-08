use crate::handlers::blocks::utils::extract_digest_from_header;
use crate::state::AppState;
use crate::types::BlockHash;
use crate::utils::rc_block::RcBlockHeaderWithParachainsResponse;
use crate::utils::{
    BlockHeaderRcResponse, RcBlockError, compute_block_hash_from_header_json,
    find_ah_blocks_by_rc_block, get_rc_block_header_info, get_timestamp_from_storage,
};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
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

    /// When true, query Asset Hub blocks by Relay Chain block number
    #[serde(default, rename = "useRcBlock")]
    pub use_rc_block: Option<bool>,
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

    #[error("RC block operation failed: {0}")]
    RcBlockFailed(#[from] RcBlockError),
}

impl IntoResponse for GetBlockHeadHeaderError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetBlockHeadHeaderError::HeaderFetchFailed(_)
            | GetBlockHeadHeaderError::HeaderFieldMissing(_)
            | GetBlockHeadHeaderError::HashComputationFailed(_)
            | GetBlockHeadHeaderError::RcBlockFailed(_) => {
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
/// - `useRcBlock` (boolean, optional): When true, queries Asset Hub blocks by Relay Chain block number and returns array.
///
/// Optimizations:
/// - Computes block hash locally from header data (saves 1 RPC call)
/// - Reuses header data instead of fetching twice (saves 1 RPC call)
pub async fn get_blocks_head_header(
    State(state): State<AppState>,
    Query(params): Query<BlockQueryParams>,
) -> Result<Response, GetBlockHeadHeaderError> {
    // Check if useRcBlock is enabled
    if params.use_rc_block == Some(true) && state.has_asset_hub() {
        return handle_rc_block_query(state, params).await;
    }

    // Standard behavior: return single block header
    handle_standard_query(state, params)
        .await
        .map(|json| json.into_response())
}

/// Handle standard query (single block header response)
async fn handle_standard_query(
    state: AppState,
    params: BlockQueryParams,
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

/// Handle useRcBlock query (array of Asset Hub block headers)
async fn handle_rc_block_query(
    state: AppState,
    params: BlockQueryParams,
) -> Result<Response, GetBlockHeadHeaderError> {
    // Get Asset Hub RPC client
    let ah_rpc_client = state.get_asset_hub_rpc_client().await?;

    let rc_client = state.get_relay_chain_subxt_client().await?;
    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;

    let rc_block_number = if params.finalized {
        let finalized_hash = rc_rpc_client
            .request::<Option<String>>("chain_getFinalizedHead", rpc_params![])
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?
            .ok_or_else(|| {
                GetBlockHeadHeaderError::HeaderFieldMissing("Finalized head not found".to_string())
            })?;

        let header_json: serde_json::Value = rc_rpc_client
            .request("chain_getHeader", rpc_params![finalized_hash.clone()])
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

        let number_hex = header_json
            .get("number")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("number".to_string()))?;

        u64::from_str_radix(number_hex.strip_prefix("0x").unwrap_or(number_hex), 16).map_err(
            |_| GetBlockHeadHeaderError::HeaderFieldMissing("number (invalid format)".to_string()),
        )?
    } else {
        let header_json: serde_json::Value = rc_rpc_client
            .request::<serde_json::Value>("chain_getHeader", rpc_params![])
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

        let number_hex = header_json
            .get("number")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("number".to_string()))?;

        u64::from_str_radix(number_hex.strip_prefix("0x").unwrap_or(number_hex), 16).map_err(
            |_| GetBlockHeadHeaderError::HeaderFieldMissing("number (invalid format)".to_string()),
        )?
    };

    // Get RC block header info (hash, parent hash, number)
    let (rc_block_hash, rc_block_parent_hash, rc_block_number_str) =
        get_rc_block_header_info(&rc_rpc_client, rc_block_number).await?;

    // Find Asset Hub blocks corresponding to this RC block number
    // This queries RC block events to find paraInclusion.CandidateIncluded events for Asset Hub
    let ah_blocks = find_ah_blocks_by_rc_block(&rc_client, &rc_rpc_client, rc_block_number).await?;

    // Query each Asset Hub block and build response
    let mut parachains = Vec::new();
    for ah_block in ah_blocks {
        // Get block header from Asset Hub
        let header_json: serde_json::Value = ah_rpc_client
            .request("chain_getHeader", rpc_params![ah_block.hash.clone()])
            .await
            .map_err(|e| GetBlockHeadHeaderError::HeaderFetchFailed(e))?;

        if header_json.is_null() {
            continue;
        }

        // Extract header fields
        let number_hex = header_json
            .get("number")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("number".to_string()))?;

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
            .ok_or_else(|| {
                GetBlockHeadHeaderError::HeaderFieldMissing("extrinsicsRoot".to_string())
            })?
            .to_string();

        let digest = extract_digest_from_header(&header_json);

        let ah_timestamp = get_timestamp_from_storage(&ah_rpc_client, &ah_block.hash)
            .await
            .unwrap_or_else(|| "0".to_string());

        let rc_response = BlockHeaderRcResponse {
            parent_hash,
            number: number_hex.to_string(),
            state_root,
            extrinsics_root,
            digest,
            ah_timestamp,
        };

        parachains.push(rc_response);
    }

    // Always return object with RC block info and parachains array
    let response = RcBlockHeaderWithParachainsResponse {
        rc_block_hash,
        rc_block_parent_hash,
        rc_block_number: rc_block_number_str,
        parachains,
    };

    Ok(Json(response).into_response())
}
