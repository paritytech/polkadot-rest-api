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

/// Handler for GET /rc/blocks/{blockId}/extrinsics/{extrinsicIndex}
///
/// Returns a specific extrinsic from a Relay Chain block by its index
///
/// Query Parameters:
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
/// - `noFees` (boolean, default: false): Skip fee calculation (info will be empty object)
#[utoipa::path(
    get,
    path = "/v1/rc/blocks/{blockId}/extrinsics/{extrinsicIndex}",
    tag = "rc",
    summary = "RC get extrinsic by index",
    description = "Returns a specific extrinsic from a relay chain block by its index.",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash"),
        ("extrinsicIndex" = String, Path, description = "Index of the extrinsic in the block"),
        ("eventDocs" = Option<bool>, Query, description = "Include event documentation"),
        ("extrinsicDocs" = Option<bool>, Query, description = "Include extrinsic documentation"),
        ("noFees" = Option<bool>, Query, description = "Skip fee calculation")
    ),
    responses(
        (status = 200, description = "Extrinsic details", body = Object),
        (status = 400, description = "Invalid block ID or extrinsic index"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
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
    let relay_chain_info = state
        .relay_chain_info
        .clone()
        .ok_or(GetBlockError::RelayChainNotConfigured)?;

    let ss58_prefix = relay_chain_info.ss58_prefix;

    let block_id_parsed = path_params.block_id.parse::<utils::BlockId>()?;
    let client_at_block = match block_id_parsed {
        utils::BlockId::Hash(hash) => relay_client.at_block(hash).await?,
        utils::BlockId::Number(number) => relay_client.at_block(number).await?,
    };

    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    let header = client_at_block
        .block_header()
        .await
        .map_err(GetBlockError::BlockHeaderFailed)?;

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
        let spec_version = client_at_block.spec_version();
        let client_at_parent = relay_client.at_block(header.parent_hash).await?;

        let fee_info = extract_fee_info_for_extrinsic(
            &state,
            &client_at_parent,
            &extrinsic.raw_hex,
            &extrinsic.events,
            extrinsic_outcomes.get(extrinsic_index),
            spec_version,
            &relay_chain_info.spec_name,
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
            hash: block_hash,
        },
        extrinsics: extrinsic,
    };

    Ok(Json(response))
}
