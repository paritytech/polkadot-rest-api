use crate::handlers::blocks::common::{
    BlockClient, add_docs_to_events, decode_digest_logs, extract_author_with_prefix,
    get_canonical_hash_at_number_with_rpc, get_finalized_block_number_with_rpc, parse_range,
};
use crate::handlers::blocks::decode::XcmDecoder;
use crate::handlers::blocks::docs::Docs;
use crate::handlers::blocks::processing::{
    categorize_events, extract_extrinsics_with_prefix, extract_fee_info_for_extrinsic,
    fetch_block_events_with_prefix,
};
use crate::handlers::blocks::types::{BlockQueryParams, BlockResponse, GetBlockError};
use crate::state::AppState;
use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use futures::stream::{self, StreamExt, TryStreamExt};
use heck::{ToSnakeCase, ToUpperCamelCase};
use serde::Deserialize;
use std::sync::Arc;
use subxt_rpcs::{RpcClient, rpc_params};

/// Query parameters for GET /rc/blocks endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlocksRangeQueryParams {
    /// Range of blocks to fetch
    pub range: Option<String>,
    /// Include documentation for events
    #[serde(default)]
    pub event_docs: bool,
    /// Include documentation for extrinsics
    #[serde(default)]
    pub extrinsic_docs: bool,
    /// Skip fee calculation
    #[serde(default)]
    pub no_fees: bool,
    /// When true, decode and include XCM messages from the block's extrinsics
    #[serde(default)]
    pub decoded_xcm_msgs: bool,
    /// Filter messages by parachain ID
    #[serde(default)]
    pub para_id: Option<u32>,
}

/// Handler for GET /rc/blocks
///
/// Returns a collection of Relay Chain blocks given a numeric range.
///
/// Query Parameters:
/// - `range` (required): Range of block numbers (e.g., "100-200"). Max 500 blocks.
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
/// - `noFees` (boolean, default: false): Skip fee calculation
/// - `decodedXcmMsgs` (boolean, default: false): Decode and include XCM messages
/// - `paraId` (number, optional): Filter XCM messages by parachain ID
pub async fn get_rc_blocks(
    State(state): State<AppState>,
    Query(params): Query<RcBlocksRangeQueryParams>,
) -> Result<Response, GetBlockError> {
    let range_str = params.range.clone().ok_or(GetBlockError::MissingRange)?;

    let (start, end) = parse_range(&range_str)?;

    let relay_client = state
        .get_relay_chain_client()
        .ok_or(GetBlockError::RelayChainNotConfigured)?
        .clone();
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(GetBlockError::RelayChainNotConfigured)?
        .clone();
    let relay_chain_rpc = state
        .get_relay_chain_rpc()
        .ok_or(GetBlockError::RelayChainNotConfigured)?
        .clone();

    let relay_chain_info = state
        .relay_chain_info
        .clone()
        .ok_or(GetBlockError::RelayChainNotConfigured)?;

    let base_params = BlockQueryParams {
        event_docs: params.event_docs,
        extrinsic_docs: params.extrinsic_docs,
        no_fees: params.no_fees,
        finalized_key: true, // RC blocks endpoint includes finalized field
        decoded_xcm_msgs: params.decoded_xcm_msgs,
        para_id: params.para_id,
        ..BlockQueryParams::default()
    };

    let finalized_block_number =
        get_finalized_block_number_with_rpc(&relay_chain_rpc, &relay_rpc_client).await?;

    let concurrency = state.config.express.block_fetch_concurrency;

    let blocks: Vec<BlockResponse> = stream::iter(start..=end)
        .map(|number| {
            let relay_client = relay_client.clone();
            let relay_rpc_client = relay_rpc_client.clone();
            let relay_chain_rpc = relay_chain_rpc.clone();
            let relay_chain_info = relay_chain_info.clone();
            let params = base_params.clone();
            let state = state.clone();
            async move {
                let client_at_block = relay_client
                    .at_block(number)
                    .await
                    .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))?;

                let block_hash = format!("{:#x}", client_at_block.block_hash());

                let is_finalized = if number <= finalized_block_number {
                    match get_canonical_hash_at_number_with_rpc(&relay_chain_rpc, number).await? {
                        Some(canonical_hash) => canonical_hash == block_hash,
                        None => false,
                    }
                } else {
                    false
                };

                build_rc_block_response(
                    &state,
                    &relay_rpc_client,
                    &relay_chain_info,
                    &block_hash,
                    number,
                    &client_at_block,
                    &params,
                    is_finalized,
                )
                .await
            }
        })
        .buffered(concurrency)
        .try_collect()
        .await?;

    Ok(Json(blocks).into_response())
}

/// Build a BlockResponse for a Relay Chain block
async fn build_rc_block_response(
    state: &AppState,
    relay_rpc_client: &Arc<RpcClient>,
    relay_chain_info: &crate::state::ChainInfo,
    block_hash: &str,
    block_number: u64,
    client_at_block: &BlockClient,
    params: &BlockQueryParams,
    is_finalized: bool,
) -> Result<BlockResponse, GetBlockError> {
    let ss58_prefix = relay_chain_info.ss58_prefix;

    let header_json: serde_json::Value = relay_rpc_client
        .request("chain_getHeader", rpc_params![block_hash])
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

    let extrinsics_root = header_json
        .get("extrinsicsRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("extrinsicsRoot".to_string()))?
        .to_string();

    let logs = decode_digest_logs(&header_json);

    let author_id =
        extract_author_with_prefix(ss58_prefix, client_at_block, &logs, block_number).await;

    let (extrinsics_result, events_result) = tokio::join!(
        extract_extrinsics_with_prefix(ss58_prefix, client_at_block, block_number),
        fetch_block_events_with_prefix(ss58_prefix, client_at_block, block_number),
    );

    let extrinsics = extrinsics_result?;
    let block_events = events_result?;

    let (on_initialize, per_extrinsic_events, on_finalize, extrinsic_outcomes) =
        categorize_events(block_events, extrinsics.len());

    let mut extrinsics_with_events = extrinsics;
    for (i, (extrinsic_events, outcome)) in per_extrinsic_events
        .iter()
        .zip(extrinsic_outcomes.iter())
        .enumerate()
    {
        if let Some(extrinsic) = extrinsics_with_events.get_mut(i) {
            extrinsic.events = extrinsic_events.clone();
            extrinsic.success = outcome.success;
            if extrinsic.signature.is_some() && outcome.pays_fee.is_some() {
                extrinsic.pays_fee = outcome.pays_fee;
            }
        }
    }

    if !params.no_fees {
        let fee_indices: Vec<usize> = extrinsics_with_events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.signature.is_some() && e.pays_fee == Some(true))
            .map(|(i, _)| i)
            .collect();

        if !fee_indices.is_empty() {
            let spec_version: serde_json::Value = relay_rpc_client
                .request("state_getRuntimeVersion", rpc_params![block_hash])
                .await
                .map_err(GetBlockError::RuntimeVersionFailed)?;

            let spec_version = spec_version
                .get("specVersion")
                .and_then(|sv| sv.as_u64())
                .map(|v| v as u32)
                .ok_or_else(|| GetBlockError::HeaderFieldMissing("specVersion".to_string()))?;

            let fee_futures: Vec<_> = fee_indices
                .iter()
                .map(|&i| {
                    let extrinsic = &extrinsics_with_events[i];
                    extract_fee_info_for_extrinsic(
                        state,
                        Some(relay_rpc_client),
                        &extrinsic.raw_hex,
                        &extrinsic.events,
                        extrinsic_outcomes.get(i),
                        &parent_hash,
                        spec_version,
                    )
                })
                .collect();

            let fee_results = futures::future::join_all(fee_futures).await;

            for (idx, fee_info) in fee_indices.into_iter().zip(fee_results.into_iter()) {
                extrinsics_with_events[idx].info = fee_info;
            }
        }
    }

    let (mut on_initialize, mut on_finalize) = (on_initialize, on_finalize);

    if params.event_docs || params.extrinsic_docs {
        let metadata = client_at_block.metadata();

        if params.event_docs {
            add_docs_to_events(&mut on_initialize.events, &metadata);
            add_docs_to_events(&mut on_finalize.events, &metadata);

            for extrinsic in extrinsics_with_events.iter_mut() {
                add_docs_to_events(&mut extrinsic.events, &metadata);
            }
        }

        if params.extrinsic_docs {
            for extrinsic in extrinsics_with_events.iter_mut() {
                let pallet_name = extrinsic.method.pallet.to_upper_camel_case();
                let method_name = extrinsic.method.method.to_snake_case();
                extrinsic.docs = Docs::for_call_subxt(&metadata, &pallet_name, &method_name)
                    .map(|d| d.to_string());
            }
        }
    }

    let decoded_xcm_msgs = if params.decoded_xcm_msgs {
        let decoder = XcmDecoder::new(
            relay_chain_info.chain_type.clone(),
            &extrinsics_with_events,
            params.para_id,
        );
        Some(decoder.decode())
    } else {
        None
    };

    Ok(BlockResponse {
        number: block_number.to_string(),
        hash: block_hash.to_string(),
        parent_hash,
        state_root,
        extrinsics_root,
        author_id,
        logs,
        on_initialize,
        extrinsics: extrinsics_with_events,
        on_finalize,
        finalized: Some(is_finalized),
        decoded_xcm_msgs,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}
