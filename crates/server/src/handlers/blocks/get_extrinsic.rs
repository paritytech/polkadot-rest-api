//! Handler for GET /blocks/{blockId}/extrinsics/{extrinsicIndex} endpoint.
//!
//! This module provides the handler for fetching a specific extrinsic by its index
//! within a block.

use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};

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
pub async fn get_extrinsic(
    State(state): State<AppState>,
    Path(path_params): Path<ExtrinsicPathParams>,
    Query(params): Query<ExtrinsicQueryParams>,
) -> Result<impl IntoResponse, GetBlockError> {
    let extrinsic_index: usize = path_params
        .extrinsic_index
        .parse()
        .map_err(|_| GetBlockError::InvalidExtrinsicIndex(path_params.extrinsic_index.clone()))?;

    let block_id_parsed = path_params.block_id.parse::<utils::BlockId>()?;
    let resolved_block = utils::resolve_block(&state, Some(block_id_parsed)).await?;

    let block_hash = &resolved_block.hash;
    let block_number = resolved_block.number;

    let header_json = state
        .get_header_json(block_hash)
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;

    let parent_hash = header_json
        .get("parentHash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("parentHash".to_string()))?
        .to_string();

    let client_at_block = state.client.at_block(block_number).await?;

    let (extrinsics_result, events_result) = tokio::join!(
        extract_extrinsics(&state, &client_at_block, block_number),
        fetch_block_events(&state, &client_at_block, block_number),
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
        let spec_version = state
            .get_runtime_version_at_hash(block_hash)
            .await
            .map_err(GetBlockError::RuntimeVersionFailed)?
            .get("specVersion")
            .and_then(|sv| sv.as_u64())
            .map(|v| v as u32)
            .ok_or_else(|| GetBlockError::HeaderFieldMissing("specVersion".to_string()))?;

        let fee_info = extract_fee_info_for_extrinsic(
            &state,
            None,
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
