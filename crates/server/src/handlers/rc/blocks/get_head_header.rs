//! Handler for GET /rc/blocks/head/header
//!
//! Returns the header of the latest relay chain block.

use crate::handlers::blocks::common::{HeaderParseError, parse_header_fields};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt_rpcs::rpc_params;
use thiserror::Error;

/// Query parameters for /rc/blocks/head/header endpoint
#[derive(Debug, Deserialize)]
pub struct RcBlockHeadHeaderQueryParams {
    /// When true (default), query finalized head. When false, query canonical head.
    #[serde(default = "default_finalized")]
    pub finalized: bool,
}

fn default_finalized() -> bool {
    true
}

/// Relay chain block header response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlockHeaderResponse {
    pub parent_hash: String,
    pub number: String,
    pub state_root: String,
    pub extrinsics_root: String,
    pub digest: serde_json::Value,
}

/// Error types for /rc/blocks/head/header endpoint
#[derive(Debug, Error)]
pub enum GetRcBlockHeadHeaderError {
    #[error("Failed to get block header")]
    HeaderFetchFailed(#[source] subxt_rpcs::Error),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Relay chain API is not configured. Please set SAS_RELAY_CHAIN_URL")]
    RelayChainNotConfigured,
}

impl From<HeaderParseError> for GetRcBlockHeadHeaderError {
    fn from(err: HeaderParseError) -> Self {
        match err {
            HeaderParseError::FieldMissing(field) => {
                GetRcBlockHeadHeaderError::HeaderFieldMissing(field)
            }
        }
    }
}

impl IntoResponse for GetRcBlockHeadHeaderError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcBlockHeadHeaderError::RelayChainNotConfigured => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcBlockHeadHeaderError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcBlockHeadHeaderError::HeaderFetchFailed(err) => utils::rpc_error_to_status(err),
            GetRcBlockHeadHeaderError::HeaderFieldMissing(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Handler for GET /rc/blocks/head/header
///
/// Returns the header of the latest relay chain block.
///
/// Query Parameters:
/// - `finalized` (boolean, default: true): When true, returns finalized head. When false, returns canonical head.
pub async fn get_rc_blocks_head_header(
    State(state): State<AppState>,
    Query(params): Query<RcBlockHeadHeaderQueryParams>,
) -> Result<Response, GetRcBlockHeadHeaderError> {
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(GetRcBlockHeadHeaderError::RelayChainNotConfigured)?;
    let relay_legacy_rpc = state
        .get_relay_chain_rpc()
        .ok_or(GetRcBlockHeadHeaderError::RelayChainNotConfigured)?;

    // Fetch header JSON from relay chain
    let header_json = if params.finalized {
        let finalized_hash = relay_legacy_rpc
            .chain_get_finalized_head()
            .await
            .map_err(GetRcBlockHeadHeaderError::HeaderFetchFailed)?;
        let hash_str = format!("{:#x}", finalized_hash);
        relay_rpc_client
            .request::<serde_json::Value>("chain_getHeader", rpc_params![hash_str])
            .await
            .map_err(GetRcBlockHeadHeaderError::HeaderFetchFailed)?
    } else {
        relay_rpc_client
            .request::<serde_json::Value>("chain_getHeader", rpc_params![])
            .await
            .map_err(GetRcBlockHeadHeaderError::HeaderFetchFailed)?
    };

    let parsed = parse_header_fields(&header_json)?;

    let response = RcBlockHeaderResponse {
        parent_hash: parsed.parent_hash,
        number: parsed.number.to_string(),
        state_root: parsed.state_root,
        extrinsics_root: parsed.extrinsics_root,
        digest: json!({
            "logs": parsed.digest_logs
        }),
    };

    Ok(Json(response).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::RouteRegistry;
    use crate::state::{AppState, ChainInfo};
    use crate::test_fixtures::mock_rpc_client_builder;
    use axum::extract::State;
    use config::SidecarConfig;
    use std::sync::Arc;
    use subxt_rpcs::LegacyRpcMethods;
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    const TEST_BLOCK_HASH: &str =
        "0xd39ee2fcfc7b4da491fa056d4675f3af38cc29205397c2449749dbced57712b9";
    const TEST_PARENT_HASH: &str =
        "0xb5531541d3c407569749190350c19784baee799e1e4b9ea52471e75150cd3ec1";
    const TEST_STATE_ROOT: &str =
        "0xa50e5b59f2978c4c6b3c5a54f905711d323741e0b512c8280835c75e8e9afb43";
    const TEST_EXTRINSICS_ROOT: &str =
        "0xefbdd47ab4826ccd29911e04f8b93df56ef43241c3c75f610f0171152a48c6b1";
    const TEST_BLOCK_NUMBER: u64 = 29639318;

    async fn create_test_state_with_relay_mock(relay_mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let primary_mock = mock_rpc_client_builder().build();
        let rpc_client = Arc::new(RpcClient::new(primary_mock));
        let relay_rpc_client = Arc::new(RpcClient::new(relay_mock_client));
        let legacy_rpc = Arc::new(LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = ChainInfo {
            chain_type: config::ChainType::AssetHub,
            spec_name: "statemint".to_string(),
            spec_version: 1,
            ss58_prefix: 0,
        };

        let client = subxt::OnlineClient::from_rpc_client((*rpc_client).clone())
            .await
            .expect("Failed to create test OnlineClient");

        AppState {
            config,
            client: Arc::new(client),
            legacy_rpc,
            rpc_client,
            chain_info,
            relay_client: None,
            relay_rpc_client: Some(relay_rpc_client.clone()),
            relay_chain_rpc: Some(Arc::new(LegacyRpcMethods::new((*relay_rpc_client).clone()))),
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: RouteRegistry::new(),
        }
    }

    #[test]
    fn test_response_has_correct_structure() {
        // Verify the response matches the expected fixture structure
        let response = RcBlockHeaderResponse {
            parent_hash: TEST_PARENT_HASH.to_string(),
            number: TEST_BLOCK_NUMBER.to_string(),
            state_root: TEST_STATE_ROOT.to_string(),
            extrinsics_root: TEST_EXTRINSICS_ROOT.to_string(),
            digest: json!({
                "logs": [
                    {"preRuntime": ["0x42414245", "0x032d010000"]},
                    {"consensus": ["0x42454546", "0x03c41b16c2"]},
                    {"seal": ["0x42414245", "0x8e3d333ec6"]}
                ]
            }),
        };

        let json = serde_json::to_value(&response).unwrap();

        // Verify all required fields are present
        assert!(json.get("parentHash").is_some());
        assert!(json.get("number").is_some());
        assert!(json.get("stateRoot").is_some());
        assert!(json.get("extrinsicsRoot").is_some());
        assert!(json.get("digest").is_some());

        // Verify field types
        assert!(json["parentHash"].is_string());
        assert!(json["number"].is_string());
        assert!(json["stateRoot"].is_string());
        assert!(json["extrinsicsRoot"].is_string());
        assert!(json["digest"]["logs"].is_array());

        // Verify hash fields start with 0x
        assert!(json["parentHash"].as_str().unwrap().starts_with("0x"));
        assert!(json["stateRoot"].as_str().unwrap().starts_with("0x"));
        assert!(json["extrinsicsRoot"].as_str().unwrap().starts_with("0x"));

        // Verify number is a valid decimal string
        let number_str = json["number"].as_str().unwrap();
        assert!(number_str.parse::<u64>().is_ok());
    }

    #[test]
    fn test_error_responses() {
        let relay_not_configured = GetRcBlockHeadHeaderError::RelayChainNotConfigured;
        let response = relay_not_configured.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let service_unavailable = GetRcBlockHeadHeaderError::ServiceUnavailable("test".to_string());
        let response = service_unavailable.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let missing_field = GetRcBlockHeadHeaderError::HeaderFieldMissing("number".to_string());
        let response = missing_field.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_get_rc_blocks_head_header_finalized_success() {
        let relay_mock = mock_rpc_client_builder()
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson(TEST_BLOCK_HASH)
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": format!("0x{:x}", TEST_BLOCK_NUMBER),
                    "parentHash": TEST_PARENT_HASH,
                    "stateRoot": TEST_STATE_ROOT,
                    "extrinsicsRoot": TEST_EXTRINSICS_ROOT,
                    "digest": {
                        "logs": [
                            "0x0642414245b501032d01000000000000",
                            "0x04424545468403c41b16c2943a",
                            "0x05424142450101"
                        ]
                    }
                }))
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let params = RcBlockHeadHeaderQueryParams { finalized: true };
        let result = get_rc_blocks_head_header(State(state), Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Extract body and verify structure
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["parentHash"], TEST_PARENT_HASH);
        assert_eq!(json["number"], TEST_BLOCK_NUMBER.to_string());
        assert_eq!(json["stateRoot"], TEST_STATE_ROOT);
        assert_eq!(json["extrinsicsRoot"], TEST_EXTRINSICS_ROOT);
        assert!(json["digest"]["logs"].is_array());
    }

    #[tokio::test]
    async fn test_get_rc_blocks_head_header_not_finalized_success() {
        let relay_mock = mock_rpc_client_builder()
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": format!("0x{:x}", TEST_BLOCK_NUMBER + 10),
                    "parentHash": TEST_PARENT_HASH,
                    "stateRoot": TEST_STATE_ROOT,
                    "extrinsicsRoot": TEST_EXTRINSICS_ROOT,
                    "digest": { "logs": [] }
                }))
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let params = RcBlockHeadHeaderQueryParams { finalized: false };
        let result = get_rc_blocks_head_header(State(state), Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Block number should be TEST_BLOCK_NUMBER + 10 (canonical is ahead of finalized)
        assert_eq!(json["number"], (TEST_BLOCK_NUMBER + 10).to_string());
    }

    #[tokio::test]
    async fn test_get_rc_blocks_head_header_with_digest_logs() {
        let relay_mock = mock_rpc_client_builder()
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson(TEST_BLOCK_HASH)
            })
            .method_handler("chain_getHeader", async |_params| {
                // Real digest logs from Polkadot relay chain
                MockJson(json!({
                    "number": "0x1c4f306",
                    "parentHash": TEST_PARENT_HASH,
                    "stateRoot": TEST_STATE_ROOT,
                    "extrinsicsRoot": TEST_EXTRINSICS_ROOT,
                    "digest": {
                        "logs": [
                            "0x0642414245b501032d010000a5429311000000005c64bf9f9bc3ca37a329d015d3df9c307a02d0740eac9683dbe4184924017e7aeb5a504bd02b557c0cf7aabd3574ed11479c9d22e76c73380369863d2e4b8e0322b9d8504bdb28a855ec8b9788708ca01d8766b696b6751ed279532f1192d207",
                            "0x04424545468403c41b16c2943a6d45b1bf0b9a796fa4e4a7f66d75b8538d6882f32f40ebce7c82",
                            "0x05424142450101"
                        ]
                    }
                }))
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let params = RcBlockHeadHeaderQueryParams { finalized: true };
        let result = get_rc_blocks_head_header(State(state), Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Verify digest logs are properly formatted
        let logs = json["digest"]["logs"].as_array().unwrap();
        assert!(!logs.is_empty());

        // Each log should be an object with a single key (preRuntime, consensus, seal, etc.)
        for log in logs {
            assert!(log.is_object());
            let obj = log.as_object().unwrap();
            assert_eq!(obj.len(), 1);
        }
    }
}
