// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for GET /rc/blocks/{blockId}/extrinsics-raw endpoint.
//!
//! This module provides a handler for fetching raw block data with hex-encoded extrinsics
//! from the relay chain. Unlike the main /blocks/{blockId}/extrinsics-raw endpoint,
//! this queries the relay chain connection instead of the primary chain.

use crate::handlers::blocks::common::convert_digest_items_to_logs;
use crate::handlers::blocks::get_block_extrinsics_raw::{BlockRawDigest, BlockRawResponse};
use crate::handlers::blocks::types::GetBlockError;
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use subxt_rpcs::rpc_params;

/// Handler for GET /rc/blocks/{blockId}/extrinsics-raw
///
/// Returns raw block data with hex-encoded extrinsics for a given block identifier (hash or number)
/// from the relay chain. The extrinsics are returned as raw hex strings without decoding.
///
/// # Path Parameters
/// - `blockId`: Block identifier (height number or block hash)
#[utoipa::path(
    get,
    path = "/v1/rc/blocks/{blockId}/extrinsics-raw",
    tag = "rc",
    summary = "RC get raw extrinsics",
    description = "Returns raw hex-encoded extrinsics for a relay chain block without decoding.",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash")
    ),
    responses(
        (status = 200, description = "Raw extrinsics", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
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
    let relay_client = state
        .get_relay_chain_client()
        .ok_or_else(|| GetBlockError::RelayChainNotConfigured)?;

    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or_else(|| GetBlockError::RelayChainNotConfigured)?;

    let client_at_block =
        utils::resolve_client_at_block(relay_client.as_ref(), Some(&block_id)).await?;

    let block_hash = format!("{:#x}", client_at_block.block_hash());

    let header = client_at_block
        .block_header()
        .await
        .map_err(|e| GetBlockError::ExtrinsicsFetchFailed(format!("Header fetch failed: {}", e)))?;

    // Use chain_getBlock RPC directly to get raw extrinsics without decoding
    let raw_extrinsics = fetch_raw_extrinsics_via_rpc(relay_rpc_client, &block_hash).await?;

    let parent_hash = format!("{:#x}", header.parent_hash);
    let number = format!("0x{:08x}", header.number);
    let state_root = format!("{:#x}", header.state_root);
    let extrinsic_root = format!("{:#x}", header.extrinsics_root);

    let logs = convert_digest_items_to_logs(&header.digest.logs);

    Ok(BlockRawResponse {
        parent_hash,
        number,
        state_root,
        extrinsic_root,
        digest: BlockRawDigest { logs },
        extrinsics: raw_extrinsics,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

/// Fetch raw extrinsics from the relay chain using chain_getBlock RPC
async fn fetch_raw_extrinsics_via_rpc(
    rpc_client: &subxt_rpcs::RpcClient,
    block_hash: &str,
) -> Result<Vec<String>, GetBlockError> {
    let block_json: Value = rpc_client
        .request("chain_getBlock", rpc_params![block_hash])
        .await
        .map_err(GetBlockError::BlockFetchFailed)?;

    extract_raw_extrinsics_from_json(&block_json)
}

/// Extract raw extrinsics from the chain_getBlock JSON response
fn extract_raw_extrinsics_from_json(block_json: &Value) -> Result<Vec<String>, GetBlockError> {
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
