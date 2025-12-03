//! Handler for GET /blocks/{blockId} endpoint.
//!
//! This module provides the main handler for fetching block information.

use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use heck::{ToSnakeCase, ToUpperCamelCase};

use super::common::{
    add_docs_to_events, categorize_events, decode_digest_logs, extract_author, extract_extrinsics,
    extract_fee_info_for_extrinsic, fetch_block_events, get_canonical_hash_at_number,
    get_finalized_block_number,
};
use super::docs::Docs;
use super::types::{BlockQueryParams, BlockResponse, GetBlockError};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /blocks/{blockId}
///
/// Returns block information for a given block identifier (hash or number)
///
/// Query Parameters:
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
pub async fn get_block(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<BlockQueryParams>,
) -> Result<Json<BlockResponse>, GetBlockError> {
    let block_id = block_id.parse::<utils::BlockId>()?;
    // Track if the block was queried by hash (needed for canonical chain check)
    let queried_by_hash = matches!(block_id, utils::BlockId::Hash(_));
    let resolved_block = utils::resolve_block(&state, Some(block_id)).await?;
    let header_json = state
        .get_header_json(&resolved_block.hash)
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

    // Create client_at_block once and reuse for all operations
    let client_at_block = state.client.at(resolved_block.number).await?;

    let (author_id, extrinsics_result, events_result, finalized_head_result, canonical_hash_result) = tokio::join!(
        extract_author(&state, &client_at_block, &logs),
        extract_extrinsics(&state, &client_at_block),
        fetch_block_events(&state, &client_at_block),
        get_finalized_block_number(&state),
        // Only fetch canonical hash if queried by hash (needed for fork detection)
        async {
            if queried_by_hash {
                Some(get_canonical_hash_at_number(&state, resolved_block.number).await)
            } else {
                None
            }
        }
    );

    let extrinsics = extrinsics_result?;
    let block_events = events_result?;
    let finalized_head_number = finalized_head_result?;

    // Determine if the block is finalized:
    // 1. Block number must be <= finalized head number
    // 2. If queried by hash, the hash must match the canonical chain hash
    //    (to detect blocks on forked/orphaned chains)
    let finalized = if resolved_block.number <= finalized_head_number {
        if let Some(canonical_result) = canonical_hash_result {
            // Queried by hash - verify it's on the canonical chain
            match canonical_result? {
                Some(canonical_hash) => canonical_hash == resolved_block.hash,
                // If canonical hash not found, block is not finalized
                None => false,
            }
        } else {
            // Queried by number - assumed to be canonical
            true
        }
    } else {
        false
    };

    // Categorize events by phase and extract extrinsic outcomes (success, paysFee)
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
            // DispatchInfo.paysFee says in the event. The event's paysFee indicates
            // whether the call *would* pay a fee if called as a transaction, but
            // inherents are inserted by block authors and don't actually pay fees.
            if extrinsic.signature.is_some() && outcome.pays_fee.is_some() {
                extrinsic.pays_fee = outcome.pays_fee;
            }
        }
    }

    // Populate fee info for signed extrinsics that pay fees (unless noFees=true)
    if !params.no_fees {
        let spec_version = state
            .get_runtime_version_at_hash(&resolved_block.hash)
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
        // Reuse the client_at_block we created earlier
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
                // Pallet names in metadata are PascalCase, but our pallet names are lowerCamelCase
                // We need to convert back: "system" -> "System", "balances" -> "Balances"
                // Method names in metadata are snake_case, but our method names are lowerCamelCase
                let pallet_name = extrinsic.method.pallet.to_upper_camel_case();
                let method_name = extrinsic.method.method.to_snake_case();
                extrinsic.docs =
                    Docs::for_call(metadata, &pallet_name, &method_name).map(|d| d.to_string());
            }
        }
    }

    let response = BlockResponse {
        number: resolved_block.number.to_string(),
        hash: resolved_block.hash,
        parent_hash,
        state_root,
        extrinsics_root,
        author_id,
        logs,
        on_initialize,
        extrinsics: extrinsics_with_events,
        on_finalize,
        finalized,
    };

    Ok(Json(response))
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
    fn create_test_state_with_mock(mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let rpc_client = Arc::new(RpcClient::new(mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: config::ChainType::Relay,
            spec_name: "test".to_string(),
            spec_version: 1,
            ss58_prefix: 42,
        };

        AppState {
            config,
            client: Arc::new(subxt_historic::OnlineClient::from_rpc_client(
                subxt_historic::SubstrateConfig::new(),
                (*rpc_client).clone(),
            )),
            legacy_rpc,
            rpc_client,
            chain_info,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
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

        let state = create_test_state_with_mock(mock_client);
        let block_id = "100".to_string();
        let params = BlockQueryParams::default();

        // Attempt to get the block - this will fail at metadata fetch in current setup
        // but validates the handler flow up to that point
        let result = get_block(State(state), Path(block_id), Query(params)).await;

        // We expect an error due to metadata fetching limitations in mock environment
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_digest_logs() {
        use serde_json::json;

        // Test decoding PreRuntime BABE log
        let header_json = json!({
            "digest": {
                "logs": [
                    // PreRuntime (6) + BABE engine (42414245) + payload
                    "0x0642414245340201000000ef55a50f00000000"
                ]
            }
        });

        let logs = decode_digest_logs(&header_json);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].log_type, "PreRuntime");
        assert_eq!(logs[0].index, "6");

        // The value should be [engine_id, payload]
        let arr = logs[0].value.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_str().unwrap(), "0x42414245"); // "BABE" in hex
    }

    #[test]
    fn test_decode_seal_log() {
        use serde_json::json;

        // Test decoding Seal log
        // Format: discriminant (05) + engine_id (42414245 = "BABE") + SCALE compact length (0101 = 64) + 64 bytes of signature data
        let header_json = json!({
            "digest": {
                "logs": [
                    // Seal (5) + BABE engine (42414245) + compact length (0101 = 64 bytes) + 64 bytes of signature
                    "0x05424142450101aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                ]
            }
        });

        let logs = decode_digest_logs(&header_json);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].log_type, "Seal");
        assert_eq!(logs[0].index, "5");
    }

    #[test]
    fn test_decode_runtime_environment_updated() {
        use serde_json::json;

        // Test decoding RuntimeEnvironmentUpdated log (discriminant 8, no data)
        let header_json = json!({
            "digest": {
                "logs": [
                    "0x08"
                ]
            }
        });

        let logs = decode_digest_logs(&header_json);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].log_type, "RuntimeEnvironmentUpdated");
        assert_eq!(logs[0].index, "8");
        assert!(logs[0].value.is_null());
    }
}
