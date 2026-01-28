//! Handler for GET /rc/blocks/{blockId}/extrinsics/{extrinsicIndex} endpoint.
//!
//! This module provides the handler for fetching a specific extrinsic by its index
//! within a Relay Chain block.

use crate::handlers::blocks::common::{
    add_docs_to_events, add_docs_to_extrinsic, associate_events_with_extrinsics,
};
use crate::handlers::blocks::processing::{
    categorize_events, extract_extrinsics_with_prefix, extract_fee_info_for_extrinsic,
    fetch_block_events_with_prefix,
};
use crate::handlers::blocks::types::{
    BlockIdentifiers, ExtrinsicIndexResponse, ExtrinsicPathParams, ExtrinsicQueryParams,
    GetBlockError,
};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use subxt_rpcs::rpc_params;

/// Handler for GET /rc/blocks/{blockId}/extrinsics/{extrinsicIndex}
///
/// Returns a specific extrinsic from a Relay Chain block by its index
///
/// Query Parameters:
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
/// - `noFees` (boolean, default: false): Skip fee calculation (info will be empty object)
pub async fn get_rc_extrinsic(
    State(state): State<AppState>,
    Path(path_params): Path<ExtrinsicPathParams>,
    Query(params): Query<ExtrinsicQueryParams>,
) -> Result<impl IntoResponse, GetBlockError> {
    let extrinsic_index: usize = path_params
        .extrinsic_index
        .parse()
        .map_err(|_| GetBlockError::InvalidExtrinsicIndex(path_params.extrinsic_index.clone()))?;

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

    let ss58_prefix = relay_chain_info.ss58_prefix;

    let block_id_parsed = path_params.block_id.parse::<utils::BlockId>()?;
    let resolved_block =
        utils::resolve_block_with_rpc(&relay_rpc_client, &relay_chain_rpc, Some(block_id_parsed))
            .await?;

    let block_hash = &resolved_block.hash;
    let block_number = resolved_block.number;

    let client_at_block = relay_client.at_block(block_number).await?;

    let header = client_at_block
        .block_header()
        .await
        .map_err(GetBlockError::BlockHeaderFailed)?;
    let parent_hash = format!("{:#x}", header.parent_hash);

    let (extrinsics_result, events_result) = tokio::join!(
        extract_extrinsics_with_prefix(ss58_prefix, &client_at_block, block_number),
        fetch_block_events_with_prefix(ss58_prefix, &client_at_block, block_number),
    );

    let extrinsics = extrinsics_result?;
    let block_events = events_result?;

    if extrinsic_index >= extrinsics.len() {
        return Err(GetBlockError::ExtrinsicIndexNotFound);
    }

    let (_on_initialize, per_extrinsic_events, _on_finalize, extrinsic_outcomes) =
        categorize_events(block_events, extrinsics.len());

    let mut extrinsics_with_events = extrinsics;
    associate_events_with_extrinsics(
        &mut extrinsics_with_events,
        &per_extrinsic_events,
        &extrinsic_outcomes,
    );

    let mut extrinsic = extrinsics_with_events
        .into_iter()
        .nth(extrinsic_index)
        .ok_or(GetBlockError::ExtrinsicIndexNotFound)?;

    if !params.no_fees && extrinsic.signature.is_some() && extrinsic.pays_fee == Some(true) {
        let spec_version: serde_json::Value = relay_rpc_client
            .request("state_getRuntimeVersion", rpc_params![block_hash])
            .await
            .map_err(GetBlockError::RuntimeVersionFailed)?;

        let spec_version = spec_version
            .get("specVersion")
            .and_then(|sv| sv.as_u64())
            .map(|v| v as u32)
            .ok_or_else(|| GetBlockError::HeaderFieldMissing("specVersion".to_string()))?;

        let fee_info = extract_fee_info_for_extrinsic(
            &state,
            Some(&relay_rpc_client),
            &extrinsic.raw_hex,
            &extrinsic.events,
            extrinsic_outcomes.get(extrinsic_index),
            &parent_hash,
            spec_version,
        )
        .await;

        extrinsic.info = fee_info;
    }

    if params.event_docs || params.extrinsic_docs {
        let metadata = client_at_block.metadata();

        if params.event_docs {
            add_docs_to_events(&mut extrinsic.events, &metadata);
        }

        if params.extrinsic_docs {
            add_docs_to_extrinsic(&mut extrinsic, &metadata);
        }
    }

    let response = ExtrinsicIndexResponse {
        at: BlockIdentifiers {
            height: block_number.to_string(),
            hash: block_hash.to_string(),
        },
        extrinsics: extrinsic,
    };

    Ok(Json(response))
}
