// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::extractors::JsonQuery;
use crate::handlers::blocks::common::convert_digest_items_to_logs;
use crate::handlers::blocks::types::{BlockHeaderResponse, convert_digest_logs_to_sidecar_format};
use crate::state::{AppState, RelayChainError};
use crate::utils::{self, RcBlockError, fetch_block_timestamp, find_ah_blocks_in_rc_block_at};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use polkadot_rest_api_config::ChainType;
use serde::Deserialize;
use serde_json::json;
use subxt::error::OnlineClientAtBlockError;
use thiserror::Error;

/// Query parameters for /blocks/head/header endpoint
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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

    #[error("Failed to get block header")]
    BlockHeaderFailed(#[source] subxt::error::BlockError),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Failed to find Asset Hub blocks in Relay Chain block")]
    RcBlockError(#[source] Box<RcBlockError>),

    #[error("useRcBlock parameter is only supported for Asset Hub endpoints")]
    UseRcBlockNotSupported,

    #[error(transparent)]
    RelayChain(#[from] RelayChainError),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[source] Box<OnlineClientAtBlockError>),
}

impl IntoResponse for GetBlockHeadHeaderError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetBlockHeadHeaderError::UseRcBlockNotSupported => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetBlockHeadHeaderError::RelayChain(RelayChainError::NotConfigured) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetBlockHeadHeaderError::RelayChain(RelayChainError::ConnectionFailed(_)) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetBlockHeadHeaderError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetBlockHeadHeaderError::HeaderFetchFailed(err) => utils::rpc_error_to_status(err),
            GetBlockHeadHeaderError::HeaderFieldMissing(_)
            | GetBlockHeadHeaderError::RcBlockError(_)
            | GetBlockHeadHeaderError::ClientAtBlockFailed(_)
            | GetBlockHeadHeaderError::BlockHeaderFailed(_) => {
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
#[utoipa::path(
    get,
    path = "/v1/blocks/head/header",
    tag = "blocks",
    summary = "Get head block header",
    description = "Returns the header of the latest finalized or canonical block (lightweight, no extrinsics/events).",
    params(
        ("finalized" = Option<bool>, description = "When true (default), returns finalized head header. When false, returns canonical head header."),
        ("useRcBlock" = Option<bool>, description = "Treat as Relay Chain block and return Asset Hub blocks")
    ),
    responses(
        (status = 200, description = "Block header information", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_blocks_head_header(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<BlockQueryParams>,
) -> Result<Response, GetBlockHeadHeaderError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, params).await;
    }

    let client_at_block = if params.finalized {
        state
            .client
            .at_current_block()
            .await
            .map_err(|e| GetBlockHeadHeaderError::ClientAtBlockFailed(Box::new(e)))?
    } else {
        let best_hash = state
            .legacy_rpc
            .chain_get_block_hash(None)
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?
            .ok_or_else(|| {
                GetBlockHeadHeaderError::HeaderFieldMissing("best block hash".to_string())
            })?;

        state
            .client
            .at_block(best_hash)
            .await
            .map_err(|e| GetBlockHeadHeaderError::ClientAtBlockFailed(Box::new(e)))?
    };

    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    let header = client_at_block
        .block_header()
        .await
        .map_err(GetBlockHeadHeaderError::BlockHeaderFailed)?;

    let parent_hash = format!("{:#x}", header.parent_hash);
    let state_root = format!("{:#x}", header.state_root);
    let extrinsics_root = format!("{:#x}", header.extrinsics_root);

    let digest_logs = convert_digest_items_to_logs(&header.digest.logs);
    let digest_logs_formatted = convert_digest_logs_to_sidecar_format(digest_logs);

    let response = BlockHeaderResponse {
        parent_hash,
        number: block_number.to_string(),
        state_root,
        extrinsics_root,
        digest: json!({
            "logs": digest_logs_formatted
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

    let relay_client = state.get_relay_chain_client().await?;

    let relay_rpc = state.get_relay_chain_rpc().await?;

    let rc_client_at_block = if params.finalized {
        relay_client
            .at_current_block()
            .await
            .map_err(|e| GetBlockHeadHeaderError::ClientAtBlockFailed(Box::new(e)))?
    } else {
        let best_hash = relay_rpc
            .chain_get_block_hash(None)
            .await
            .map_err(GetBlockHeadHeaderError::HeaderFetchFailed)?
            .ok_or_else(|| {
                GetBlockHeadHeaderError::HeaderFieldMissing("best block hash".to_string())
            })?;

        relay_client
            .at_block(best_hash)
            .await
            .map_err(|e| GetBlockHeadHeaderError::ClientAtBlockFailed(Box::new(e)))?
    };

    let ah_blocks = find_ah_blocks_in_rc_block_at(&rc_client_at_block)
        .await
        .map_err(|e| GetBlockHeadHeaderError::RcBlockError(Box::new(e)))?;

    if ah_blocks.is_empty() {
        return Ok(Json(json!([])).into_response());
    }

    let rc_block_number = rc_client_at_block.block_number().to_string();
    let rc_block_hash = format!("{:#x}", rc_client_at_block.block_hash());

    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let client_at_block = state
            .client
            .at_block(ah_block.number)
            .await
            .map_err(|e| GetBlockHeadHeaderError::ClientAtBlockFailed(Box::new(e)))?;

        let header = client_at_block
            .block_header()
            .await
            .map_err(GetBlockHeadHeaderError::BlockHeaderFailed)?;

        let parent_hash = format!("{:#x}", header.parent_hash);
        let state_root = format!("{:#x}", header.state_root);
        let extrinsics_root = format!("{:#x}", header.extrinsics_root);

        let digest_logs = convert_digest_items_to_logs(&header.digest.logs);
        let digest_logs_formatted = convert_digest_logs_to_sidecar_format(digest_logs);

        let ah_timestamp = fetch_block_timestamp(&client_at_block).await;

        results.push(BlockHeaderResponse {
            parent_hash,
            number: ah_block.number.to_string(),
            state_root,
            extrinsics_root,
            digest: json!({
                "logs": digest_logs_formatted
            }),
            hash: Some(ah_block.hash),
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        });
    }

    Ok(Json(json!(results)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_query_params_rejects_unknown_fields() {
        let json = r#"{"finalized": true, "unknownField": true}"#;
        let result: Result<BlockQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_block_query_params_accepts_known_fields() {
        let json = r#"{"finalized": false, "useRcBlock": true}"#;
        let result: Result<BlockQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_ok());
        let params = result.unwrap();
        assert!(!params.finalized);
        assert!(params.use_rc_block);
    }

    #[test]
    fn test_block_query_params_accepts_empty_object() {
        let json = r#"{}"#;
        let result: Result<BlockQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_ok());
        let params = result.unwrap();
        assert!(params.finalized); // default is true
        assert!(!params.use_rc_block); // default is false
    }
}
