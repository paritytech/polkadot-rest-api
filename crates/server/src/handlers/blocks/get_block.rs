// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for GET /blocks/{blockId} endpoint.
//!
//! This module provides the main handler for fetching block information.

use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use serde_json::json;

use super::common::{BlockBuildContext, build_block_response_generic};
use super::types::{BlockQueryParams, BlockResponse, GetBlockError};

// ================================================================================================
// Main Handler
// ================================================================================================

#[utoipa::path(
    get,
    path = "/v1/blocks/{blockId}",
    tag = "blocks",
    summary = "Get block by ID",
    description = "Returns block information for a given block identifier (hash or number), including extrinsics, events, and fees.",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash"),
        ("eventDocs" = Option<bool>, Query, description = "Include documentation for events"),
        ("extrinsicDocs" = Option<bool>, Query, description = "Include documentation for extrinsics"),
        ("noFees" = Option<bool>, Query, description = "Skip fee calculation for extrinsics"),
        ("decodedXcmMsgs" = Option<bool>, Query, description = "Decode and include XCM messages"),
        ("paraId" = Option<u32>, Query, description = "Filter XCM messages by parachain ID"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat blockId as Relay Chain block and return Asset Hub blocks")
    ),
    responses(
        (status = 200, description = "Block information", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 404, description = "Block not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_block(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<BlockQueryParams>,
) -> Result<Response, GetBlockError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, block_id, params).await;
    }

    let response = build_block_response(&state, block_id, &params).await?;
    Ok(Json(response).into_response())
}

async fn handle_use_rc_block(
    state: AppState,
    block_id: String,
    params: BlockQueryParams,
) -> Result<Response, GetBlockError> {
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
        // Create client_at_block for this Asset Hub block
        let client_at_block = state
            .client
            .at_block(ah_block.number)
            .await
            .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))?;

        let mut response = build_block_response_for_hash(
            &state,
            &ah_block.hash,
            ah_block.number,
            true,
            &client_at_block,
            &params,
        )
        .await?;

        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        response.ah_timestamp = fetch_block_timestamp(&client_at_block).await;

        results.push(response);
    }

    Ok(Json(json!(results)).into_response())
}

pub(crate) async fn build_block_response(
    state: &AppState,
    block_id: String,
    params: &BlockQueryParams,
) -> Result<BlockResponse, GetBlockError> {
    let block_id_parsed = block_id.parse::<utils::BlockId>()?;
    let queried_by_hash = matches!(block_id_parsed, utils::BlockId::Hash(_));

    // Create client_at_block directly from parsed input - saves 1 RPC call
    // by letting subxt resolve hash<->number internally
    let client_at_block = match &block_id_parsed {
        utils::BlockId::Hash(hash) => state.client.at_block(*hash).await,
        utils::BlockId::Number(number) => state.client.at_block(*number).await,
    }
    .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))?;

    // Extract hash and number from the resolved client
    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    build_block_response_for_hash(
        state,
        &block_hash,
        block_number,
        queried_by_hash,
        &client_at_block,
        params,
    )
    .await
}

pub(crate) async fn build_block_response_for_hash(
    state: &AppState,
    block_hash: &str,
    block_number: u64,
    queried_by_hash: bool,
    client_at_block: &super::common::BlockClient,
    params: &BlockQueryParams,
) -> Result<BlockResponse, GetBlockError> {
    let ctx = BlockBuildContext {
        state,
        client: &state.client,
        ss58_prefix: state.chain_info.ss58_prefix,
        chain_type: state.chain_info.chain_type.clone(),
        spec_name: state.chain_info.spec_name.clone(),
    };

    build_block_response_generic(
        &ctx,
        client_at_block,
        block_hash,
        block_number,
        queried_by_hash,
        &params.to_build_params(),
        params.finalized_key,
    )
    .await
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use config::SidecarConfig;
    use serde_json::json;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    /// Helper to create a test AppState with mocked RPC responses
    async fn create_test_state_with_mock(mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let rpc_client = Arc::new(RpcClient::new(mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: config::ChainType::Relay,
            spec_name: "test".to_string(),
            spec_version: 1,
            ss58_prefix: 42,
        };

        let client = subxt::OnlineClient::from_rpc_client((*rpc_client).clone())
            .await
            .expect("Failed to create test OnlineClient");

        AppState {
            config,
            client: Arc::new(client),
            legacy_rpc,
            rpc_client,
            chain_info,
            relay_client: None,
            relay_rpc_client: None,
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: crate::routes::RouteRegistry::new(),
            relay_chain_rpc: None,
            lazy_relay_rpc: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    #[tokio::test]
    #[ignore] // Requires proper subxt metadata mocking for event fetching
    async fn test_get_block_by_number() {
        // Note: We don't mock state_getStorage here, so author_id will be None
        // Full author extraction is tested against live chain
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64",
                    "parentHash": "0xabcdef0000000000000000000000000000000000000000000000000000000000",
                    "stateRoot": "0xdef0000000000000000000000000000000000000000000000000000000000000",
                    "extrinsicsRoot": "0x1230000000000000000000000000000000000000000000000000000000000000",
                    "digest": {
                        "logs": [
                            // PreRuntime log: discriminant (6) + engine_id ("BABE") + variant (01) + authority_index (03000000 = 3 in LE)
                            "0x06424142450103000000"
                        ]
                    }
                }))
            })
            // Mock archive_v1_body to return empty extrinsics array
            .method_handler("archive_v1_body", async |_params| {
                MockJson(json!([]))
            })
            // Mock state_getRuntimeVersion for subxt metadata fetch
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(json!({
                    "specVersion": 1,
                    "transactionVersion": 1
                }))
            })
            // Mock state_getMetadata for subxt
            .method_handler("state_getMetadata", async |_params| {
                // Return minimal valid metadata (this is a complex SCALE-encoded structure)
                // For testing, we'll return a minimal valid metadata hex
                MockJson("0x6d657461")
            })
            // Mock state_getStorage for System.Events (returns empty events)
            .method_handler("state_getStorage", async |_params| {
                // Return SCALE-encoded empty Vec<EventRecord>
                MockJson("0x00")
            })
            .build();

        let state = create_test_state_with_mock(mock_client).await;
        let block_id = "100".to_string();
        let params = BlockQueryParams::default();

        // Attempt to get the block - this will fail at metadata fetch in current setup
        // but validates the handler flow up to that point
        let result = get_block(State(state), Path(block_id), Query(params)).await;

        // We expect an error due to metadata fetching limitations in mock environment
        assert!(result.is_err());
    }
}
