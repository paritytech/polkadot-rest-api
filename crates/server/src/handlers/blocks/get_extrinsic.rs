//! Handler for GET /blocks/{blockId}/extrinsics/{extrinsicIndex} endpoint.
//!
//! This module provides the handler for fetching a specific extrinsic by its index
//! within a block.

use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use serde_json::json;

use super::common::{add_docs_to_events, add_docs_to_extrinsic, associate_events_with_extrinsics};
use super::processing::{
    categorize_events, extract_extrinsics, extract_fee_info_for_extrinsic, fetch_block_events,
};
use super::types::{
    BlockIdentifiers, ExtrinsicIndexResponse, ExtrinsicPathParams, ExtrinsicQueryParams,
    GetBlockError,
};

/// Handler for GET /blocks/{blockId}/extrinsics/{extrinsicIndex}
///
/// Returns a specific extrinsic from a block by its index
///
/// Query Parameters:
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
/// - `noFees` (boolean, default: false): Skip fee calculation (info will be empty object)
/// - `useRcBlock` (boolean, default: false): When true, treat blockId as Relay Chain block and return Asset Hub extrinsics
#[utoipa::path(
    get,
    path = "/v1/blocks/{blockId}/extrinsics/{extrinsicIndex}",
    tag = "blocks",
    summary = "Get extrinsic by index",
    description = "Returns a specific extrinsic from a block by its index within the block.",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash"),
        ("extrinsicIndex" = String, Path, description = "Index of the extrinsic within the block"),
        ("eventDocs" = Option<bool>, Query, description = "Include documentation for events"),
        ("extrinsicDocs" = Option<bool>, Query, description = "Include documentation for extrinsics"),
        ("noFees" = Option<bool>, Query, description = "Skip fee calculation")
    ),
    responses(
        (status = 200, description = "Extrinsic details", body = Object),
        (status = 400, description = "Invalid block identifier or extrinsic index"),
        (status = 404, description = "Extrinsic not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_extrinsic(
    State(state): State<AppState>,
    Path(path_params): Path<ExtrinsicPathParams>,
    Query(params): Query<ExtrinsicQueryParams>,
) -> Result<Response, GetBlockError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, path_params, params).await;
    }

    let extrinsic_index: usize = path_params
        .extrinsic_index
        .parse()
        .map_err(|_| GetBlockError::InvalidExtrinsicIndex(path_params.extrinsic_index.clone()))?;

    let block_id_parsed = path_params.block_id.parse::<utils::BlockId>()?;
    let client_at_block = match block_id_parsed {
        utils::BlockId::Hash(hash) => state.client.at_block(hash).await?,
        utils::BlockId::Number(number) => state.client.at_block(number).await?,
    };

    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    let response = build_extrinsic_response(
        &state,
        &block_hash,
        block_number,
        extrinsic_index,
        &client_at_block,
        &params,
    )
    .await?;

    Ok(Json(response).into_response())
}

async fn handle_use_rc_block(
    state: AppState,
    path_params: ExtrinsicPathParams,
    params: ExtrinsicQueryParams,
) -> Result<Response, GetBlockError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(GetBlockError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(GetBlockError::RelayChainNotConfigured);
    }

    let extrinsic_index: usize = path_params
        .extrinsic_index
        .parse()
        .map_err(|_| GetBlockError::InvalidExtrinsicIndex(path_params.extrinsic_index.clone()))?;

    let rc_block_id = path_params.block_id.parse::<utils::BlockId>()?;
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

        let mut response = build_extrinsic_response(
            &state,
            &ah_block.hash,
            ah_block.number,
            extrinsic_index,
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

async fn build_extrinsic_response(
    state: &AppState,
    block_hash: &str,
    block_number: u64,
    extrinsic_index: usize,
    client_at_block: &super::common::BlockClient,
    params: &ExtrinsicQueryParams,
) -> Result<ExtrinsicIndexResponse, GetBlockError> {
    let header = client_at_block
        .block_header()
        .await
        .map_err(GetBlockError::BlockHeaderFailed)?;

    let (extrinsics_result, events_result) = tokio::join!(
        extract_extrinsics(state, client_at_block, block_number),
        fetch_block_events(state, client_at_block, block_number),
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
        let spec_version = client_at_block.spec_version();
        let client_at_parent = state.client.at_block(header.parent_hash).await?;

        let fee_info = extract_fee_info_for_extrinsic(
            state,
            &client_at_parent,
            &extrinsic.raw_hex,
            &extrinsic.events,
            extrinsic_outcomes.get(extrinsic_index),
            spec_version,
            &state.chain_info.spec_name,
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

    Ok(ExtrinsicIndexResponse {
        at: BlockIdentifiers {
            height: block_number.to_string(),
            hash: block_hash.to_string(),
        },
        extrinsics: extrinsic,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {

    #[test]
    fn test_parse_extrinsic_index_valid() {
        let index: Result<usize, _> = "0".parse();
        assert!(index.is_ok());
        assert_eq!(index.unwrap(), 0);

        let index: Result<usize, _> = "10".parse();
        assert!(index.is_ok());
        assert_eq!(index.unwrap(), 10);
    }

    #[test]
    fn test_parse_extrinsic_index_invalid() {
        let index: Result<usize, _> = "-1".parse();
        assert!(index.is_err());

        let index: Result<usize, _> = "abc".parse();
        assert!(index.is_err());

        let index: Result<usize, _> = "1.5".parse();
        assert!(index.is_err());
    }
}
