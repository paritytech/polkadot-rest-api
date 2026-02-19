// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for GET /blocks/{blockId}/header endpoint.
//!
//! This module provides the handler for fetching block header information
//! for a specific block identified by hash or number.

use crate::extractors::JsonQuery;
use crate::handlers::blocks::common::convert_digest_items_to_logs;
use crate::handlers::blocks::types::{
    BlockHeaderQueryParams, BlockHeaderResponse, GetBlockHeaderError,
    convert_digest_logs_to_sidecar_format,
};
use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block_at};
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use polkadot_rest_api_config::ChainType;
use serde_json::json;

#[utoipa::path(
    get,
    path = "/v1/blocks/{blockId}/header",
    tag = "blocks",
    summary = "Get block header by ID",
    description = "Returns the header of the specified block (lightweight, no extrinsics/events).",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash"),
        ("useRcBlock" = Option<bool>, description = "Treat blockId as Relay Chain block and return Asset Hub blocks")
    ),
    responses(
        (status = 200, description = "Block header information", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_block_header(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    JsonQuery(params): JsonQuery<BlockHeaderQueryParams>,
) -> Result<Response, GetBlockHeaderError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, block_id, params).await;
    }

    let block_id_parsed = block_id.parse::<utils::BlockId>()?;

    let client_at_block = match &block_id_parsed {
        utils::BlockId::Number(n) => state.client.at_block(*n).await,
        utils::BlockId::Hash(h) => state.client.at_block(*h).await,
    }
    .map_err(GetBlockHeaderError::ClientAtBlockFailed)?;

    let header = client_at_block
        .block_header()
        .await
        .map_err(GetBlockHeaderError::BlockHeaderFailed)?;

    let digest_logs = convert_digest_items_to_logs(&header.digest.logs);
    let digest_logs_formatted = convert_digest_logs_to_sidecar_format(digest_logs);

    let response = BlockHeaderResponse {
        parent_hash: format!("{:#x}", header.parent_hash),
        number: client_at_block.block_number().to_string(),
        state_root: format!("{:#x}", header.state_root),
        extrinsics_root: format!("{:#x}", header.extrinsics_root),
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

    let relay_client = state
        .get_relay_chain_client()
        .ok_or(GetBlockHeaderError::RelayChainNotConfigured)?;

    let rc_block_id = block_id.parse::<utils::BlockId>()?;
    let rc_client_at_block = match &rc_block_id {
        utils::BlockId::Number(n) => relay_client.at_block(*n).await,
        utils::BlockId::Hash(h) => relay_client.at_block(*h).await,
    }
    .map_err(GetBlockHeaderError::ClientAtBlockFailed)?;

    let ah_blocks = find_ah_blocks_in_rc_block_at(&rc_client_at_block).await?;

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
