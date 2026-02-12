//! Handler for GET /rc/blocks/{blockId} endpoint.
//!
//! Returns block information for a specific block by height or hash on the relay chain.
//! This endpoint is designed for Asset Hub or parachain endpoints that have a relay chain configured.

use crate::handlers::blocks::common::{BlockBuildContext, build_block_response_generic};
use crate::handlers::blocks::types::{BlockBuildParams, GetBlockError};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

// ================================================================================================
// Query Parameters
// ================================================================================================

/// Query parameters for /rc/blocks/{blockId} endpoint
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlockQueryParams {
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

impl RcBlockQueryParams {
    pub fn to_build_params(&self) -> BlockBuildParams {
        BlockBuildParams {
            event_docs: self.event_docs,
            extrinsic_docs: self.extrinsic_docs,
            no_fees: self.no_fees,
            decoded_xcm_msgs: self.decoded_xcm_msgs,
            para_id: self.para_id,
            use_evm_format: false,
        }
    }
}

// ================================================================================================
// Error Types
// ================================================================================================

/// Error types for /rc/blocks/{blockId} endpoint
#[derive(Debug, Error)]
pub enum GetRcBlockError {
    #[error(
        "Relay chain API is not configured. Please set SAS_SUBSTRATE_MULTI_CHAIN_URL with a relay chain entry"
    )]
    RelayChainNotConfigured,

    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Block processing error: {0}")]
    BlockProcessingError(#[from] GetBlockError),
}

impl IntoResponse for GetRcBlockError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            GetRcBlockError::RelayChainNotConfigured => (StatusCode::BAD_REQUEST, self.to_string()),
            GetRcBlockError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            GetRcBlockError::ClientAtBlockFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            GetRcBlockError::BlockProcessingError(_) => {
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

/// Handler for GET /rc/blocks/{blockId}
///
/// Returns block information for a specific block on the relay chain.
/// This endpoint requires a relay chain to be configured via SAS_SUBSTRATE_MULTI_CHAIN_URL.
///
/// Path Parameters:
/// - `blockId`: Block height (number) or block hash
///
/// Query Parameters:
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
/// - `noFees` (boolean, default: false): Skip fee calculation
/// - `decodedXcmMsgs` (boolean, default: false): Include decoded XCM messages
/// - `paraId` (number, optional): Filter XCM messages by parachain ID
#[utoipa::path(
    get,
    path = "/v1/rc/blocks/{blockId}",
    tag = "rc",
    summary = "RC get block by ID",
    description = "Returns relay chain block information for a given block identifier.",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash"),
        ("eventDocs" = Option<bool>, Query, description = "Include event documentation"),
        ("extrinsicDocs" = Option<bool>, Query, description = "Include extrinsic documentation"),
        ("noFees" = Option<bool>, Query, description = "Skip fee calculation"),
        ("decodedXcmMsgs" = Option<bool>, Query, description = "Include decoded XCM messages"),
        ("paraId" = Option<u32>, Query, description = "Filter XCM messages by parachain ID")
    ),
    responses(
        (status = 200, description = "Relay chain block information", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_block(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<RcBlockQueryParams>,
) -> Result<Response, GetRcBlockError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(GetRcBlockError::RelayChainNotConfigured)?;
    let relay_chain_info = state
        .relay_chain_info
        .as_ref()
        .ok_or(GetRcBlockError::RelayChainNotConfigured)?;

    let block_id_parsed: utils::BlockId = block_id.parse()?;
    let queried_by_hash = matches!(block_id_parsed, utils::BlockId::Hash(_));

    let client_at_block = match &block_id_parsed {
        utils::BlockId::Hash(hash) => relay_client
            .at_block(*hash)
            .await
            .map_err(|e| GetRcBlockError::ClientAtBlockFailed(Box::new(e)))?,
        utils::BlockId::Number(number) => relay_client
            .at_block(*number)
            .await
            .map_err(|e| GetRcBlockError::ClientAtBlockFailed(Box::new(e)))?,
    };

    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    let ctx = BlockBuildContext {
        state: &state,
        client: relay_client,
        ss58_prefix: relay_chain_info.ss58_prefix,
        chain_type: ChainType::Relay,
        spec_name: relay_chain_info.spec_name.clone(),
    };

    let response = build_block_response_generic(
        &ctx,
        &client_at_block,
        &block_hash,
        block_number,
        queried_by_hash,
        &params.to_build_params(),
        true,
    )
    .await?;

    Ok(Json(response).into_response())
}

// ================================================================================================
// Unit Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::blocks::types::{
        BlockResponse, DigestLog, ExtrinsicInfo, MethodInfo, OnFinalize, OnInitialize, XcmMessages,
    };
    use crate::utils::EraInfo;

    fn create_test_block_response() -> BlockResponse {
        BlockResponse {
            number: "29698001".to_string(),
            hash: "0x67ce95c6e0923e3d5638fd89940853b48f03f952bf79706f00f65a24e609a074".to_string(),
            parent_hash: "0x6a5dc0f99ebd62e6b8c949d11ccda2df18f8f1258891263eff83bffdc9253db2"
                .to_string(),
            state_root: "0xce79e23cbf260ae191184c3130ab27d69d0c19efed4df8dc2e57ebaa560e41f1"
                .to_string(),
            extrinsics_root: "0x88f053bffb861277ed565f932f90a6518135436bd554f3172c03f934be74d9b7"
                .to_string(),
            author_id: Some("16hwkvDGzdLLyaZ9CyPfwg85ijEAJUoHKxKSu6oSfDVyZm9j".to_string()),
            logs: vec![DigestLog {
                log_type: "PreRuntime".to_string(),
                index: "6".to_string(),
                value: json!([
                    "0x42414245",
                    "0x034f020000d42894110000000016310ed2257a4e5308248da4b45f94b48e9cfdf0a67bb4a8fec187f797632a20df25ae14ab1769d1f7da5135b3e144d86b66b70f8de541ab53fbec0a6a0e120c6f800a30fdf417ff16de739c57c623c621addee6be951037e0d000b2f6eb1b03"
                ]),
            }],
            on_initialize: OnInitialize { events: vec![] },
            extrinsics: vec![ExtrinsicInfo {
                method: MethodInfo {
                    pallet: "timestamp".to_string(),
                    method: "set".to_string(),
                },
                signature: None,
                nonce: None,
                args: serde_json::Map::from_iter(vec![("now".to_string(), json!("1769534712000"))]),
                tip: None,
                hash: "0x76ccdad5a14aab4061c0fb5331d5da2798695e40b948e28678a0ee2cc08b666a"
                    .to_string(),
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
}
