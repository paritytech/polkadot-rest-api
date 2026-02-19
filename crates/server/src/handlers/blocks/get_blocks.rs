// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::AppState;
use crate::utils::{self, ResolvedBlock, fetch_block_timestamp, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use futures::stream::{self, StreamExt, TryStreamExt};
use serde::Deserialize;
use std::sync::Arc;

use super::common::parse_range;
use super::get_block::build_block_response_for_hash;
use super::types::{BlockQueryParams, BlockResponse, GetBlockError};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlocksRangeQueryParams {
    pub range: Option<String>,
    #[serde(default)]
    pub event_docs: bool,
    #[serde(default)]
    pub extrinsic_docs: bool,
    #[serde(default)]
    pub no_fees: bool,
    #[serde(default)]
    pub use_rc_block: bool,
    /// When true, convert AccountId32 addresses to EVM format for revive pallet events
    #[serde(default)]
    pub use_evm_format: bool,
}

#[utoipa::path(
    get,
    path = "/v1/blocks",
    tag = "blocks",
    summary = "Get blocks by range",
    description = "Returns a collection of blocks given a numeric range. Range is inclusive and limited to 500 blocks.",
    params(
        ("range" = Option<String>, Query, description = "Block range in format 'start-end' (e.g. '100-200')"),
        ("eventDocs" = Option<bool>, Query, description = "Include documentation for events"),
        ("extrinsicDocs" = Option<bool>, Query, description = "Include documentation for extrinsics"),
        ("noFees" = Option<bool>, Query, description = "Skip fee calculation for extrinsics"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat range as Relay Chain blocks")
    ),
    responses(
        (status = 200, description = "Array of block information", body = Vec<Object>),
        (status = 400, description = "Invalid range parameter"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_blocks(
    State(state): State<AppState>,
    Query(params): Query<BlocksRangeQueryParams>,
) -> Result<Response, GetBlockError> {
    let range_str = params.range.clone().ok_or(GetBlockError::MissingRange)?;

    let (start, end) = parse_range(&range_str)?;

    let base_block_params = BlockQueryParams {
        event_docs: params.event_docs,
        extrinsic_docs: params.extrinsic_docs,
        no_fees: params.no_fees,
        use_evm_format: params.use_evm_format,
        ..BlockQueryParams::default()
    };

    if params.use_rc_block {
        let blocks =
            handle_use_rc_block_range(state.clone(), &base_block_params, start, end).await?;
        return Ok(Json(blocks).into_response());
    }

    let concurrency = state.config.express.block_fetch_concurrency;

    let blocks: Vec<BlockResponse> = stream::iter(start..=end)
        .map(|number| {
            let state = state.clone();
            let params = base_block_params.clone();
            async move {
                // Create client_at_block - this also resolves hash internally
                let client_at_block = state
                    .client
                    .at_block(number)
                    .await
                    .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))?;

                let block_hash = format!("{:#x}", client_at_block.block_hash());

                build_block_response_for_hash(
                    &state,
                    &block_hash,
                    number,
                    false,
                    &client_at_block,
                    &params,
                )
                .await
            }
        })
        .buffered(concurrency)
        .try_collect()
        .await?;

    Ok(Json(blocks).into_response())
}

async fn handle_use_rc_block_range(
    state: AppState,
    params: &BlockQueryParams,
    start: u64,
    end: u64,
) -> Result<Vec<BlockResponse>, GetBlockError> {
    use polkadot_rest_api_config::ChainType;

    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(GetBlockError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(GetBlockError::RelayChainNotConfigured);
    }

    let rc_rpc = state
        .get_relay_chain_rpc_client()
        .ok_or(GetBlockError::RelayChainNotConfigured)?
        .clone();
    let rc_legacy_rpc = state
        .get_relay_chain_rpc()
        .ok_or(GetBlockError::RelayChainNotConfigured)?
        .clone();

    let concurrency = state.config.express.block_fetch_concurrency;

    // Fetch RC blocks in parallel, each returning a Vec of AH BlockResponses
    let nested_results: Vec<Vec<BlockResponse>> = stream::iter(start..=end)
        .map(|rc_number| {
            let state = state.clone();
            let params = params.clone();
            let rc_rpc = rc_rpc.clone();
            let rc_legacy_rpc = rc_legacy_rpc.clone();
            async move {
                let rc_resolved_block =
                    resolve_rc_block(&rc_rpc, &rc_legacy_rpc, rc_number).await?;

                let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block)
                    .await
                    .map_err(|e| GetBlockError::RcBlockError(Box::new(e)))?;

                if ah_blocks.is_empty() {
                    return Ok::<Vec<BlockResponse>, GetBlockError>(Vec::new());
                }

                let rc_block_number = rc_resolved_block.number.to_string();
                let rc_block_hash = rc_resolved_block.hash.clone();

                let mut responses = Vec::with_capacity(ah_blocks.len());
                for ah_block in ah_blocks {
                    // Create client_at_block first - needed for build_block_response_for_hash
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

                    responses.push(response);
                }

                Ok(responses)
            }
        })
        .buffered(concurrency)
        .try_collect()
        .await?;

    // Flatten and sort results
    let mut results: Vec<BlockResponse> = nested_results.into_iter().flatten().collect();

    results.sort_by(|a, b| {
        let a_num = a.number.parse::<u64>().unwrap_or_default();
        let b_num = b.number.parse::<u64>().unwrap_or_default();
        a_num.cmp(&b_num)
    });

    Ok(results)
}

async fn resolve_rc_block(
    rc_rpc_client: &Arc<subxt_rpcs::RpcClient>,
    rc_legacy_rpc: &Arc<crate::state::SubstrateLegacyRpc>,
    rc_number: u64,
) -> Result<ResolvedBlock, GetBlockError> {
    let block_id = utils::BlockId::Number(rc_number);
    let resolved =
        utils::resolve_block_with_rpc(rc_rpc_client, rc_legacy_rpc, Some(block_id)).await?;
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocks_range_query_params_rejects_unknown_fields() {
        let json = r#"{"range": "1-10", "unknownField": true}"#;
        let result: Result<BlocksRangeQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_blocks_range_query_params_accepts_known_fields() {
        let json = r#"{
            "range": "100-200",
            "eventDocs": true,
            "extrinsicDocs": true,
            "noFees": true,
            "useRcBlock": true,
            "useEvmFormat": true
        }"#;
        let result: Result<BlocksRangeQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_ok());
        let params = result.unwrap();
        assert_eq!(params.range, Some("100-200".to_string()));
        assert!(params.event_docs);
        assert!(params.extrinsic_docs);
        assert!(params.no_fees);
        assert!(params.use_rc_block);
        assert!(params.use_evm_format);
    }
}
