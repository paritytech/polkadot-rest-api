//! Handler for GET /blocks/{blockId} endpoint.
//!
//! This module provides the main handler for fetching block information.

use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use heck::{ToSnakeCase, ToUpperCamelCase};
use serde_json::json;

use super::common::{
    add_docs_to_events, decode_digest_logs, extract_author, get_canonical_hash_at_number,
    get_finalized_block_number,
};
use super::decode::XcmDecoder;
use super::docs::Docs;
use super::processing::{
    categorize_events, extract_extrinsics, extract_fee_info_for_extrinsic, fetch_block_events,
};
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
/// - `useRcBlock` (boolean, default: false): When true, treat blockId as Relay Chain block and return Asset Hub blocks
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
        let mut response =
            build_block_response_for_hash(&state, &ah_block.hash, ah_block.number, true, &params)
                .await?;

        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        let client_at_block = state
            .client
            .at_block(ah_block.number)
            .await
            .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))?;
        let timestamp_addr = subxt::dynamic::storage::<(), u64>("Timestamp", "Now");
        if let Ok(timestamp) = client_at_block.storage().fetch(timestamp_addr, ()).await
            && let Ok(timestamp_value) = timestamp.decode()
        {
            response.ah_timestamp = Some(timestamp_value.to_string());
        }

        results.push(response);
    }

    Ok(Json(json!(results)).into_response())
}

async fn build_block_response(
    state: &AppState,
    block_id: String,
    params: &BlockQueryParams,
) -> Result<BlockResponse, GetBlockError> {
    let block_id_parsed = block_id.parse::<utils::BlockId>()?;
    let queried_by_hash = matches!(block_id_parsed, utils::BlockId::Hash(_));
    let resolved_block = utils::resolve_block(state, Some(block_id_parsed)).await?;
    build_block_response_for_hash(
        state,
        &resolved_block.hash,
        resolved_block.number,
        queried_by_hash,
        params,
    )
    .await
}

async fn build_block_response_for_hash(
    state: &AppState,
    block_hash: &str,
    block_number: u64,
    queried_by_hash: bool,
    params: &BlockQueryParams,
) -> Result<BlockResponse, GetBlockError> {
    let header_json = state
        .get_header_json(block_hash)
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
    let client_at_block = state
        .client
        .at_block(block_number)
        .await
        .map_err(|e| GetBlockError::ClientAtBlockFailed(Box::new(e)))?;

    let (author_id, extrinsics_result, events_result, finalized_head_result, canonical_hash_result) = tokio::join!(
        extract_author(state, &client_at_block, &logs, block_number),
        extract_extrinsics(state, &client_at_block, block_number),
        fetch_block_events(state, &client_at_block, block_number),
        // Only fetch canonical hash if queried by hash (needed for fork detection)
        async {
            if params.finalized_key {
                Some(get_finalized_block_number(state).await)
            } else {
                None
            }
        },
        // Only fetch canonical hash if queried by hash AND finalizedKey=true
        async {
            if queried_by_hash && params.finalized_key {
                Some(get_canonical_hash_at_number(state, block_number).await)
            } else {
                None
            }
        },
    );

    let extrinsics = extrinsics_result?;
    let block_events = events_result?;

    // Determine if the block is finalized (only when finalizedKey=true)
    let finalized = if let Some(finalized_head_result) = finalized_head_result {
        let finalized_head_number = finalized_head_result?;
        // 1. Block number must be <= finalized head number
        // 2. If queried by hash, the hash must match the canonical chain hash
        //    (to detect blocks on forked/orphaned chains)
        let is_finalized = if block_number <= finalized_head_number {
            if let Some(canonical_result) = canonical_hash_result {
                // Queried by hash - verify it's on the canonical chain
                match canonical_result? {
                    Some(canonical_hash) => canonical_hash == block_hash,
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
        Some(is_finalized)
    } else {
        // finalizedKey=false, omit finalized field
        None
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
    // Optimization: Only fetch runtime version and process fees if there are extrinsics that need it
    if !params.no_fees {
        // Find indices of extrinsics that need fee calculation
        let fee_indices: Vec<usize> = extrinsics_with_events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.signature.is_some() && e.pays_fee == Some(true))
            .map(|(i, _)| i)
            .collect();

        // Only fetch runtime version if there are extrinsics that need fees
        if !fee_indices.is_empty() {
            let spec_version = state
                .get_runtime_version_at_hash(block_hash)
                .await
                .map_err(GetBlockError::RuntimeVersionFailed)?
                .get("specVersion")
                .and_then(|sv| sv.as_u64())
                .map(|v| v as u32)
                .ok_or_else(|| GetBlockError::HeaderFieldMissing("specVersion".to_string()))?;

            // Parallelize fee extraction for all extrinsics that need it
            let fee_futures: Vec<_> = fee_indices
                .iter()
                .map(|&i| {
                    let extrinsic = &extrinsics_with_events[i];
                    extract_fee_info_for_extrinsic(
                        state,
                        &extrinsic.raw_hex,
                        &extrinsic.events,
                        extrinsic_outcomes.get(i),
                        &parent_hash,
                        spec_version,
                    )
                })
                .collect();

            let fee_results = futures::future::join_all(fee_futures).await;

            // Apply fee results back to extrinsics
            for (idx, fee_info) in fee_indices.into_iter().zip(fee_results.into_iter()) {
                extrinsics_with_events[idx].info = fee_info;
            }
        }
    }

    // Optionally populate documentation for events and extrinsics
    let (mut on_initialize, mut on_finalize) = (on_initialize, on_finalize);

    if params.event_docs || params.extrinsic_docs {
        // Reuse the client_at_block we created earlier
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
                // Pallet names in metadata are PascalCase, but our pallet names are lowerCamelCase
                // We need to convert back: "system" -> "System", "balances" -> "Balances"
                // Method names in metadata are snake_case, but our method names are lowerCamelCase
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
        finalized,
        decoded_xcm_msgs,
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

        let client = subxt::OnlineClient::from_rpc_client_with_config(
            subxt::SubstrateConfig::new(),
            (*rpc_client).clone(),
        )
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
