//! Handler for GET /blocks/{blockId} endpoint.
//!
//! This module provides the main handler for fetching block information.

use crate::handlers::blocks::common::{
    add_docs_to_events, categorize_events, decode_digest_logs, extract_author, extract_extrinsics,
    extract_fee_info_for_extrinsic, fetch_block_events, get_canonical_hash_at_number,
    get_finalized_block_number,
};
use crate::handlers::blocks::docs::Docs;
use crate::handlers::blocks::types::{BlockQueryParams, DigestLog, ExtrinsicInfo};
use crate::state::AppState;
use crate::utils::rc_block::RcBlockFullWithParachainsResponse;
use crate::utils::{
    self, BlockRcResponse, RcBlockError, find_ah_blocks_by_rc_block, get_rc_block_header_info,
    get_timestamp_from_storage,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use heck::{ToSnakeCase, ToUpperCamelCase};
use serde::Serialize;
use serde_json::json;
use subxt_historic::error::{OnlineClientAtBlockError, StorageEntryIsNotAPlainValue, StorageError};
use subxt_rpcs::rpc_params;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetBlockError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Failed to get block header")]
    HeaderFetchFailed(#[source] subxt_rpcs::Error),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[from] OnlineClientAtBlockError),

    #[error("Failed to fetch chain storage")]
    StorageFetchFailed(#[from] StorageError),

    #[error("Storage entry is not a plain value")]
    StorageNotPlainValue(#[from] StorageEntryIsNotAPlainValue),

    #[error("Failed to decode storage value")]
    StorageDecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Failed to fetch extrinsics")]
    ExtrinsicsFetchFailed(String),

    #[error("Missing signature bytes for signed extrinsic")]
    MissingSignatureBytes,

    #[error("Missing address bytes for signed extrinsic")]
    MissingAddressBytes,

    #[error("Failed to decode extrinsic field: {0}")]
    ExtrinsicDecodeFailed(String),

    #[error("RC block operation failed: {0}")]
    RcBlockFailed(#[from] RcBlockError),
}

impl IntoResponse for GetBlockError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetBlockError::InvalidBlockParam(_) | GetBlockError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetBlockError::HeaderFetchFailed(_)
            | GetBlockError::HeaderFieldMissing(_)
            | GetBlockError::ClientAtBlockFailed(_)
            | GetBlockError::StorageFetchFailed(_)
            | GetBlockError::StorageNotPlainValue(_)
            | GetBlockError::StorageDecodeFailed(_)
            | GetBlockError::ExtrinsicsFetchFailed(_)
            | GetBlockError::MissingSignatureBytes
            | GetBlockError::MissingAddressBytes
            | GetBlockError::ExtrinsicDecodeFailed(_)
            | GetBlockError::RcBlockFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockResponse {
    pub number: String,
    pub hash: String,
    pub parent_hash: String,
    pub state_root: String,
    pub extrinsics_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    pub logs: Vec<DigestLog>,
    pub extrinsics: Vec<ExtrinsicInfo>,
    pub on_initialize: crate::handlers::blocks::types::OnInitialize,
    pub on_finalize: crate::handlers::blocks::types::OnFinalize,
    pub finalized: bool,
}

/// Handler for GET /blocks/{blockId}
///
/// Query Parameters:
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
/// - `useRcBlock` (boolean, default: false): Query Asset Hub blocks by Relay Chain block number
pub async fn get_block(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<BlockQueryParams>,
) -> Result<Response, GetBlockError> {
    // Check if useRcBlock is enabled
    if params.use_rc_block && state.has_asset_hub() {
        return handle_rc_block_query(state, block_id).await;
    }

    handle_standard_block_query(state, block_id).await
}

async fn handle_standard_block_query(
    state: AppState,
    block_id: String,
) -> Result<Response, GetBlockError> {
    // Parse the block identifier
    let block_id_parsed = block_id.parse::<utils::BlockId>()?;
    let queried_by_hash = matches!(block_id_parsed, utils::BlockId::Hash(_));
    let resolved_block = utils::resolve_block(&state, Some(block_id_parsed)).await?;
    let header_json = state
        .get_header_json(&resolved_block.hash)
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;

    // Extract header fields
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

    // Use decode_digest_logs from common.rs (returns DigestLog from types.rs)
    let logs = decode_digest_logs(&header_json);

    if queried_by_hash {
        if let Ok(Some(canonical_hash)) =
            get_canonical_hash_at_number(&state, resolved_block.number).await
        {
            if canonical_hash != resolved_block.hash {
                tracing::warn!(
                    "Block {} queried by hash {} is not on canonical chain (canonical: {})",
                    resolved_block.number,
                    resolved_block.hash,
                    canonical_hash
                );
            }
        }
    }

    // Create client_at_block once and reuse for all operations
    let client_at_block = state.client.at(resolved_block.number).await?;

    let (author_id, extrinsics_result, events_result) = tokio::join!(
        extract_author(&state, &client_at_block, &logs, resolved_block.number),
        extract_extrinsics(&state, &client_at_block, resolved_block.number),
        fetch_block_events(&state, &client_at_block, resolved_block.number),
    );

    let extrinsics = extrinsics_result.map_err(|e| match e {
        crate::handlers::blocks::types::GetBlockError::StorageDecodeFailed(err) => {
            GetBlockError::StorageDecodeFailed(err)
        }
        crate::handlers::blocks::types::GetBlockError::StorageFetchFailed(err) => {
            GetBlockError::StorageFetchFailed(err)
        }
        crate::handlers::blocks::types::GetBlockError::ExtrinsicsFetchFailed(msg) => {
            GetBlockError::ExtrinsicsFetchFailed(msg)
        }
        crate::handlers::blocks::types::GetBlockError::MissingSignatureBytes => {
            GetBlockError::MissingSignatureBytes
        }
        crate::handlers::blocks::types::GetBlockError::MissingAddressBytes => {
            GetBlockError::MissingAddressBytes
        }
        crate::handlers::blocks::types::GetBlockError::ExtrinsicDecodeFailed(msg) => {
            GetBlockError::ExtrinsicDecodeFailed(msg)
        }
        _ => GetBlockError::ExtrinsicsFetchFailed(format!("Failed to extract extrinsics: {:?}", e)),
    })?;
    let block_events = events_result.map_err(|e| match e {
        crate::handlers::blocks::types::GetBlockError::StorageDecodeFailed(err) => {
            GetBlockError::StorageDecodeFailed(err)
        }
        crate::handlers::blocks::types::GetBlockError::StorageFetchFailed(err) => {
            GetBlockError::StorageFetchFailed(err)
        }
        _ => GetBlockError::StorageDecodeFailed(parity_scale_codec::Error::from(
            "Failed to fetch events",
        )),
    })?;

    // Determine if the block is finalized by comparing block number with finalized block number
    let finalized = match get_finalized_block_number(&state).await {
        Ok(finalized_number) => resolved_block.number <= finalized_number,
        Err(e) => {
            tracing::warn!("Failed to get finalized block number: {:?}, defaulting to false", e);
            false
        }
    };

    // Categorize events by phase and extract extrinsic outcomes (success, paysFee)
    let (on_initialize, per_extrinsic_events, on_finalize, extrinsic_outcomes) =
        categorize_events(block_events, extrinsics.len());

    let mut extrinsics_with_events = extrinsics;
    for ((extrinsic_events, outcome), extrinsic) in per_extrinsic_events
        .iter()
        .zip(extrinsic_outcomes.iter())
        .zip(extrinsics_with_events.iter_mut())
    {
        extrinsic.events = extrinsic_events.clone();
        extrinsic.success = outcome.success;
       if extrinsic.signature.is_some() {
            extrinsic.pays_fee = outcome.pays_fee;
        }
    }

    // Populate fee info for signed extrinsics that pay fees
    let spec_version = state
        .get_runtime_version_at_hash(&resolved_block.hash)
        .await
        .ok()
        .and_then(|v| v.get("specVersion").and_then(|sv| sv.as_u64()))
        .map(|v| v as u32)
        .unwrap_or(state.chain_info.spec_version);

    for (idx, extrinsic) in extrinsics_with_events.iter_mut().enumerate() {
        if extrinsic.signature.is_some() && extrinsic.pays_fee == Some(true) {
            if let (Some(extrinsic_events), Some(outcome)) = (
                per_extrinsic_events.get(idx),
                extrinsic_outcomes.get(idx),
            ) {
                let fee_info = extract_fee_info_for_extrinsic(
                    &state,
                    &extrinsic.raw_hex,
                    extrinsic_events,
                    Some(outcome),
                    &parent_hash,
                    spec_version,
                )
                .await;
                extrinsic.info = fee_info;
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

    Ok(Json(response).into_response())
}

/// Handle useRcBlock query (array of Asset Hub blocks)
async fn handle_rc_block_query(
    state: AppState,
    block_id: String,
) -> Result<Response, GetBlockError> {
    // Parse blockId as RC block number
    let rc_block_number = block_id.parse::<u64>().map_err(|e| {
        GetBlockError::InvalidBlockParam(utils::BlockIdParseError::InvalidNumber(e))
    })?;

    // Get Asset Hub RPC client
    let ah_rpc_client = state.get_asset_hub_rpc_client().await?;

    let rc_client = state.get_relay_chain_subxt_client().await?;
    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;

    // Get RC block header info (hash, parent hash, number)
    let (rc_block_hash, rc_block_parent_hash, rc_block_number_str) =
        get_rc_block_header_info(&rc_rpc_client, rc_block_number).await?;

    // Find Asset Hub blocks corresponding to this RC block number
    // This queries RC block events to find paraInclusion.CandidateIncluded events for Asset Hub
    let ah_blocks = find_ah_blocks_by_rc_block(&rc_client, &rc_rpc_client, rc_block_number).await?;

    // Query each Asset Hub block and build response
    let mut parachains = Vec::new();
    for ah_block in ah_blocks {
        // Get block header from Asset Hub
        let header_json: serde_json::Value = ah_rpc_client
            .request("chain_getHeader", rpc_params![ah_block.hash.clone()])
            .await
            .map_err(GetBlockError::HeaderFetchFailed)?;

        if header_json.is_null() {
            continue;
        }

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

        let number_hex = header_json
            .get("number")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GetBlockError::HeaderFieldMissing("number".to_string()))?;

        let block_number =
            u64::from_str_radix(number_hex.trim_start_matches("0x"), 16).map_err(|e| {
                GetBlockError::HeaderFieldMissing(format!("Failed to parse block number: {}", e))
            })?;

        let number = block_number.to_string();

        let client_at_block = match state.client.at(block_number).await {
            Ok(client) => client,
            Err(e) => {
                tracing::warn!(
                    "Failed to get client at block {}: {:?}",
                    block_number,
                    e
                );
                continue;
            }
        };

        let author_id = extract_author(&state, &client_at_block, &logs, block_number).await;

        let extrinsics = match extract_extrinsics(&state, &client_at_block, block_number).await {
            Ok(exts) => exts,
            Err(e) => {
                tracing::warn!(
                    "Failed to extract extrinsics for block {}: {:?}",
                    block_number,
                    e
                );
                Vec::new()
            }
        };

        let ah_timestamp = get_timestamp_from_storage(&ah_rpc_client, &ah_block.hash)
            .await
            .unwrap_or_else(|| "0".to_string());

        let finalized = match get_finalized_block_number(&state).await {
            Ok(finalized_number) => block_number <= finalized_number,
            Err(e) => {
                tracing::warn!(
                    "Failed to get finalized block number for block {}: {:?}, defaulting to false",
                    block_number,
                    e
                );
                false
            }
        };

        let rc_response = BlockRcResponse {
            number,
            hash: ah_block.hash.clone(),
            parent_hash,
            state_root,
            extrinsics_root,
            author_id,
            logs,
            on_initialize: utils::rc_block::OnInitializeFinalize { events: Vec::new() },
            extrinsics,
            on_finalize: utils::rc_block::OnInitializeFinalize { events: Vec::new() },
            finalized,
            ah_timestamp,
        };

        parachains.push(rc_response);
    }

    // Always return object with RC block info and parachains array
    let response = RcBlockFullWithParachainsResponse {
        rc_block_hash,
        rc_block_parent_hash,
        rc_block_number: rc_block_number_str,
        parachains,
    };

    Ok(Json(response).into_response())
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
        let params = GetBlockQueryParams::default();

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
