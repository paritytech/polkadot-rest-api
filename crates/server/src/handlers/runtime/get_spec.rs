use crate::state::AppState;
use crate::utils::{
    self, BlockInfo as UtilsBlockInfo, RcBlockError, RcBlockResponse,
    find_ah_blocks_by_rc_block,
};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subxt_rpcs::rpc_params;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetSpecError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Failed to get runtime version")]
    RuntimeVersionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system properties")]
    SystemPropertiesFailed(#[source] subxt_rpcs::Error),

    #[error("RC block operation failed: {0}")]
    RcBlockFailed(#[from] RcBlockError),
}

impl IntoResponse for GetSpecError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetSpecError::InvalidBlockParam(_) | GetSpecError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetSpecError::RcBlockFailed(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct AtBlockParam {
    pub at: Option<String>,

    /// When true, query Asset Hub blocks by Relay Chain block number
    #[serde(default, rename = "useRcBlock")]
    pub use_rc_block: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct BlockInfo {
    pub hash: String,
    pub height: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpecResponse {
    pub at: BlockInfo,
    pub authoring_version: String,
    pub chain_type: Value,
    pub impl_version: String,
    pub spec_name: String,
    pub spec_version: String,
    pub transaction_version: String,
    pub properties: Value,
}

pub async fn runtime_spec(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Response, GetSpecError> {
    // Check if useRcBlock is enabled
    if params.use_rc_block == Some(true) && state.has_asset_hub() {
        return handle_rc_block_runtime_query(state, params).await;
    }

    // Standard behavior: treat at as AH block
    handle_standard_runtime_query(state, params).await
}

/// Handle standard runtime query (single runtime spec response)
async fn handle_standard_runtime_query(
    state: AppState,
    params: AtBlockParam,
) -> Result<Response, GetSpecError> {
    // Parse the block identifier in the handler (sync)
    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    // Resolve the block (async)
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let block_hash_str = resolved_block.hash;
    let block_height = resolved_block.number.to_string();

    let runtime_version = state
        .get_runtime_version_at_hash(&block_hash_str)
        .await
        .map_err(GetSpecError::RuntimeVersionFailed)?;

    let spec_name = runtime_version
        .get("specName")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let authoring_version = runtime_version
        .get("authoringVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let impl_version = runtime_version
        .get("implVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let spec_version = runtime_version
        .get("specVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let transaction_version = runtime_version
        .get("transactionVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let properties = state
        .legacy_rpc
        .system_properties()
        .await
        .map_err(GetSpecError::SystemPropertiesFailed)?;

    // TODO: system_chain_type is not available in LegacyRpcMethods
    // Need to find the correct RPC method or use a different approach
    // For now, default to "live"
    let chain_type = serde_json::json!({
        "live": null
    });

    let response = RuntimeSpecResponse {
        at: BlockInfo {
            hash: block_hash_str,
            height: block_height,
        },
        authoring_version,
        chain_type,
        impl_version,
        spec_name,
        spec_version,
        transaction_version,
        properties: serde_json::to_value(properties).unwrap_or(serde_json::json!({})),
    };

    Ok(Json(response).into_response())
}

/// Handle useRcBlock runtime query (array of Asset Hub runtime specs)
async fn handle_rc_block_runtime_query(
    state: AppState,
    params: AtBlockParam,
) -> Result<Response, GetSpecError> {
    // Parse at parameter as RC block number
    let rc_block_number = params
        .at
        .ok_or_else(|| {
            GetSpecError::InvalidBlockParam(crate::utils::BlockIdParseError::InvalidNumber(
                "0".parse::<u64>().unwrap_err(),
            ))
        })?
        .parse::<u64>()
        .map_err(|e| {
            GetSpecError::InvalidBlockParam(crate::utils::BlockIdParseError::InvalidNumber(e))
        })?;

    // Get Asset Hub RPC client
    let ah_rpc_client = state.get_asset_hub_rpc_client().await?;

    // Get Relay Chain RPC client to get RC block hash and finalized status
    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    
    // Get RC block hash for reference
    let rc_block_hash: Option<String> = rc_rpc_client
        .request("chain_getBlockHash", rpc_params![rc_block_number])
        .await
        .map_err(|e| GetSpecError::RcBlockFailed(RcBlockError::AssetHubQueryFailed(e)))?;
    let _rc_block_hash = rc_block_hash.ok_or_else(|| {
        GetSpecError::RcBlockFailed(RcBlockError::HeaderFieldMissing(
            format!("RC block {} not found", rc_block_number)
        ))
    })?;

    // Get Relay Chain subxt client to query events
    let rc_client = state.get_relay_chain_subxt_client().await?;
    
    // Find Asset Hub blocks corresponding to this RC block number
    // This queries RC block events to find paraInclusion.CandidateIncluded events for Asset Hub
    let ah_blocks = find_ah_blocks_by_rc_block(&rc_client, rc_block_number).await?;

    // If no blocks found, return empty array
    if ah_blocks.is_empty() {
        return Ok(Json::<Vec<RcBlockResponse<RuntimeSpecResponse>>>(vec![]).into_response());
    }

    // Query runtime spec for each Asset Hub block
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        // Get runtime version for this AH block
        let runtime_version: Value = ah_rpc_client
            .request(
                "state_getRuntimeVersion",
                rpc_params![ah_block.hash.clone()],
            )
            .await
            .map_err(GetSpecError::RuntimeVersionFailed)?;

        // Get system properties (using the AH client)
        let ah_legacy_rpc = state.get_asset_hub_legacy_rpc().await?;
        let properties = ah_legacy_rpc
            .system_properties()
            .await
            .map_err(GetSpecError::SystemPropertiesFailed)?;

        // Extract runtime version fields
        let spec_name = runtime_version
            .get("specName")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let authoring_version = runtime_version
            .get("authoringVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .to_string();

        let impl_version = runtime_version
            .get("implVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .to_string();

        let spec_version = runtime_version
            .get("specVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .to_string();

        let transaction_version = runtime_version
            .get("transactionVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .to_string();

        // Build RuntimeSpecResponse
        let data = RuntimeSpecResponse {
            at: BlockInfo {
                hash: ah_block.hash.clone(),
                height: ah_block.number.to_string(),
            },
            authoring_version,
            chain_type: json!({ "live": null }),
            impl_version,
            spec_name,
            spec_version,
            transaction_version,
            properties: serde_json::to_value(properties).unwrap_or(json!({})),
        };

        // Extract timestamp from Asset Hub block
        // TODO: Implement timestamp extraction using storage query
        let ah_timestamp = "0".to_string();

        // Build RcBlockResponse
        let rc_response = RcBlockResponse {
            at: UtilsBlockInfo {
                hash: ah_block.hash,
                height: ah_block.number.to_string(),
            },
            data,
            rc_block_number: rc_block_number.to_string(),
            ah_timestamp,
        };

        results.push(rc_response);
    }

    Ok(Json(results).into_response())
}

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
        }
    }

    #[tokio::test]
    async fn test_runtime_spec_at_finalized() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x2a", // Block 42
                }))
            })
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(json!({
                    "specName": "polkadot",
                    "specVersion": 9430,
                    "implVersion": 0,
                    "authoringVersion": 0,
                    "transactionVersion": 24
                }))
            })
            .method_handler("system_properties", async |_params| {
                MockJson(json!({
                    "ss58Format": 0,
                    "tokenDecimals": [10],
                    "tokenSymbol": ["DOT"]
                }))
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: None,
            use_rc_block: None,
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        // TODO: Update test to handle Response type properly
        // For now, just verify it succeeds
    }

    #[tokio::test]
    async fn test_runtime_spec_at_specific_hash() {
        let test_hash = "0xabcdef1234567890123456789012345678901234567890123456789012345678";

        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64", // Block 100
                }))
            })
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(json!({
                    "specName": "kusama",
                    "specVersion": 9430,
                    "implVersion": 0,
                    "authoringVersion": 2,
                    "transactionVersion": 24
                }))
            })
            .method_handler("system_properties", async |_params| {
                MockJson(json!({
                    "ss58Format": 2,
                    "tokenDecimals": [12],
                    "tokenSymbol": ["KSM"]
                }))
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some(test_hash.to_string()),
            use_rc_block: None,
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        // TODO: Update test to handle Response type properly
    }

    #[tokio::test]
    async fn test_runtime_spec_at_specific_number() {
        let test_number = "200";
        let _expected_hash = "0x9876543210987654321098765432109876543210987654321098765432109876";

        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson("0x9876543210987654321098765432109876543210987654321098765432109876")
            })
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(json!({
                    "specName": "westend",
                    "specVersion": 9430,
                    "implVersion": 0,
                    "authoringVersion": 0,
                    "transactionVersion": 24
                }))
            })
            .method_handler("system_properties", async |_params| {
                MockJson(json!({
                    "ss58Format": 42,
                    "tokenDecimals": [12],
                    "tokenSymbol": ["WND"]
                }))
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some(test_number.to_string()),
            use_rc_block: None,
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        // TODO: Update test to handle Response type properly
    }

    #[tokio::test]
    async fn test_runtime_spec_invalid_block_param() {
        let mock_client = MockRpcClient::builder().build();
        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some("invalid_block".to_string()),
            use_rc_block: None,
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetSpecError::InvalidBlockParam(_)
        ));
    }

    #[tokio::test]
    async fn test_runtime_spec_block_not_found() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson(serde_json::Value::Null) // Block doesn't exist
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some("999999".to_string()),
            use_rc_block: None,
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetSpecError::BlockResolveFailed(_)
        ));
    }
}
