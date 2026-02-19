// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for GET /blocks/{blockId}/extrinsics-raw endpoint.
//!
//! This module provides a handler for fetching raw block data with hex-encoded extrinsics.
//! Unlike the main /blocks/{blockId} endpoint, this returns raw extrinsic bytes without decoding.

use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use polkadot_rest_api_config::ChainType;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::common::convert_digest_items_to_logs;
use super::types::{DigestLog, GetBlockError};

// ================================================================================================
// Query Parameters
// ================================================================================================

/// Query parameters for /blocks/{blockId}/extrinsics-raw endpoint
#[derive(Debug, Deserialize)]
pub struct BlockRawExtrinsicsQueryParams {
    /// When true, treat blockId as Relay Chain block and return Asset Hub blocks
    #[serde(default, rename = "useRcBlock")]
    pub use_rc_block: bool,
}

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
    /// Relay Chain block hash (only present when useRcBlock=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    /// Relay Chain block number (only present when useRcBlock=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    /// Asset Hub block timestamp (only present when useRcBlock=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
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
///
/// # Query Parameters
/// - `useRcBlock` (boolean, default: false): When true, treat blockId as Relay Chain block and return Asset Hub blocks
#[utoipa::path(
    get,
    path = "/v1/blocks/{blockId}/extrinsics-raw",
    tag = "blocks",
    summary = "Get raw extrinsics",
    description = "Returns raw block data with hex-encoded extrinsics for a given block identifier. The extrinsics are returned as raw hex strings without decoding.",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash"),
        ("useRcBlock" = Option<bool>, Query, description = "When true, treat blockId as Relay Chain block and return Asset Hub blocks")
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
    Query(params): Query<BlockRawExtrinsicsQueryParams>,
) -> Result<Response, GetBlockError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, block_id).await;
    }

    let response = build_block_raw_response(&state, block_id).await?;
    Ok(Json(response).into_response())
}

async fn build_block_raw_response(
    state: &AppState,
    block_id: String,
) -> Result<BlockRawResponse, GetBlockError> {
    let client_at_block = utils::resolve_client_at_block(&state.client, Some(&block_id)).await?;

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
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

async fn handle_use_rc_block(state: AppState, block_id: String) -> Result<Response, GetBlockError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(GetBlockError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(GetBlockError::RelayChainNotConfigured);
    }

    let rc_block_id = block_id.parse::<utils::BlockId>()?;
    let rc_resolved_block = utils::resolve_block_with_rpc(
        state.get_relay_chain_rpc_client().unwrap(),
        state.get_relay_chain_rpc().unwrap(),
        Some(rc_block_id),
    )
    .await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block)
        .await
        .map_err(|e| GetBlockError::RcBlockError(Box::new(e)))?;

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
            .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))?;

        let mut response = build_block_raw_response(&state, ah_block.hash.clone()).await?;

        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());
        response.ah_timestamp = fetch_block_timestamp(&client_at_block).await;

        results.push(response);
    }

    Ok(Json(json!(results)).into_response())
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
