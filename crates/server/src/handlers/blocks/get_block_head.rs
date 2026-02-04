//! Handler for GET /blocks/head endpoint.
//!
//! This module provides the handler for fetching the latest block (head).

use crate::state::AppState;
use axum::{
    Json,
    extract::{Query, State},
};
use heck::{ToSnakeCase, ToUpperCamelCase};
use serde::Deserialize;

use super::common::{add_docs_to_events, convert_digest_items_to_logs, extract_author};
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
    let (client_at_block, is_finalized) = if params.finalized {
        let client = state
            .client
            .at_current_block()
            .await
            .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))?;

        (client, true)
    } else {
        let best_hash = state
            .legacy_rpc
            .chain_get_block_hash(None)
            .await
            .map_err(GetBlockError::RpcCallFailed)?
            .ok_or_else(|| GetBlockError::HeaderFieldMissing("best block hash".to_string()))?;

        let (canonical_client, finalized_client) = tokio::try_join!(
            async {
                state
                    .client
                    .at_block(best_hash)
                    .await
                    .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))
            },
            async {
                state
                    .client
                    .at_current_block()
                    .await
                    .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))
            }
        )?;

        let is_finalized = canonical_client.block_number() <= finalized_client.block_number();

        (canonical_client, is_finalized)
    };

    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    let header = client_at_block
        .block_header()
        .await
        .map_err(GetBlockError::BlockHeaderFailed)?;

    let parent_hash = format!("{:#x}", header.parent_hash);
    let state_root = format!("{:#x}", header.state_root);
    let extrinsics_root = format!("{:#x}", header.extrinsics_root);

    let logs = convert_digest_items_to_logs(&header.digest.logs);

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
        let spec_version = client_at_block.spec_version();
        let client_at_parent = state.client.at_block(header.parent_hash).await?;

        for (i, extrinsic) in extrinsics_with_events.iter_mut().enumerate() {
            if extrinsic.signature.is_some() && extrinsic.pays_fee == Some(true) {
                extrinsic.info = extract_fee_info_for_extrinsic(
                    &state,
                    &client_at_parent,
                    &extrinsic.raw_hex,
                    &extrinsic.events,
                    extrinsic_outcomes.get(i),
                    spec_version,
                    &state.chain_info.spec_name,
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
