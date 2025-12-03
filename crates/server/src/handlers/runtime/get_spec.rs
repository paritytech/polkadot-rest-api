use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subxt_rpcs::client::rpc_params;
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

    #[error("Failed to get system chain type")]
    SystemChainTypeFailed(#[source] subxt_rpcs::Error),
}

impl IntoResponse for GetSpecError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetSpecError::InvalidBlockParam(_) | GetSpecError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
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
    // Parse the block identifier in the handler (sync)
    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    // Resolve the block (async)
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let block_hash_str = resolved_block.hash;
    let block_height = resolved_block.number.to_string();

    // Execute RPC calls in parallel
    // TODO: Once subxt-rpcs v0.50.0 is released, replace the direct RPC request
    // with `state.legacy_rpc.system_chain_type()` for type-safe access.
    let (runtime_version_result, properties_result, chain_type_result) = tokio::join!(
        state.get_runtime_version_at_hash(&block_hash_str),
        state.legacy_rpc.system_properties(),
        state
            .rpc_client
            .request::<Value>("system_chainType", rpc_params![]),
    );

    let runtime_version = runtime_version_result.map_err(GetSpecError::RuntimeVersionFailed)?;
    let properties = properties_result.map_err(GetSpecError::SystemPropertiesFailed)?;
    let chain_type = chain_type_result.map_err(GetSpecError::SystemChainTypeFailed)?;

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
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: config::ChainConfig::default(),
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
            .method_handler("system_chainType", async |_params| MockJson("Live"))
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam { at: None };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.height, "42");
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
            .method_handler("system_chainType", async |_params| MockJson("Live"))
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some(test_hash.to_string()),
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.hash, test_hash);
        assert_eq!(response.at.height, "100");
        assert_eq!(response.spec_name, "kusama");
        assert_eq!(response.authoring_version, "2");
        assert_eq!(response.chain_type, json!({ "live": null }));
    }

    #[tokio::test]
    async fn test_runtime_spec_at_specific_number() {
        let test_number = "200";
        let expected_hash = "0x9876543210987654321098765432109876543210987654321098765432109876";

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
            .method_handler("system_chainType", async |_params| MockJson("Development"))
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some(test_number.to_string()),
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.hash, expected_hash);
        assert_eq!(response.at.height, test_number);
        assert_eq!(response.spec_name, "westend");
        assert_eq!(response.chain_type, json!({ "development": null }));
    }

    #[tokio::test]
    async fn test_runtime_spec_invalid_block_param() {
        let mock_client = MockRpcClient::builder().build();
        let state = create_test_state_with_mock(mock_client);

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
    async fn test_runtime_spec_block_not_found() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson(serde_json::Value::Null) // Block doesn't exist
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some("999999".to_string()),
        };
        let result = runtime_spec(State(state), axum::extract::Query(params)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetSpecError::BlockResolveFailed(_)
        ));
    }
}
