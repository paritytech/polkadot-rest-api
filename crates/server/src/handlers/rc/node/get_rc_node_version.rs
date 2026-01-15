use crate::handlers::node::NodeVersionResponse;
use crate::handlers::node::common::{FetchError, fetch_node_version};
use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use config::ChainType;
use serde_json::json;
use std::sync::Arc;
use subxt_historic::SubstrateConfig;
use subxt_rpcs::{LegacyRpcMethods, RpcClient};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetRcNodeVersionError {
    #[error("Relay chain connection not available")]
    RelayChainNotAvailable,

    #[error("Failed to connect to relay chain from multi-chain URLs")]
    MultiChainConnectionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get runtime version")]
    RuntimeVersionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system chain")]
    SystemChainFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system version")]
    SystemVersionFailed(#[source] subxt_rpcs::Error),
}

impl From<FetchError> for GetRcNodeVersionError {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::RpcFailed(e) => GetRcNodeVersionError::RuntimeVersionFailed(e),
            _ => unreachable!("fetch_node_version only returns RpcFailed"),
        }
    }
}

impl IntoResponse for GetRcNodeVersionError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcNodeVersionError::RelayChainNotAvailable => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcNodeVersionError::MultiChainConnectionFailed(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcNodeVersionError::RuntimeVersionFailed(err)
            | GetRcNodeVersionError::SystemChainFailed(err)
            | GetRcNodeVersionError::SystemVersionFailed(err) => utils::rpc_error_to_status(err),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

async fn get_relay_rpc_client(state: &AppState) -> Result<Arc<RpcClient>, GetRcNodeVersionError> {
    if let Some(relay_rpc_client) = &state.relay_rpc_client {
        return Ok(relay_rpc_client.clone());
    }

    let relay_url = state
        .config
        .substrate
        .multi_chain_urls
        .iter()
        .find(|chain_url| chain_url.chain_type == ChainType::Relay)
        .map(|chain_url| chain_url.url.clone())
        .ok_or(GetRcNodeVersionError::RelayChainNotAvailable)?;

    let relay_rpc_client = RpcClient::from_insecure_url(&relay_url)
        .await
        .map_err(GetRcNodeVersionError::MultiChainConnectionFailed)?;

    Ok(Arc::new(relay_rpc_client))
}

/// Handler for GET /rc/node/version
///
/// Returns the relay chain node's version information. This endpoint is specifically
/// for Asset Hub instances to query relay chain node version details.
pub async fn get_rc_node_version(
    State(state): State<AppState>,
) -> Result<Json<NodeVersionResponse>, GetRcNodeVersionError> {
    let relay_rpc_client = get_relay_rpc_client(&state).await?;
    let relay_legacy_rpc = LegacyRpcMethods::<SubstrateConfig>::new((*relay_rpc_client).clone());

    let response = fetch_node_version(&relay_rpc_client, &relay_legacy_rpc).await?;
    Ok(Json(response))
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

    fn create_test_state_with_relay_mock(relay_mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let primary_mock = MockRpcClient::builder().build();
        let rpc_client = Arc::new(RpcClient::new(primary_mock));
        let relay_rpc_client = Arc::new(RpcClient::new(relay_mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: config::ChainType::AssetHub,
            spec_name: "statemint".to_string(),
            spec_version: 1,
            ss58_prefix: 0,
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
            relay_rpc_client: Some(relay_rpc_client.clone()),
            relay_chain_rpc: Some(Arc::new(subxt_rpcs::LegacyRpcMethods::new(
                (*relay_rpc_client).clone(),
            ))),
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: crate::routes::RouteRegistry::new(),
        }
    }

    #[tokio::test]
    async fn test_get_rc_node_version_success() {
        let relay_mock = MockRpcClient::builder()
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(serde_json::json!({
                    "specName": "polkadot",
                    "implName": "parity-polkadot",
                    "authoringVersion": 0,
                    "specVersion": 1003000,
                    "implVersion": 0,
                    "apis": [],
                    "transactionVersion": 26,
                    "stateVersion": 1
                }))
            })
            .method_handler("system_chain", async |_params| {
                MockJson("Polkadot".to_string())
            })
            .method_handler("system_version", async |_params| {
                MockJson("1.15.2-abcdef12".to_string())
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock);
        let result = get_rc_node_version(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.chain, "Polkadot");
        assert_eq!(response.client_version, "1.15.2-abcdef12");
        assert_eq!(response.client_impl_name, "parity-polkadot");
    }

    #[tokio::test]
    async fn test_get_rc_node_version_missing_impl_name() {
        let relay_mock = MockRpcClient::builder()
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(serde_json::json!({
                    "specName": "westend",
                    "authoringVersion": 0,
                    "specVersion": 1000000,
                    "implVersion": 0,
                    "apis": [],
                    "transactionVersion": 26,
                    "stateVersion": 1
                }))
            })
            .method_handler("system_chain", async |_params| {
                MockJson("Westend".to_string())
            })
            .method_handler("system_version", async |_params| {
                MockJson("1.0.0".to_string())
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock);
        let result = get_rc_node_version(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.chain, "Westend");
        assert_eq!(response.client_version, "1.0.0");
        assert_eq!(response.client_impl_name, "unknown");
    }
}
