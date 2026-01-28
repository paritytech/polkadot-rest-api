//! Handler for GET /blocks/{blockId}/header endpoint.
//!
//! This module provides the handler for fetching block header information
//! for a specific block identified by hash or number.

use crate::handlers::blocks::common::convert_digest_items_to_logs;
use crate::handlers::blocks::types::{
    BlockHeaderQueryParams, BlockHeaderResponse, GetBlockHeaderError,
    convert_digest_logs_to_sidecar_format,
};
use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use serde_json::json;

/// Handler for GET /blocks/{blockId}/header
///
/// Returns the header of the specified block (lightweight)
///
/// Path Parameters:
/// - `blockId`: Block height or block hash
///
/// Query Parameters:
/// - `useRcBlock` (boolean, default: false): When true, treat blockId as Relay Chain block
///   and return Asset Hub blocks included in it
pub async fn get_block_header(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<BlockHeaderQueryParams>,
) -> Result<Response, GetBlockHeaderError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, block_id, params).await;
    }

    let block_id_parsed = block_id.parse::<utils::BlockId>()?;
    let resolved_block = utils::resolve_block(&state, Some(block_id_parsed)).await?;

    let client_at_block = state
        .client
        .at_block(resolved_block.number)
        .await
        .map_err(GetBlockHeaderError::ClientAtBlockFailed)?;

    let header = client_at_block
        .block_header()
        .await
        .map_err(GetBlockHeaderError::BlockHeaderFailed)?;

    let parent_hash = format!("{:#x}", header.parent_hash);
    let state_root = format!("{:#x}", header.state_root);
    let extrinsics_root = format!("{:#x}", header.extrinsics_root);

    let digest_logs = convert_digest_items_to_logs(&header.digest.logs);
    let digest_logs_formatted = convert_digest_logs_to_sidecar_format(digest_logs);

    let response = BlockHeaderResponse {
        parent_hash,
        number: resolved_block.number.to_string(),
        state_root,
        extrinsics_root,
        digest: json!({
            "logs": digest_logs_formatted
        }),
        hash: None,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    Ok(Json(response).into_response())
}

async fn handle_use_rc_block(
    state: AppState,
    block_id: String,
    _params: BlockHeaderQueryParams,
) -> Result<Response, GetBlockHeaderError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(GetBlockHeaderError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(GetBlockHeaderError::RelayChainNotConfigured);
    }

    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(GetBlockHeaderError::RelayChainNotConfigured)?;

    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(GetBlockHeaderError::RelayChainNotConfigured)?;

    let rc_block_id = block_id.parse::<utils::BlockId>()?;
    let rc_resolved_block =
        utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, Some(rc_block_id)).await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    if ah_blocks.is_empty() {
        return Ok(Json(json!([])).into_response());
    }

    let rc_block_number = rc_resolved_block.number.to_string();
    let rc_block_hash = rc_resolved_block.hash.clone();

    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let client_at_block = state
            .client
            .at_block(ah_block.number)
            .await
            .map_err(GetBlockHeaderError::ClientAtBlockFailed)?;

        let header = client_at_block
            .block_header()
            .await
            .map_err(GetBlockHeaderError::BlockHeaderFailed)?;

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
            hash: None,
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        });
    }

    Ok(Json(json!(results)).into_response())
}
