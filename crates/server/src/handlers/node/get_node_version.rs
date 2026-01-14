use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt_rpcs::client::rpc_params;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetNodeVersionError {
    #[error("Failed to get runtime version")]
    RuntimeVersionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system chain")]
    SystemChainFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system version")]
    SystemVersionFailed(#[source] subxt_rpcs::Error),
}

impl IntoResponse for GetNodeVersionError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetNodeVersionError::RuntimeVersionFailed(err)
            | GetNodeVersionError::SystemChainFailed(err)
            | GetNodeVersionError::SystemVersionFailed(err) => utils::rpc_error_to_status(err),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeVersionResponse {
    pub client_version: String,
    pub client_impl_name: String,
    pub chain: String,
}

/// Handler for GET /node/version
///
/// Returns the node's version information including client version, implementation name, and chain name.
pub async fn get_node_version(
    State(state): State<AppState>,
) -> Result<Json<NodeVersionResponse>, GetNodeVersionError> {
    let (runtime_version_result, chain_result, version_result) = tokio::join!(
        state.legacy_rpc.state_get_runtime_version(None),
        state
            .rpc_client
            .request::<String>("system_chain", rpc_params![]),
        state
            .rpc_client
            .request::<String>("system_version", rpc_params![]),
    );

    let runtime_version =
        runtime_version_result.map_err(GetNodeVersionError::RuntimeVersionFailed)?;
    let chain = chain_result.map_err(GetNodeVersionError::SystemChainFailed)?;
    let client_version = version_result.map_err(GetNodeVersionError::SystemVersionFailed)?;

    let client_impl_name = runtime_version
        .other
        .get("implName")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(Json(NodeVersionResponse {
        client_version,
        client_impl_name,
        chain,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use axum::extract::State;
    use config::SidecarConfig;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

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
    async fn test_get_node_version_success() {
        let mock_client = MockRpcClient::builder()
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(serde_json::json!({
                    "specName": "asset-hub-polkadot",
                    "implName": "asset-hub-polkadot",
                    "authoringVersion": 0,
                    "specVersion": 1003000,
                    "implVersion": 0,
                    "apis": [],
                    "transactionVersion": 26,
                    "stateVersion": 1
                }))
            })
            .method_handler("system_chain", async |_params| {
                MockJson("Polkadot Asset Hub".to_string())
            })
            .method_handler("system_version", async |_params| {
                MockJson("1.16.0-xyz9876".to_string())
            })
            .build();

        let state = create_test_state_with_mock(mock_client);
        let result = get_node_version(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.chain, "Polkadot Asset Hub");
        assert_eq!(response.client_version, "1.16.0-xyz9876");
        assert_eq!(response.client_impl_name, "asset-hub-polkadot");
    }

    #[tokio::test]
    async fn test_get_node_version_missing_impl_name() {
        let mock_client = MockRpcClient::builder()
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(serde_json::json!({
                    "specName": "asset-hub-westend",
                    "authoringVersion": 0,
                    "specVersion": 1000000,
                    "implVersion": 0,
                    "apis": [],
                    "transactionVersion": 26,
                    "stateVersion": 1
                }))
            })
            .method_handler("system_chain", async |_params| {
                MockJson("Westend Asset Hub".to_string())
            })
            .method_handler("system_version", async |_params| {
                MockJson("1.0.0".to_string())
            })
            .build();

        let state = create_test_state_with_mock(mock_client);
        let result = get_node_version(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.chain, "Westend Asset Hub");
        assert_eq!(response.client_version, "1.0.0");
        assert_eq!(response.client_impl_name, "unknown");
    }
}
