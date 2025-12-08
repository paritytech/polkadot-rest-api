use crate::state::AppState;
use crate::utils::{
    self, RcBlockError,
    find_ah_blocks_by_rc_block, get_timestamp_from_storage,
    BlockRcResponse,
};
use crate::handlers::blocks::utils::{
    DigestLog, ExtrinsicInfo,
    extract_header_fields, extract_author, extract_extrinsics, is_block_finalized,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use subxt_historic::error::{OnlineClientAtBlockError, StorageEntryIsNotAPlainValue, StorageError};
use subxt_rpcs::rpc_params;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetBlockError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Failed to get block header")]
    HeaderFetchFailed(#[source] subxt_rpcs::Error),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[from] OnlineClientAtBlockError),

    #[error("Failed to fetch chain storage")]
    StorageFetchFailed(#[from] StorageError),

    #[error("Storage entry is not a plain value")]
    StorageNotPlainValue(#[from] StorageEntryIsNotAPlainValue),

    #[error("Failed to decode storage value")]
    StorageDecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Failed to fetch extrinsics")]
    ExtrinsicsFetchFailed(String),

    #[error("Missing signature bytes for signed extrinsic")]
    MissingSignatureBytes,

    #[error("Missing address bytes for signed extrinsic")]
    MissingAddressBytes,

    #[error("Failed to decode extrinsic field: {0}")]
    ExtrinsicDecodeFailed(String),

    #[error("RC block operation failed: {0}")]
    RcBlockFailed(#[from] RcBlockError),
}

impl IntoResponse for GetBlockError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetBlockError::InvalidBlockParam(_) | GetBlockError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetBlockError::HeaderFetchFailed(_)
            | GetBlockError::HeaderFieldMissing(_)
            | GetBlockError::ClientAtBlockFailed(_)
            | GetBlockError::StorageFetchFailed(_)
            | GetBlockError::StorageNotPlainValue(_)
            | GetBlockError::StorageDecodeFailed(_)
            | GetBlockError::ExtrinsicsFetchFailed(_)
            | GetBlockError::MissingSignatureBytes
            | GetBlockError::MissingAddressBytes
            | GetBlockError::ExtrinsicDecodeFailed(_)
            | GetBlockError::RcBlockFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockResponse {
    pub number: String,
    pub hash: String,
    pub parent_hash: String,
    pub state_root: String,
    pub extrinsics_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    pub logs: Vec<DigestLog>,
    pub extrinsics: Vec<ExtrinsicInfo>,
}


/// Query parameters for /blocks/{blockId} endpoint
#[derive(Debug, Deserialize)]
pub struct GetBlockQueryParams {
    /// When true, query Asset Hub blocks by Relay Chain block number
    #[serde(default, rename = "useRcBlock")]
    pub use_rc_block: Option<bool>,
}

/// Handler for GET /blocks/{blockId}
pub async fn get_block(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<GetBlockQueryParams>,
) -> Result<Response, GetBlockError> {
    // Check if useRcBlock is enabled
    if params.use_rc_block == Some(true) && state.has_asset_hub() {
        return handle_rc_block_query(state, block_id).await;
    }

    handle_standard_block_query(state, block_id).await
}



async fn handle_standard_block_query(
    state: AppState,
    block_id: String,
) -> Result<Response, GetBlockError> {
    // Parse the block identifier
    let block_id = block_id.parse::<utils::BlockId>()?;

    // Resolve the block
    let resolved_block = utils::resolve_block(&state, Some(block_id)).await?;

    // Fetch the header JSON
    let header_json = state
        .get_header_json(&resolved_block.hash)
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;

    // Extract header fields
    let (parent_hash, state_root, extrinsics_root, logs) = extract_header_fields(&header_json)?;

    // Extract author from digest logs by mapping authority index to validator
    let author_id = extract_author(&state, resolved_block.number, &logs).await;

    // Extract extrinsics using subxt-historic for historical integrity
    let extrinsics = extract_extrinsics(&state, resolved_block.number).await?;

    // Build response
    let response = BlockResponse {
        number: resolved_block.number.to_string(),
        hash: resolved_block.hash,
        parent_hash,
        state_root,
        extrinsics_root,
        author_id,
        logs,
        extrinsics,
    };

    Ok(Json(response).into_response())
}

/// Handle useRcBlock query (array of Asset Hub blocks)
async fn handle_rc_block_query(
    state: AppState,
    block_id: String,
) -> Result<Response, GetBlockError> {
    // Parse blockId as RC block number
    let rc_block_number = block_id.parse::<u64>().map_err(|e| {
        GetBlockError::InvalidBlockParam(utils::BlockIdParseError::InvalidNumber(e))
    })?;

    // Get Asset Hub RPC client
    let ah_rpc_client = state.get_asset_hub_rpc_client().await?;

    let rc_client = state.get_relay_chain_subxt_client().await?;
    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;

    // Find Asset Hub blocks corresponding to this RC block number
    // This queries RC block events to find paraInclusion.CandidateIncluded events for Asset Hub
    let ah_blocks = find_ah_blocks_by_rc_block(&rc_client, &rc_rpc_client, rc_block_number).await?;

    // If no blocks found, return empty array
    if ah_blocks.is_empty() {
        return Ok(Json::<Vec<BlockRcResponse>>(vec![]).into_response());
    }

    // Query each Asset Hub block and build response
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        // Get block header from Asset Hub
        let header_json: serde_json::Value = ah_rpc_client
            .request("chain_getHeader", rpc_params![ah_block.hash.clone()])
            .await
            .map_err(|e| GetBlockError::HeaderFetchFailed(e))?;

        if header_json.is_null() {
            continue;
        }

        // Extract header fields
        let (parent_hash, state_root, extrinsics_root, logs) = match extract_header_fields(&header_json) {
            Ok(fields) => fields,
            Err(e) => {
                tracing::warn!("Failed to extract header fields: {:?}", e);
                continue;
            }
        };

        let number_hex = header_json
            .get("number")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockError::HeaderFieldMissing("number".to_string()))?;
        
        let block_number = u64::from_str_radix(number_hex.trim_start_matches("0x"), 16)
            .map_err(|e| GetBlockError::HeaderFieldMissing(format!("Failed to parse block number: {}", e)))?;
        
        let number = block_number.to_string();

        let author_id = extract_author(&state, block_number, &logs).await;

        let extrinsics = match extract_extrinsics(&state, block_number).await {
            Ok(exts) => exts,
            Err(e) => {
                tracing::warn!("Failed to extract extrinsics for block {}: {:?}", block_number, e);
                Vec::new()
            }
        };

        let ah_timestamp = get_timestamp_from_storage(&ah_rpc_client, &ah_block.hash)
            .await
            .unwrap_or_else(|| "0".to_string());

        let rc_response = BlockRcResponse {
            number,
            hash: ah_block.hash.clone(),
            parent_hash,
            state_root,
            extrinsics_root,
            author_id,
            logs,
            on_initialize: utils::rc_block::OnInitializeFinalize {
                events: Vec::new(),
            },
            extrinsics,
            on_finalize: utils::rc_block::OnInitializeFinalize {
                events: Vec::new(),
            },
            finalized: is_block_finalized(&ah_rpc_client, &ah_block.hash).await.unwrap_or(false),
            rc_block_hash: ah_block.rc_block_hash.clone(),
            rc_block_number: rc_block_number.to_string(),
            ah_timestamp,
        };

        results.push(rc_response);
    }

    Ok(Json(results).into_response())
}


