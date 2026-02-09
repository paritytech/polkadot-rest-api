//! Handler for GET /blocks/{blockId}/extrinsics-raw endpoint.
//!
//! This module provides a handler for fetching raw block data with hex-encoded extrinsics.
//! Unlike the main /blocks/{blockId} endpoint, this returns raw extrinsic bytes without decoding.

use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use super::common::convert_digest_items_to_logs;
use super::types::{DigestLog, GetBlockError};

// ================================================================================================
// Response Types
// ================================================================================================

/// Digest containing log entries for the block
#[derive(Debug, Serialize)]
pub struct BlockRawDigest {
    pub logs: Vec<DigestLog>,
}

/// Raw block response with hex-encoded extrinsics
///
/// This matches the sidecar format for /blocks/{blockId}/extrinsics-raw
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRawResponse {
    /// Parent block hash
    pub parent_hash: String,
    /// Block number in hex format (e.g., "0x00137c23")
    pub number: String,
    /// State root hash
    pub state_root: String,
    /// Merkle root of extrinsics (note: camelCase is extrinsicRoot, not extrinsicsRoot)
    pub extrinsic_root: String,
    /// Block digest containing log entries
    pub digest: BlockRawDigest,
    /// Raw extrinsics as hex-encoded strings
    pub extrinsics: Vec<String>,
}

// ================================================================================================
// Main Handler
// ================================================================================================

#[utoipa::path(
    get,
    path = "/v1/blocks/{blockId}/extrinsics-raw",
    tag = "blocks",
    summary = "Get raw extrinsics",
    description = "Returns raw block data with hex-encoded extrinsics for a given block identifier. The extrinsics are returned as raw hex strings without decoding.",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash")
    ),
    responses(
        (status = 200, description = "Raw block data with hex-encoded extrinsics", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_block_extrinsics_raw(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
) -> Result<Response, GetBlockError> {
    let response = build_block_raw_response(&state, block_id).await?;
    Ok(Json(response).into_response())
}

async fn build_block_raw_response(
    state: &AppState,
    block_id: String,
) -> Result<BlockRawResponse, GetBlockError> {
    let block_id_parsed = block_id.parse::<utils::BlockId>()?;

    let client_at_block = match &block_id_parsed {
        utils::BlockId::Hash(hash) => state.client.at_block(*hash).await,
        utils::BlockId::Number(number) => state.client.at_block(*number).await,
    }
    .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))?;

    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    let header = client_at_block
        .block_header()
        .await
        .map_err(GetBlockError::BlockHeaderFailed)?;

    let parent_hash = format!("{:#x}", header.parent_hash);
    let state_root = format!("{:#x}", header.state_root);
    let extrinsic_root = format!("{:#x}", header.extrinsics_root);

    let logs = convert_digest_items_to_logs(&header.digest.logs);

    let number = format!("0x{:08x}", block_number);

    let extrinsics = fetch_raw_extrinsics(state, &block_hash).await?;

    Ok(BlockRawResponse {
        parent_hash,
        number,
        state_root,
        extrinsic_root,
        digest: BlockRawDigest { logs },
        extrinsics,
    })
}

async fn fetch_raw_extrinsics(
    state: &AppState,
    block_hash: &str,
) -> Result<Vec<String>, GetBlockError> {
    let block_json = state
        .get_block_json(block_hash)
        .await
        .map_err(GetBlockError::BlockFetchFailed)?;

    extract_raw_extrinsics_from_json(&block_json)
}

fn extract_raw_extrinsics_from_json(
    block_json: &serde_json::Value,
) -> Result<Vec<String>, GetBlockError> {
    let extrinsics = block_json
        .get("block")
        .and_then(|b| b.get("extrinsics"))
        .and_then(|e| e.as_array())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("extrinsics".to_string()))?;

    let raw_extrinsics: Vec<String> = extrinsics
        .iter()
        .filter_map(|e| e.as_str().map(|s| s.to_string()))
        .collect();

    Ok(raw_extrinsics)
}
