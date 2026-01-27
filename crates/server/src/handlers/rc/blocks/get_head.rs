//! Handler for GET /rc/blocks/head endpoint.
//!
//! Returns block information for the latest block (head) on the relay chain.
//! This endpoint is designed for Asset Hub or parachain endpoints that have a relay chain configured.

use crate::handlers::blocks::common::{
    add_docs_to_events, convert_digest_items_to_logs, extract_author_with_prefix,
};
use crate::handlers::blocks::decode::XcmDecoder;
use crate::handlers::blocks::docs::Docs;
use crate::handlers::blocks::processing::{
    categorize_events, extract_extrinsics_with_prefix, extract_fee_info_for_extrinsic,
    fetch_block_events_with_prefix,
};
use crate::handlers::blocks::types::{BlockResponse, GetBlockError};
use crate::state::AppState;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use heck::{ToSnakeCase, ToUpperCamelCase};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

// ================================================================================================
// Query Parameters
// ================================================================================================

/// Query parameters for /rc/blocks/head endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlockHeadQueryParams {
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

impl Default for RcBlockHeadQueryParams {
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
// Error Types
// ================================================================================================

/// Error types for /rc/blocks/head endpoint
#[derive(Debug, Error)]
pub enum GetRcBlockHeadError {
    #[error("Relay chain API is not configured. Please set SAS_RELAY_CHAIN_URL")]
    RelayChainNotConfigured,

    #[error("Failed to get finalized head")]
    FinalizedHeadFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get block header: {0}")]
    BlockHeaderFailed(#[source] subxt::error::BlockError),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Block processing error: {0}")]
    BlockProcessingError(#[from] GetBlockError),
}

impl IntoResponse for GetRcBlockHeadError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            GetRcBlockHeadError::RelayChainNotConfigured => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcBlockHeadError::FinalizedHeadFailed(err) => crate::utils::rpc_error_to_status(err),
            GetRcBlockHeadError::BlockHeaderFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            GetRcBlockHeadError::HeaderFieldMissing(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            GetRcBlockHeadError::ClientAtBlockFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            GetRcBlockHeadError::BlockProcessingError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /rc/blocks/head
///
/// Returns block information for the latest block (head) on the relay chain.
/// This endpoint requires a relay chain to be configured via SAS_RELAY_CHAIN_URL.
///
/// Query Parameters:
/// - `finalized` (boolean, default: true): When true, returns finalized head. When false, returns canonical head.
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
/// - `noFees` (boolean, default: false): Skip fee calculation
pub async fn get_rc_blocks_head(
    State(state): State<AppState>,
    Query(params): Query<RcBlockHeadQueryParams>,
) -> Result<Response, GetRcBlockHeadError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(GetRcBlockHeadError::RelayChainNotConfigured)?;
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(GetRcBlockHeadError::RelayChainNotConfigured)?;
    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(GetRcBlockHeadError::RelayChainNotConfigured)?;
    let relay_chain_info = state
        .relay_chain_info
        .as_ref()
        .ok_or(GetRcBlockHeadError::RelayChainNotConfigured)?;

    let ss58_prefix = relay_chain_info.ss58_prefix;

    let (client_at_block, is_finalized) = if params.finalized {
        let finalized_hash = relay_rpc
            .chain_get_finalized_head()
            .await
            .map_err(GetRcBlockHeadError::FinalizedHeadFailed)?;

        let client = relay_client
            .at_block(finalized_hash)
            .await
            .map_err(|e| GetRcBlockHeadError::ClientAtBlockFailed(Box::new(e)))?;

        (client, true)
    } else {
        let (best_hash_result, finalized_hash_result) = tokio::join!(
            relay_rpc.chain_get_block_hash(None),
            relay_rpc.chain_get_finalized_head()
        );

        let best_hash = best_hash_result
            .map_err(GetRcBlockHeadError::FinalizedHeadFailed)?
            .ok_or_else(|| {
                GetRcBlockHeadError::HeaderFieldMissing("best block hash".to_string())
            })?;

        let finalized_hash =
            finalized_hash_result.map_err(GetRcBlockHeadError::FinalizedHeadFailed)?;

        let canonical_client = relay_client
            .at_block(best_hash)
            .await
            .map_err(|e| GetRcBlockHeadError::ClientAtBlockFailed(Box::new(e)))?;

        let finalized_client = relay_client
            .at_block(finalized_hash)
            .await
            .map_err(|e| GetRcBlockHeadError::ClientAtBlockFailed(Box::new(e)))?;

        let is_finalized = canonical_client.block_number() <= finalized_client.block_number();

        (canonical_client, is_finalized)
    };

    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    let header = client_at_block
        .block_header()
        .await
        .map_err(GetRcBlockHeadError::BlockHeaderFailed)?;

    let parent_hash = format!("{:#x}", header.parent_hash);
    let state_root = format!("{:#x}", header.state_root);
    let extrinsics_root = format!("{:#x}", header.extrinsics_root);

    let logs = convert_digest_items_to_logs(&header.digest.logs);

    let (author_id, extrinsics_result, events_result) = tokio::join!(
        extract_author_with_prefix(&client_at_block, &logs, ss58_prefix, block_number),
        extract_extrinsics_with_prefix(&client_at_block, ss58_prefix, block_number),
        fetch_block_events_with_prefix(&client_at_block, ss58_prefix, block_number),
    );

    let extrinsics = extrinsics_result?;
    let block_events = events_result?;

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
            let spec_version = client_at_block.spec_version();

            let fee_futures: Vec<_> = fee_indices
                .iter()
                .map(|&i| {
                    let extrinsic = &extrinsics_with_events[i];
                    extract_fee_info_for_extrinsic(
                        &state,
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
        let decoder = XcmDecoder::new(ChainType::Relay, &extrinsics_with_events, params.para_id);
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

    Ok(Json(response).into_response())
}

// ================================================================================================
// Unit Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_query_params() {
        let params = RcBlockHeadQueryParams::default();
        assert!(params.finalized);
        assert!(!params.event_docs);
        assert!(!params.extrinsic_docs);
        assert!(!params.no_fees);
        assert!(!params.decoded_xcm_msgs);
        assert!(params.para_id.is_none());
    }

    #[test]
    fn test_query_params_deserialization_custom() {
        let json = r#"{"finalized": false, "eventDocs": true, "extrinsicDocs": true, "noFees": true, "decodedXcmMsgs": true, "paraId": 1000}"#;
        let params: RcBlockHeadQueryParams = serde_json::from_str(json).unwrap();
        assert!(!params.finalized);
        assert!(params.event_docs);
        assert!(params.extrinsic_docs);
        assert!(params.no_fees);
        assert!(params.decoded_xcm_msgs);
        assert_eq!(params.para_id, Some(1000));
    }

    #[test]
    fn test_error_responses() {
        let relay_not_configured = GetRcBlockHeadError::RelayChainNotConfigured;
        let response = relay_not_configured.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ========================================================================
    // Response Structure Tests
    // ========================================================================

    use crate::handlers::blocks::types::{
        DigestLog, ExtrinsicInfo, MethodInfo, OnFinalize, OnInitialize, XcmMessages,
    };
    use crate::utils::EraInfo;

    fn create_test_block_response() -> BlockResponse {
        BlockResponse {
            number: "29639318".to_string(),
            hash: "0xd39ee2fcfc7b4da491fa056d4675f3af38cc29205397c2449749dbced57712b9".to_string(),
            parent_hash: "0xb5531541d3c407569749190350c19784baee799e1e4b9ea52471e75150cd3ec1"
                .to_string(),
            state_root: "0xa50e5b59f2978c4c6b3c5a54f905711d323741e0b512c8280835c75e8e9afb43"
                .to_string(),
            extrinsics_root: "0xefbdd47ab4826ccd29911e04f8b93df56ef43241c3c75f610f0171152a48c6b1"
                .to_string(),
            author_id: Some("1zugcag7cJVBtVRnFxv5Qftn7xKGLXqR4VEy4Hzir2u5f5X".to_string()),
            logs: vec![DigestLog {
                log_type: "PreRuntime".to_string(),
                index: "6".to_string(),
                value: json!(["0x42414245", "0x0301000000"]),
            }],
            on_initialize: OnInitialize { events: vec![] },
            extrinsics: vec![ExtrinsicInfo {
                method: MethodInfo {
                    pallet: "timestamp".to_string(),
                    method: "set".to_string(),
                },
                signature: None,
                nonce: None,
                args: serde_json::Map::from_iter(vec![("now".to_string(), json!("1737935148003"))]),
                tip: None,
                hash: "0x1234".to_string(),
                info: serde_json::Map::new(),
                era: EraInfo {
                    immortal_era: Some("0x00".to_string()),
                    mortal_era: None,
                },
                events: vec![],
                success: true,
                pays_fee: None,
                docs: None,
                raw_hex: "0x".to_string(),
            }],
            on_finalize: OnFinalize { events: vec![] },
            finalized: Some(true),
            decoded_xcm_msgs: None,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }
    }

    #[test]
    fn test_response_field_types() {
        let response = create_test_block_response();
        let json = serde_json::to_value(&response).unwrap();

        // Verify field types
        assert!(json["number"].is_string());
        assert!(json["hash"].is_string());
        assert!(json["parentHash"].is_string());
        assert!(json["stateRoot"].is_string());
        assert!(json["extrinsicsRoot"].is_string());
        assert!(json["authorId"].is_string());
        assert!(json["logs"].is_array());
        assert!(json["onInitialize"].is_object());
        assert!(json["extrinsics"].is_array());
        assert!(json["onFinalize"].is_object());
        assert!(json["finalized"].is_boolean());

        // Verify hash fields start with 0x
        assert!(json["hash"].as_str().unwrap().starts_with("0x"));
        assert!(json["parentHash"].as_str().unwrap().starts_with("0x"));
        assert!(json["stateRoot"].as_str().unwrap().starts_with("0x"));
        assert!(json["extrinsicsRoot"].as_str().unwrap().starts_with("0x"));

        // Verify number is a valid decimal string
        let number_str = json["number"].as_str().unwrap();
        assert!(number_str.parse::<u64>().is_ok());
    }

    #[test]
    fn test_response_extrinsic_structure() {
        let response = create_test_block_response();
        let json = serde_json::to_value(&response).unwrap();

        let extrinsics = json["extrinsics"].as_array().unwrap();
        assert!(!extrinsics.is_empty());

        let ext = &extrinsics[0];
        // Verify extrinsic fields
        assert!(ext.get("method").is_some());
        assert!(ext["method"].get("pallet").is_some());
        assert!(ext["method"].get("method").is_some());
        assert!(ext.get("args").is_some());
        assert!(ext.get("hash").is_some());
        assert!(ext.get("info").is_some());
        assert!(ext.get("era").is_some());
        assert!(ext.get("events").is_some());
        assert!(ext.get("success").is_some());

        // Unsigned extrinsic should have null signature
        assert!(ext["signature"].is_null());
    }

    #[test]
    fn test_response_with_xcm_messages() {
        let mut response = create_test_block_response();
        response.decoded_xcm_msgs = Some(XcmMessages {
            horizontal_messages: vec![],
            downward_messages: vec![],
            upward_messages: vec![],
        });

        let json = serde_json::to_value(&response).unwrap();

        // Verify XCM messages structure when present
        assert!(json.get("decodedXcmMsgs").is_some());
        let xcm = &json["decodedXcmMsgs"];
        assert!(xcm.get("horizontalMessages").is_some());
        assert!(xcm.get("downwardMessages").is_some());
        assert!(xcm.get("upwardMessages").is_some());
    }

    #[test]
    fn test_response_finalized_omitted_when_none() {
        let mut response = create_test_block_response();
        response.finalized = None;

        let json = serde_json::to_value(&response).unwrap();

        // finalized should be omitted when None
        assert!(json.get("finalized").is_none());
    }
}
