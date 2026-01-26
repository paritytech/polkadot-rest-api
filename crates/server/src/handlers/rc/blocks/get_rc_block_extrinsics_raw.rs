//! Handler for GET /rc/blocks/{blockId}/extrinsics-raw endpoint.
//!
//! This module provides a handler for fetching raw block data with hex-encoded extrinsics
//! from the relay chain. Unlike the main /blocks/{blockId}/extrinsics-raw endpoint,
//! this queries the relay chain connection instead of the primary chain.

use crate::handlers::blocks::get_block_extrinsics_raw::{
    BlockRawResponse, build_block_raw_response_from_json,
};
use crate::handlers::blocks::types::GetBlockError;
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};

/// Handler for GET /rc/blocks/{blockId}/extrinsics-raw
///
/// Returns raw block data with hex-encoded extrinsics for a given block identifier (hash or number)
/// from the relay chain. The extrinsics are returned as raw hex strings without decoding.
///
/// # Path Parameters
/// - `blockId`: Block identifier (height number or block hash)
pub async fn get_rc_block_extrinsics_raw(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
) -> Result<Response, GetBlockError> {
    let response: BlockRawResponse = build_rc_block_raw_response(&state, block_id).await?;
    Ok(Json(response).into_response())
}

async fn build_rc_block_raw_response(
    state: &AppState,
    block_id: String,
) -> Result<BlockRawResponse, GetBlockError> {
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or_else(|| GetBlockError::RelayChainNotConfigured)?;

    let block_id_parsed = block_id.parse::<utils::BlockId>()?;
    let resolved_block =
        utils::resolve_block_with_rpc_client(relay_rpc_client, Some(block_id_parsed)).await?;

    let block_json = state.get_relay_block_json(&resolved_block.hash).await?;

    let header_json = block_json
        .get("block")
        .and_then(|b| b.get("header"))
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("block.header".to_string()))?;

    build_block_raw_response_from_json(header_json, resolved_block.number, &block_json)
}
