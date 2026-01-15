use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use super::common::{FetchError, fetch_node_network};

#[derive(Debug, Error)]
pub enum GetNodeNetworkError {
    #[error("Failed to get system health")]
    SystemHealthFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get local peer ID")]
    LocalPeerIdFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get node roles")]
    NodeRolesFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get local listen addresses")]
    LocalListenAddressesFailed(#[source] subxt_rpcs::Error),
}

impl From<FetchError> for GetNodeNetworkError {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::RpcFailed(e) => GetNodeNetworkError::SystemHealthFailed(e),
            _ => unreachable!("fetch_node_network only returns RpcFailed"),
        }
    }
}

impl IntoResponse for GetNodeNetworkError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetNodeNetworkError::SystemHealthFailed(err)
            | GetNodeNetworkError::LocalPeerIdFailed(err)
            | GetNodeNetworkError::NodeRolesFailed(err)
            | GetNodeNetworkError::LocalListenAddressesFailed(err) => {
                utils::rpc_error_to_status(err)
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeNetworkResponse {
    pub node_roles: Vec<Value>,
    #[serde(serialize_with = "serialize_u64_as_string")]
    pub num_peers: u64,
    pub is_syncing: bool,
    pub should_have_peers: bool,
    pub local_peer_id: String,
    pub local_listen_addresses: Vec<String>,
    pub peers_info: Value,
}

fn serialize_u64_as_string<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_string())
}

/// Handler for GET /node/network
///
/// Returns the node's network information
pub async fn get_node_network(
    State(state): State<AppState>,
) -> Result<Json<NodeNetworkResponse>, GetNodeNetworkError> {
    let response = fetch_node_network(&state.rpc_client).await?;
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
    async fn test_get_node_network_success() {
        let mock_client = MockRpcClient::builder()
            .method_handler("system_health", async |_params| {
                MockJson(json!({
                    "peers": 42,
                    "isSyncing": false,
                    "shouldHavePeers": true
                }))
            })
            .method_handler("system_localPeerId", async |_params| {
                MockJson("12D3KooWNDrKSayoZXGGE2dRSFW2g1iGPq3fTZE2U39ma9yZGKd3".to_string())
            })
            .method_handler("system_nodeRoles", async |_params| {
                MockJson(vec!["Full".to_string()])
            })
            .method_handler("system_localListenAddresses", async |_params| {
                MockJson(vec![
                    "/ip4/127.0.0.1/tcp/30333".to_string(),
                    "/ip4/192.168.1.1/tcp/30333".to_string(),
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

        let state = create_test_state_with_mock(mock_client);
        let result = get_node_network(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.num_peers, 42);
        assert!(!response.is_syncing);
        assert!(response.should_have_peers);
        assert_eq!(
            response.local_peer_id,
            "12D3KooWNDrKSayoZXGGE2dRSFW2g1iGPq3fTZE2U39ma9yZGKd3"
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
    async fn test_get_node_network_peers_unavailable() {
        let mock_client = MockRpcClient::builder()
            .method_handler("system_health", async |_params| {
                MockJson(json!({
                    "peers": 10,
                    "isSyncing": true,
                    "shouldHavePeers": true
                }))
            })
            .method_handler("system_localPeerId", async |_params| {
                MockJson("12D3KooWTestPeerId".to_string())
            })
            .method_handler("system_nodeRoles", async |_params| {
                MockJson(vec!["Authority".to_string()])
            })
            .method_handler("system_localListenAddresses", async |_params| {
                MockJson(vec!["/ip4/127.0.0.1/tcp/30333".to_string()])
            })
            .build();

        let state = create_test_state_with_mock(mock_client);
        let result = get_node_network(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.num_peers, 10);
        assert!(response.is_syncing);
        assert_eq!(
            response.peers_info,
            Value::String("Cannot query system_peers from node.".to_string())
        );
    }
}
