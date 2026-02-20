// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::handlers::node::NodeVersionResponse;
use crate::handlers::node::common::{FetchError, fetch_node_version};
use crate::state::{AppState, RelayChainError};
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetRcNodeVersionError {
    #[error(transparent)]
    RelayChain(#[from] RelayChainError),

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
            GetRcNodeVersionError::RelayChain(RelayChainError::NotConfigured) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcNodeVersionError::RelayChain(RelayChainError::ConnectionFailed(_)) => {
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

/// Handler for GET /rc/node/version
///
/// Returns the relay chain node's version information. This endpoint is specifically
/// for Asset Hub instances to query relay chain node version details.
#[utoipa::path(
    get,
    path = "/v1/rc/node/version",
    tag = "rc",
    summary = "RC get node version",
    description = "Returns the relay chain node's version information.",
    responses(
        (status = 200, description = "Relay chain node version", body = NodeVersionResponse),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_node_version(
    State(state): State<AppState>,
) -> Result<Json<NodeVersionResponse>, GetRcNodeVersionError> {
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_legacy_rpc = state.get_relay_chain_rpc().await?;

    let response = fetch_node_version(&relay_rpc_client, &relay_legacy_rpc).await?;
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

    async fn create_test_state_with_relay_mock(relay_mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let primary_mock = mock_rpc_client_builder().build();
        let rpc_client = Arc::new(RpcClient::new(primary_mock));
        let relay_rpc_client = Arc::new(RpcClient::new(relay_mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: polkadot_rest_api_config::ChainType::AssetHub,
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
            relay_rpc_client: {
                let cell = Arc::new(tokio::sync::OnceCell::new());
                cell.set(relay_rpc_client.clone()).ok();
                cell
            },
            relay_chain_rpc: {
                let cell = Arc::new(tokio::sync::OnceCell::new());
                cell.set(Arc::new(subxt_rpcs::LegacyRpcMethods::new(
                    (*relay_rpc_client).clone(),
                ))).ok();
                cell
            },
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(polkadot_rest_api_config::ChainConfigs::default()),
            chain_config: Arc::new(polkadot_rest_api_config::Config::single_chain(
                polkadot_rest_api_config::ChainConfig::default(),
            )),
            route_registry: crate::routes::RouteRegistry::new(),
        }
    }

    #[tokio::test]
    async fn test_get_rc_node_version_success() {
        let relay_mock = mock_rpc_client_builder()
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

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let result = get_rc_node_version(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.chain, "Polkadot");
        assert_eq!(response.client_version, "1.15.2-abcdef12");
        assert_eq!(response.client_impl_name, "parity-polkadot");
    }

    #[tokio::test]
    async fn test_get_rc_node_version_missing_impl_name() {
        let relay_mock = mock_rpc_client_builder()
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

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let result = get_rc_node_version(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.chain, "Westend");
        assert_eq!(response.client_version, "1.0.0");
        assert_eq!(response.client_impl_name, "unknown");
    }
}
