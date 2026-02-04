use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subxt::error::OnlineClientAtBlockError;
use subxt_rpcs::client::rpc_params;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetSpecError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[source] Box<OnlineClientAtBlockError>),

    #[error("Failed to get runtime version")]
    RuntimeVersionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system properties")]
    SystemPropertiesFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system chain type")]
    SystemChainTypeFailed(#[source] subxt_rpcs::Error),

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),
}

impl IntoResponse for GetSpecError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetSpecError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            GetSpecError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetSpecError::ClientAtBlockFailed(err) => {
                if utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Service temporarily unavailable".to_string(),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }
            // Handle RPC errors with appropriate status codes
            GetSpecError::RuntimeVersionFailed(err)
            | GetSpecError::SystemPropertiesFailed(err)
            | GetSpecError::SystemChainTypeFailed(err) => utils::rpc_error_to_status(err),
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

/// Transform chain type to lowercase object format
/// - String "Live" -> {"live": null}
/// - Object {"Live": null} -> {"live": null}
fn transform_chain_type(chain_type: Value) -> Value {
    match chain_type {
        Value::String(s) => {
            let mut map = serde_json::Map::new();
            map.insert(s.to_lowercase(), Value::Null);
            Value::Object(map)
        }
        Value::Object(map) => {
            let transformed: serde_json::Map<String, Value> = map
                .into_iter()
                .map(|(k, v)| (k.to_lowercase(), v))
                .collect();
            Value::Object(transformed)
        }
        other => other,
    }
}

/// Transform system properties to match expected format:
/// - ss58Format: number -> string
/// - tokenDecimals: number or array -> array of strings
/// - tokenSymbol: string or array -> array of strings
/// - isEthereum: add if missing (default false)
fn transform_properties(properties: Value) -> Value {
    let mut result = serde_json::Map::new();

    if let Value::Object(props) = properties {
        // Transform ss58Format to string
        if let Some(ss58) = props.get("ss58Format") {
            let ss58_str = match ss58 {
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                _ => "0".to_string(),
            };
            result.insert("ss58Format".to_string(), Value::String(ss58_str));
        }

        // Transform tokenDecimals to array of strings
        if let Some(decimals) = props.get("tokenDecimals") {
            let decimals_arr = match decimals {
                Value::Number(n) => vec![Value::String(n.to_string())],
                Value::Array(arr) => arr
                    .iter()
                    .map(|v| match v {
                        Value::Number(n) => Value::String(n.to_string()),
                        Value::String(s) => Value::String(s.clone()),
                        _ => Value::String("0".to_string()),
                    })
                    .collect(),
                _ => vec![],
            };
            result.insert("tokenDecimals".to_string(), Value::Array(decimals_arr));
        }

        // Transform tokenSymbol to array of strings
        if let Some(symbol) = props.get("tokenSymbol") {
            let symbol_arr = match symbol {
                Value::String(s) => vec![Value::String(s.clone())],
                Value::Array(arr) => arr
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => Value::String(s.clone()),
                        _ => Value::String("".to_string()),
                    })
                    .collect(),
                _ => vec![],
            };
            result.insert("tokenSymbol".to_string(), Value::Array(symbol_arr));
        }

        // Add isEthereum (default false if not present)
        let is_ethereum = props
            .get("isEthereum")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        result.insert("isEthereum".to_string(), Value::Bool(is_ethereum));
    }

    Value::Object(result)
}

pub async fn runtime_spec(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<RuntimeSpecResponse>, GetSpecError> {
    // Create client at the specified block - saves RPC calls by letting subxt
    // resolve hash<->number internally
    let client_at_block = match params.at {
        None => {
            // Use current finalized block
            state
                .client
                .at_current_block()
                .await
                .map_err(|e| GetSpecError::ClientAtBlockFailed(Box::new(e)))?
        }
        Some(ref at_str) => {
            let block_id = at_str.parse::<crate::utils::BlockId>()?;
            match block_id {
                crate::utils::BlockId::Hash(hash) => state.client.at_block(hash).await,
                crate::utils::BlockId::Number(number) => state.client.at_block(number).await,
            }
            .map_err(|e| GetSpecError::ClientAtBlockFailed(Box::new(e)))?
        }
    };

    // Extract hash and number from the resolved client
    let block_hash_str = format!("{:#x}", client_at_block.block_hash());
    let block_height = client_at_block.block_number().to_string();

    // Execute RPC calls in parallel
    // TODO: Once subxt-rpcs v0.50.0 is released, replace the direct RPC request
    // with `state.legacy_rpc.system_chain_type()` for type-safe access.
    let (runtime_version_result, properties_result, chain_type_result) = tokio::join!(
        state
            .legacy_rpc
            .state_get_runtime_version(Some(client_at_block.block_hash())),
        state.legacy_rpc.system_properties(),
        state
            .rpc_client
            .request::<Value>("system_chainType", rpc_params![]),
    );

    let runtime_version = runtime_version_result.map_err(GetSpecError::RuntimeVersionFailed)?;
    let properties = properties_result.map_err(GetSpecError::SystemPropertiesFailed)?;
    let chain_type = chain_type_result.map_err(GetSpecError::SystemChainTypeFailed)?;

    // Extract fields from RuntimeVersion struct
    let spec_name = runtime_version
        .other
        .get("specName")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let authoring_version = runtime_version
        .other
        .get("authoringVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let impl_version = runtime_version
        .other
        .get("implVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let spec_version = runtime_version.spec_version.to_string();
    let transaction_version = runtime_version.transaction_version.to_string();

    let response = RuntimeSpecResponse {
        at: BlockInfo {
            hash: block_hash_str,
            height: block_height,
        },
        authoring_version,
        chain_type: transform_chain_type(chain_type),
        impl_version,
        spec_name,
        spec_version,
        transaction_version,
        properties: transform_properties(
            serde_json::to_value(properties).unwrap_or(serde_json::json!({})),
        ),
    };

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use crate::test_fixtures::{TEST_BLOCK_HASH, TEST_BLOCK_NUMBER, mock_rpc_client_builder};
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
            relay_rpc_client: None,
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: crate::routes::RouteRegistry::new(),
            relay_chain_rpc: None,
            lazy_relay_rpc: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    #[tokio::test]
    async fn test_runtime_spec_at_finalized() {
        // Use test fixtures builder and add runtime spec handlers
        let mock_client = mock_rpc_client_builder()
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
            .method_handler("system_chainType", async |_params| MockJson("Live"))
            .build();

        let state = create_test_state_with_mock(mock_client).await;

        let params = AtBlockParam { at: None };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.height, TEST_BLOCK_NUMBER.to_string());
        assert_eq!(response.spec_name, "polkadot");
        assert_eq!(response.spec_version, "9430");
        assert_eq!(response.transaction_version, "24");
        assert_eq!(response.chain_type, json!({ "live": null }));
        assert_eq!(
            response.properties,
            json!({
                "ss58Format": "0",
                "tokenDecimals": ["10"],
                "tokenSymbol": ["DOT"],
                "isEthereum": false
            })
        );
    }

    #[tokio::test]
    async fn test_runtime_spec_at_specific_hash() {
        // Use test fixtures builder and add runtime spec handlers
        let mock_client = mock_rpc_client_builder()
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
            .method_handler("system_chainType", async |_params| MockJson("Live"))
            .build();

        let state = create_test_state_with_mock(mock_client).await;

        let params = AtBlockParam {
            at: Some(TEST_BLOCK_HASH.to_string()),
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.hash, TEST_BLOCK_HASH);
        assert_eq!(response.at.height, TEST_BLOCK_NUMBER.to_string());
        assert_eq!(response.spec_name, "kusama");
        assert_eq!(response.authoring_version, "2");
        assert_eq!(response.chain_type, json!({ "live": null }));
    }

    #[tokio::test]
    async fn test_runtime_spec_at_specific_number() {
        // Use test fixtures builder and add runtime spec handlers
        let mock_client = mock_rpc_client_builder()
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
            .method_handler("system_chainType", async |_params| MockJson("Development"))
            .build();

        let state = create_test_state_with_mock(mock_client).await;

        let params = AtBlockParam {
            at: Some(TEST_BLOCK_NUMBER.to_string()),
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.hash, TEST_BLOCK_HASH);
        assert_eq!(response.at.height, TEST_BLOCK_NUMBER.to_string());
        assert_eq!(response.spec_name, "westend");
        assert_eq!(response.chain_type, json!({ "development": null }));
    }

    #[tokio::test]
    async fn test_runtime_spec_invalid_block_param() {
        // Use test fixtures for proper OnlineClient initialization
        let mock_client = mock_rpc_client_builder().build();
        let state = create_test_state_with_mock(mock_client).await;

        let params = AtBlockParam {
            at: Some("invalid_block".to_string()),
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetSpecError::InvalidBlockParam(_)
        ));
    }

    #[tokio::test]
    #[ignore] // Cannot test block-not-found with mock because OnlineClient init also calls chain_getBlockHash
    async fn test_runtime_spec_block_not_found() {
        let mock_client = MockRpcClient::builder()
            .method_handler("rpc_methods", async |_params| {
                MockJson(json!({ "methods": [] }))
            })
            .method_handler("chain_getBlockHash", async |_params| {
                // Return null for block not found
                MockJson(json!(null))
            })
            .build();

        let state = create_test_state_with_mock(mock_client).await;

        let params = AtBlockParam {
            at: Some("999999".to_string()),
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetSpecError::ClientAtBlockFailed(_)
        ));
    }
}
