//! Handler for GET /blocks/head endpoint.
//!
//! This module provides the handler for fetching the latest block (head).

use crate::state::AppState;
use crate::types::BlockHash;
use crate::utils::{compute_block_hash_from_header_json, parse_block_number_from_json};
use axum::{
    Json,
    extract::{Query, State},
};
use heck::{ToSnakeCase, ToUpperCamelCase};
use serde::Deserialize;
use subxt_rpcs::rpc_params;

use super::common::{add_docs_to_events, decode_digest_logs, extract_author};
use super::decode::XcmDecoder;
use super::docs::Docs;
use super::processing::{
    categorize_events, extract_extrinsics, extract_fee_info_for_extrinsic, fetch_block_events,
};
use super::types::{BlockResponse, GetBlockError};

// ================================================================================================
// Query Parameters
// ================================================================================================

/// Query parameters for /blocks/head endpoint
#[derive(Debug, Deserialize)]
pub struct BlockHeadQueryParams {
    /// When true (default), returns finalized head. When false, returns canonical head.
    #[serde(default = "default_true")]
    pub finalized: bool,
    /// When true, include documentation for events
    #[serde(default)]
    pub event_docs: bool,
    /// When true, include documentation for extrinsics
    #[serde(default)]
    pub extrinsic_docs: bool,
    /// When true, skip fee calculation for extrinsics (info will be empty object)
    #[serde(default)]
    pub no_fees: bool,
    /// When true, decode and include XCM messages from the block's extrinsics
    #[serde(default)]
    pub decoded_xcm_msgs: bool,
    /// Filter decoded XCM messages by parachain ID (only used when decodedXcmMsgs=true)
    #[serde(default)]
    pub para_id: Option<u32>,
}

fn default_true() -> bool {
    true
}

impl Default for BlockHeadQueryParams {
    fn default() -> Self {
        Self {
            finalized: true,
            event_docs: false,
            extrinsic_docs: false,
            no_fees: false,
            decoded_xcm_msgs: false,
            para_id: None,
        }
    }
}

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /blocks/head
///
/// Returns block information for the latest block (head)
///
/// Query Parameters:
/// - `finalized` (boolean, default: true): When true, returns finalized head. When false, returns canonical head.
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
/// - `noFees` (boolean, default: false): Skip fee calculation
/// - `decodedXcmMsgs` (boolean, default: false): Decode and include XCM messages
/// - `paraId` (number, optional): Filter XCM messages by parachain ID
pub async fn get_block_head(
    State(state): State<AppState>,
    Query(params): Query<BlockHeadQueryParams>,
) -> Result<Json<BlockResponse>, GetBlockError> {
    // Resolve head block hash based on finalized parameter
    // Returns (block_hash, block_number, is_finalized, header_json)
    let (block_hash, block_number, is_finalized, header_json) = if params.finalized {
        let finalized_hash = state
            .legacy_rpc
            .chain_get_finalized_head()
            .await
            .map_err(GetBlockError::FinalizedHeadFailed)?;
        let block_hash_typed = BlockHash::from(finalized_hash);
        let hash_str = block_hash_typed.to_string();
        let header_json = state
            .get_header_json(&hash_str)
            .await
            .map_err(GetBlockError::HeaderFetchFailed)?;
        let block_number = header_json
            .get("number")
            .ok_or_else(|| GetBlockError::HeaderFieldMissing("number".to_string()))
            .and_then(|v| {
                parse_block_number_from_json(v)
                    .map_err(|e| GetBlockError::HeaderFieldMissing(format!("number: {}", e)))
            })?;

        (hash_str, block_number, true, header_json)
    } else {
        // Get canonical head (may not be finalized)
        // We need to also fetch the finalized head to determine if canonical is finalized
        let (canonical_header_json, finalized_hash) = tokio::join!(
            async {
                state
                    .rpc_client
                    .request::<serde_json::Value>("chain_getHeader", rpc_params![])
                    .await
            },
            state.legacy_rpc.chain_get_finalized_head()
        );

        let header_json = canonical_header_json.map_err(GetBlockError::HeaderFetchFailed)?;
        let finalized_hash = finalized_hash.map_err(GetBlockError::FinalizedHeadFailed)?;

        // Extract block number from canonical header JSON
        let block_number = header_json
            .get("number")
            .ok_or_else(|| GetBlockError::HeaderFieldMissing("number".to_string()))
            .and_then(|v| {
                parse_block_number_from_json(v)
                    .map_err(|e| GetBlockError::HeaderFieldMissing(format!("number: {}", e)))
            })?;

        let finalized_hash_str = BlockHash::from(finalized_hash).to_string();
        let finalized_header_json = state
            .get_header_json(&finalized_hash_str)
            .await
            .map_err(GetBlockError::HeaderFetchFailed)?;
        let finalized_block_number = finalized_header_json
            .get("number")
            .ok_or_else(|| GetBlockError::HeaderFieldMissing("number".to_string()))
            .and_then(|v| {
                parse_block_number_from_json(v)
                    .map_err(|e| GetBlockError::HeaderFieldMissing(format!("number: {}", e)))
            })?;

        // Block is finalized if its number <= finalized head number
        let is_finalized = block_number <= finalized_block_number;
        let block_hash_typed = compute_block_hash_from_header_json(&header_json)?;
        let block_hash = block_hash_typed.to_string();

        (block_hash, block_number, is_finalized, header_json)
    };

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

    let client_at_block = state.client.at(block_number).await?;

    let (author_id, extrinsics_result, events_result) = tokio::join!(
        extract_author(&state, &client_at_block, &logs, block_number),
        extract_extrinsics(&state, &client_at_block, block_number),
        fetch_block_events(&state, &client_at_block, block_number),
    );

    let extrinsics = extrinsics_result?;
    let block_events = events_result?;

    // The finalized status is determined by comparing block number against finalized head:
    // - If finalized=true (default), we fetched the finalized head, so it IS finalized
    // - If finalized=false, we fetched the canonical head and checked if it's <= finalized head
    let finalized = Some(is_finalized);

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
            // Only update pays_fee from events if the extrinsic is SIGNED.
            // Unsigned extrinsics (inherents) never pay fees, regardless of what
            // DispatchInfo.paysFee says in the event.
            if extrinsic.signature.is_some() && outcome.pays_fee.is_some() {
                extrinsic.pays_fee = outcome.pays_fee;
            }
        }
    }

    // Populate fee info for signed extrinsics that pay fees (unless noFees=true)
    if !params.no_fees {
        let spec_version = state
            .get_runtime_version_at_hash(&block_hash)
            .await
            .ok()
            .and_then(|v| v.get("specVersion").and_then(|sv| sv.as_u64()))
            .map(|v| v as u32)
            .unwrap_or(state.chain_info.spec_version);

        for (i, extrinsic) in extrinsics_with_events.iter_mut().enumerate() {
            if extrinsic.signature.is_some() && extrinsic.pays_fee == Some(true) {
                extrinsic.info = extract_fee_info_for_extrinsic(
                    &state,
                    &extrinsic.raw_hex,
                    &extrinsic.events,
                    extrinsic_outcomes.get(i),
                    &parent_hash,
                    spec_version,
                )
                .await;
            }
        }
    }

    // Optionally populate documentation for events and extrinsics
    let (mut on_initialize, mut on_finalize) = (on_initialize, on_finalize);

    if params.event_docs || params.extrinsic_docs {
        let metadata = client_at_block.metadata();

        if params.event_docs {
            add_docs_to_events(&mut on_initialize.events, metadata);
            add_docs_to_events(&mut on_finalize.events, metadata);

            for extrinsic in extrinsics_with_events.iter_mut() {
                add_docs_to_events(&mut extrinsic.events, metadata);
            }
        }

        if params.extrinsic_docs {
            for extrinsic in extrinsics_with_events.iter_mut() {
                let pallet_name = extrinsic.method.pallet.to_upper_camel_case();
                let method_name = extrinsic.method.method.to_snake_case();
                extrinsic.docs =
                    Docs::for_call(metadata, &pallet_name, &method_name).map(|d| d.to_string());
            }
        }
    }

    // Decode XCM messages if requested
    let decoded_xcm_msgs = if params.decoded_xcm_msgs {
        let decoder = XcmDecoder::new(
            state.chain_info.chain_type.clone(),
            &extrinsics_with_events,
            params.para_id,
        );
        Some(decoder.decode())
    } else {
        None
    };

    let response = BlockResponse {
        number: block_number.to_string(),
        hash: block_hash,
        parent_hash,
        state_root,
        extrinsics_root,
        author_id,
        logs,
        on_initialize,
        extrinsics: extrinsics_with_events,
        on_finalize,
        finalized,
        decoded_xcm_msgs,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    Ok(Json(response))
}
