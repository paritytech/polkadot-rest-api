// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use utoipa::ToSchema;

use super::common::{FetchError, fetch_node_version};

#[derive(Debug, Error)]
pub enum GetNodeVersionError {
    #[error("Failed to get runtime version")]
    RuntimeVersionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system chain")]
    SystemChainFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system version")]
    SystemVersionFailed(#[source] subxt_rpcs::Error),
}

impl From<FetchError> for GetNodeVersionError {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::RpcFailed(e) => GetNodeVersionError::RuntimeVersionFailed(e),
            _ => unreachable!("fetch_node_version only returns RpcFailed"),
        }
    }
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

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeVersionResponse {
    pub client_version: String,
    pub client_impl_name: String,
    pub chain: String,
}

#[utoipa::path(
    get,
    path = "/v1/node/version",
    tag = "node",
    summary = "Node version",
    description = "Returns the node's version information including client version, implementation name, and chain name.",
    responses(
        (status = 200, description = "Node version information", body = NodeVersionResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_node_version(
    State(state): State<AppState>,
) -> Result<Json<NodeVersionResponse>, GetNodeVersionError> {
    let response = fetch_node_version(&state.rpc_client, &state.legacy_rpc).await?;
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use crate::test_fixtures::mock_rpc_client_builder;
    use axum::extract::State;
    use polkadot_rest_api_config::SidecarConfig;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    async fn create_test_state_with_mock(mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let rpc_client = Arc::new(RpcClient::new(mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: polkadot_rest_api_config::ChainType::Relay,
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
    async fn test_get_node_version_success() {
        let mock_client = mock_rpc_client_builder()
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

        let state = create_test_state_with_mock(mock_client).await;
        let result = get_node_version(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.chain, "Polkadot Asset Hub");
        assert_eq!(response.client_version, "1.16.0-xyz9876");
        assert_eq!(response.client_impl_name, "asset-hub-polkadot");
    }

    #[tokio::test]
    async fn test_get_node_version_missing_impl_name() {
        let mock_client = mock_rpc_client_builder()
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

        let state = create_test_state_with_mock(mock_client).await;
        let result = get_node_version(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.chain, "Westend Asset Hub");
        assert_eq!(response.client_version, "1.0.0");
        assert_eq!(response.client_impl_name, "unknown");
    }
}
