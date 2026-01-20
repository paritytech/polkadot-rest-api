//! Handler for GET /blocks/{blockId}/header endpoint.
//!
//! This module provides the handler for fetching block header information
//! for a specific block identified by hash or number.

use crate::handlers::blocks::common::decode_digest_logs;
use crate::handlers::blocks::types::{
    BlockHeaderQueryParams, BlockHeaderResponse, GetBlockHeaderError,
    convert_digest_logs_to_sidecar_format,
};
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use parity_scale_codec::Decode;
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

    let header_json = state
        .get_header_json(&resolved_block.hash)
        .await
        .map_err(GetBlockHeaderError::HeaderFetchFailed)?;

    let parent_hash = header_json
        .get("parentHash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockHeaderError::HeaderFieldMissing("parentHash".to_string()))?
        .to_string();

    let state_root = header_json
        .get("stateRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockHeaderError::HeaderFieldMissing("stateRoot".to_string()))?
        .to_string();

    let extrinsics_root = header_json
        .get("extrinsicsRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockHeaderError::HeaderFieldMissing("extrinsicsRoot".to_string()))?
        .to_string();

    let digest_logs = decode_digest_logs(&header_json);
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

    let rc_block_id = block_id.parse::<utils::BlockId>()?;
    let rc_resolved_block = utils::resolve_block_with_rpc(
        state.get_relay_chain_rpc_client().unwrap(),
        state.get_relay_chain_rpc().unwrap(),
        Some(rc_block_id),
    )
    .await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

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
            .map_err(GetBlockHeaderError::HeaderFetchFailed)?;

        let parent_hash = header_json
            .get("parentHash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockHeaderError::HeaderFieldMissing("parentHash".to_string()))?
            .to_string();

        let state_root = header_json
            .get("stateRoot")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockHeaderError::HeaderFieldMissing("stateRoot".to_string()))?
            .to_string();

        let extrinsics_root = header_json
            .get("extrinsicsRoot")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockHeaderError::HeaderFieldMissing("extrinsicsRoot".to_string()))?
            .to_string();

        let digest_logs = decode_digest_logs(&header_json);
        let digest_logs_formatted = convert_digest_logs_to_sidecar_format(digest_logs);

        let mut ah_timestamp = None;
        let client_at_block = state.client.at(ah_block.number).await?;
        if let Ok(timestamp_entry) = client_at_block.storage().entry("Timestamp", "Now")
            && let Ok(Some(timestamp)) = timestamp_entry.fetch(()).await
        {
            let timestamp_bytes = timestamp.into_bytes();
            let mut cursor = &timestamp_bytes[..];
            if let Ok(timestamp_value) = u64::decode(&mut cursor) {
                ah_timestamp = Some(timestamp_value.to_string());
            }
        }

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
