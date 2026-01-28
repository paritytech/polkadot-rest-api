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

use super::common::decode_digest_logs;
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

/// Handler for GET /blocks/{blockId}/extrinsics-raw
///
/// Returns raw block data with hex-encoded extrinsics for a given block identifier (hash or number).
/// The extrinsics are returned as raw hex strings without decoding.
///
/// # Path Parameters
/// - `blockId`: Block identifier (height number or block hash)
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
    let resolved_block = utils::resolve_block(state, Some(block_id_parsed)).await?;

    let header_json = state
        .get_header_json(&resolved_block.hash)
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;

    let parent_hash = header_json
        .get("parentHash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("parentHash".to_string()))?
        .to_string();

    let state_root = header_json
        .get("stateRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("stateRoot".to_string()))?
        .to_string();

    let extrinsic_root = header_json
        .get("extrinsicsRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("extrinsicsRoot".to_string()))?
        .to_string();

    let logs = decode_digest_logs(&header_json);

    let number = format!("0x{:08x}", resolved_block.number);

    let extrinsics = fetch_raw_extrinsics(state, &resolved_block.hash).await?;

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
