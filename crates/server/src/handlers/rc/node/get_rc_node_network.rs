// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::handlers::node::NodeNetworkResponse;
use crate::handlers::node::common::{FetchError, fetch_node_network};
use crate::state::{AppState, RelayChainError};
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetRcNodeNetworkError {
    #[error(transparent)]
    RelayChain(#[from] RelayChainError),

    #[error("Failed to get system health")]
    SystemHealthFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get local peer ID")]
    LocalPeerIdFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get node roles")]
    NodeRolesFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get local listen addresses")]
    LocalListenAddressesFailed(#[source] subxt_rpcs::Error),
}

impl From<FetchError> for GetRcNodeNetworkError {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::RpcFailed(e) => GetRcNodeNetworkError::SystemHealthFailed(e),
            _ => unreachable!("fetch_node_network only returns RpcFailed"),
        }
    }
}

impl IntoResponse for GetRcNodeNetworkError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcNodeNetworkError::RelayChain(RelayChainError::NotConfigured) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcNodeNetworkError::RelayChain(RelayChainError::ConnectionFailed(_)) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcNodeNetworkError::SystemHealthFailed(err)
            | GetRcNodeNetworkError::LocalPeerIdFailed(err)
            | GetRcNodeNetworkError::NodeRolesFailed(err)
            | GetRcNodeNetworkError::LocalListenAddressesFailed(err) => {
                utils::rpc_error_to_status(err)
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Handler for GET /rc/node/network
///
/// Returns the relay chain node's network information. This endpoint is specifically
/// for Asset Hub instances to query relay chain node networking details.
#[utoipa::path(
    get,
    path = "/v1/rc/node/network",
    tag = "rc",
    summary = "RC get node network",
    description = "Returns the relay chain node's network information.",
    responses(
        (status = 200, description = "Relay chain node network info", body = Object),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_node_network(
    State(state): State<AppState>,
) -> Result<Json<NodeNetworkResponse>, GetRcNodeNetworkError> {
    let relay_rpc_client = state.get_or_init_relay_rpc_client().await?;
    let response = fetch_node_network(&relay_rpc_client).await?;
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use crate::test_fixtures::mock_rpc_client_builder;
    use axum::extract::State;
    use config::SidecarConfig;
    use serde_json::Value;
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
            relay_chain_rpc: Some(Arc::new(subxt_rpcs::LegacyRpcMethods::new(
                (*relay_rpc_client).clone(),
            ))),
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: crate::routes::RouteRegistry::new(),
            lazy_relay_rpc: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    #[tokio::test]
    async fn test_get_rc_node_network_success() {
        let relay_mock = mock_rpc_client_builder()
            .method_handler("system_health", async |_params| {
                MockJson(json!({
                    "peers": 74,
                    "isSyncing": false,
                    "shouldHavePeers": true
                }))
            })
            .method_handler("system_localPeerId", async |_params| {
                MockJson("12D3KooWKJGb7Z25jKUsMzWSEmDSXkUBhSxXsxWdDpnzDMPDLgZ1".to_string())
            })
            .method_handler("system_nodeRoles", async |_params| {
                MockJson(vec!["Full".to_string()])
            })
            .method_handler("system_localListenAddresses", async |_params| {
                MockJson(vec![
                    "/ip4/127.0.0.1/tcp/30333".to_string(),
                    "/ip4/100.65.35.228/tcp/30333".to_string(),
                ])
            })
            .method_handler("system_peers", async |_params| {
                MockJson(json!([
                    {
                        "peerId": "12D3KooWFBkZwKye8pKvnG3KH5TN6UNf146Ciz1hCJUZ6mwtE5Qw",
                        "roles": "FULL",
                        "bestHash": "0x9e9c9e87c875a5e5c9296e50d4eca8eb8dc8513ac4fad29756ce9e65066f6525",
                        "bestNumber": 29522680
                    },
                    {
                        "peerId": "12D3KooWCu3pLDAcr1JKf1WoETGaPkkbNTSoYP7cUxkLJpNDUdnd",
                        "roles": "AUTHORITY",
                        "bestHash": "0x52478fd400eaf77adc60455b7a72c7f75a76012078a678c492f2fdfb24fc1ee5",
                        "bestNumber": 29522676
                    }
                ]))
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let result = get_rc_node_network(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.num_peers, 74);
        assert!(!response.is_syncing);
        assert!(response.should_have_peers);
        assert_eq!(
            response.local_peer_id,
            "12D3KooWKJGb7Z25jKUsMzWSEmDSXkUBhSxXsxWdDpnzDMPDLgZ1"
        );
        assert_eq!(response.local_listen_addresses.len(), 2);
        assert_eq!(response.node_roles.len(), 1);

        if let Value::Array(peers) = &response.peers_info {
            assert_eq!(peers.len(), 2);

            let full_peer = &peers[0];
            assert_eq!(
                full_peer.get("peerId").and_then(|v| v.as_str()),
                Some("12D3KooWFBkZwKye8pKvnG3KH5TN6UNf146Ciz1hCJUZ6mwtE5Qw")
            );
            assert_eq!(
                full_peer.get("roles").and_then(|v| v.as_str()),
                Some("FULL")
            );
            assert_eq!(
                full_peer.get("bestNumber").and_then(|v| v.as_str()),
                Some("29522680")
            );

            let auth_peer = &peers[1];
            assert_eq!(
                auth_peer.get("peerId").and_then(|v| v.as_str()),
                Some("12D3KooWCu3pLDAcr1JKf1WoETGaPkkbNTSoYP7cUxkLJpNDUdnd")
            );
            assert_eq!(
                auth_peer.get("roles").and_then(|v| v.as_str()),
                Some("AUTHORITY")
            );
        } else {
            panic!("Expected peers_info to be an array");
        }
    }

    #[tokio::test]
    async fn test_get_rc_node_network_peers_unavailable() {
        let relay_mock = mock_rpc_client_builder()
            .method_handler("system_health", async |_params| {
                MockJson(json!({
                    "peers": 50,
                    "isSyncing": true,
                    "shouldHavePeers": true
                }))
            })
            .method_handler("system_localPeerId", async |_params| {
                MockJson("12D3KooWRelayPeerId".to_string())
            })
            .method_handler("system_nodeRoles", async |_params| {
                MockJson(vec!["Authority".to_string()])
            })
            .method_handler("system_localListenAddresses", async |_params| {
                MockJson(vec!["/ip4/127.0.0.1/tcp/30333".to_string()])
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let result = get_rc_node_network(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.num_peers, 50);
        assert!(response.is_syncing);
        assert_eq!(
            response.peers_info,
            Value::String("Cannot query system_peers from node.".to_string())
        );
    }
}
