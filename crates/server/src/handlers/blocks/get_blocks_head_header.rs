use crate::handlers::blocks::common::{HeaderParseError, parse_header_fields};
use crate::handlers::blocks::types::BlockHeaderResponse;
use crate::state::AppState;
use crate::types::BlockHash;
use crate::utils::{
    self, RcBlockError, compute_block_hash_from_header_json, fetch_block_timestamp,
    find_ah_blocks_in_rc_block,
};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use serde::Deserialize;
use serde_json::json;
use subxt::error::OnlineClientAtBlockError;
use subxt_rpcs::rpc_params;
use thiserror::Error;

/// Query parameters for /blocks/head/header endpoint
#[derive(Debug, Deserialize)]
pub struct BlockQueryParams {
    /// When true (default), query finalized head. When false, query canonical head.
    #[serde(default = "default_finalized")]
    pub finalized: bool,
    /// When true, treat block identifier as Relay Chain block and return Asset Hub blocks included in it
    #[serde(default, rename = "useRcBlock")]
    pub use_rc_block: bool,
}

fn default_finalized() -> bool {
    true
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

    #[error("Failed to find Asset Hub blocks in Relay Chain block")]
    RcBlockError(#[source] Box<RcBlockError>),

    #[error("useRcBlock parameter is only supported for Asset Hub endpoints")]
    UseRcBlockNotSupported,

    #[error(
        "useRcBlock parameter requires relay chain API to be available. Please configure SAS_RELAY_CHAIN_URL"
    )]
    RelayChainNotConfigured,

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[source] Box<OnlineClientAtBlockError>),
}

impl From<HeaderParseError> for GetBlockHeadHeaderError {
    fn from(err: HeaderParseError) -> Self {
        match err {
            HeaderParseError::FieldMissing(field) => {
                GetBlockHeadHeaderError::HeaderFieldMissing(field)
            }
        }
    }
}

impl IntoResponse for GetBlockHeadHeaderError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetBlockHeadHeaderError::UseRcBlockNotSupported
            | GetBlockHeadHeaderError::RelayChainNotConfigured => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetBlockHeadHeaderError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            // Handle RPC errors with appropriate status codes
            GetBlockHeadHeaderError::HeaderFetchFailed(err) => utils::rpc_error_to_status(err),
            GetBlockHeadHeaderError::HeaderFieldMissing(_)
            | GetBlockHeadHeaderError::HashComputationFailed(_)
            | GetBlockHeadHeaderError::RcBlockError(_)
            | GetBlockHeadHeaderError::BlockResolveFailed(_)
            | GetBlockHeadHeaderError::ClientAtBlockFailed(_) => {
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
/// - `useRcBlock` (boolean, default: false): When true, treat as Relay Chain block and return Asset Hub blocks
///
/// Optimizations:
/// - Computes block hash locally from header data (saves 1 RPC call)
/// - Reuses header data instead of fetching twice (saves 1 RPC call)
pub async fn get_blocks_head_header(
    State(state): State<AppState>,
    Query(params): Query<BlockQueryParams>,
) -> Result<Response, GetBlockHeadHeaderError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, params).await;
    }
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

    let parsed = parse_header_fields(&header_json)?;

    // Build lightweight header response
    let response = BlockHeaderResponse {
        parent_hash: parsed.parent_hash,
        number: parsed.number.to_string(),
        state_root: parsed.state_root,
        extrinsics_root: parsed.extrinsics_root,
        digest: json!({
            "logs": parsed.digest_logs
        }),
        hash: Some(block_hash),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    Ok(Json(response).into_response())
}

async fn handle_use_rc_block(
    state: AppState,
    params: BlockQueryParams,
) -> Result<Response, GetBlockHeadHeaderError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(GetBlockHeadHeaderError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(GetBlockHeadHeaderError::RelayChainNotConfigured);
    }

    let rc_resolved_block = if let (Some(rc_rpc), Some(rc_legacy_rpc)) = (
        state.get_relay_chain_rpc_client(),
        state.get_relay_chain_rpc(),
    ) {
        if params.finalized {
            let hash = rc_legacy_rpc
                .chain_get_finalized_head()
                .await
                .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;
            let hash_str = format!("{:#x}", hash);
            let number =
                crate::utils::get_block_number_from_hash_with_rpc(rc_rpc, &hash_str).await?;
            crate::utils::ResolvedBlock {
                hash: hash_str,
                number,
            }
        } else {
            let header_json = rc_rpc
                .request::<serde_json::Value>("chain_getHeader", rpc_params![])
                .await
                .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;
            let number_hex = header_json
                .get("number")
                .and_then(|v| v.as_str())
                .ok_or_else(|| GetBlockHeadHeaderError::HeaderFieldMissing("number".to_string()))?;
            let number =
                u64::from_str_radix(number_hex.strip_prefix("0x").unwrap_or(number_hex), 16)
                    .map_err(|_| {
                        GetBlockHeadHeaderError::HeaderFieldMissing(
                            "number (invalid format)".to_string(),
                        )
                    })?;
            let hash = compute_block_hash_from_header_json(&header_json)?;
            crate::utils::ResolvedBlock {
                hash: hash.to_string(),
                number,
            }
        }
    } else {
        return Err(GetBlockHeadHeaderError::RelayChainNotConfigured);
    };

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block)
        .await
        .map_err(|e| GetBlockHeadHeaderError::RcBlockError(Box::new(e)))?;

    if ah_blocks.is_empty() {
        return Ok(Json(json!([])).into_response());
    }

    let rc_block_number = rc_resolved_block.number.to_string();
    let rc_block_hash = rc_resolved_block.hash.clone();

    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let header_json = state
            .get_header_json(&ah_block.hash)
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?;

        let parsed = parse_header_fields(&header_json)?;

        let client_at_block = state
            .client
            .at_block(ah_block.number)
            .await
            .map_err(|e| GetBlockHeadHeaderError::ClientAtBlockFailed(Box::new(e)))?;
        let ah_timestamp = fetch_block_timestamp(&client_at_block).await;

        results.push(BlockHeaderResponse {
            parent_hash: parsed.parent_hash,
            number: ah_block.number.to_string(),
            state_root: parsed.state_root,
            extrinsics_root: parsed.extrinsics_root,
            digest: json!({
                "logs": parsed.digest_logs
            }),
            hash: Some(ah_block.hash),
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        });
    }

    Ok(Json(json!(results)).into_response())
}
