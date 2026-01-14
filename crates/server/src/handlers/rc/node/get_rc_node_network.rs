use crate::handlers::node::NodeNetworkResponse;
use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use config::ChainType;
use serde_json::{Value, json};
use std::sync::Arc;
use subxt_rpcs::RpcClient;
use subxt_rpcs::client::rpc_params;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetRcNodeNetworkError {
    #[error("Relay chain connection not available")]
    RelayChainNotAvailable,

    #[error("Failed to connect to relay chain from multi-chain URLs")]
    MultiChainConnectionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system health")]
    SystemHealthFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get local peer ID")]
    LocalPeerIdFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get node roles")]
    NodeRolesFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get local listen addresses")]
    LocalListenAddressesFailed(#[source] subxt_rpcs::Error),
}

impl IntoResponse for GetRcNodeNetworkError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcNodeNetworkError::RelayChainNotAvailable => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcNodeNetworkError::MultiChainConnectionFailed(_) => {
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

async fn get_relay_rpc_client(state: &AppState) -> Result<Arc<RpcClient>, GetRcNodeNetworkError> {
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
        .ok_or(GetRcNodeNetworkError::RelayChainNotAvailable)?;

    let relay_rpc_client = RpcClient::from_insecure_url(&relay_url)
        .await
        .map_err(GetRcNodeNetworkError::MultiChainConnectionFailed)?;

    Ok(Arc::new(relay_rpc_client))
}

/// Handler for GET /rc/node/network
///
/// Returns the relay chain node's network information. This endpoint is specifically
/// for Asset Hub instances to query relay chain node networking details.
pub async fn get_rc_node_network(
    State(state): State<AppState>,
) -> Result<Json<NodeNetworkResponse>, GetRcNodeNetworkError> {
    let relay_rpc_client = get_relay_rpc_client(&state).await?;

    let (health_result, peer_id_result, roles_result, addresses_result) = tokio::join!(
        relay_rpc_client.request::<Value>("system_health", rpc_params![]),
        relay_rpc_client.request::<String>("system_localPeerId", rpc_params![]),
        relay_rpc_client.request::<Vec<String>>("system_nodeRoles", rpc_params![]),
        relay_rpc_client.request::<Vec<String>>("system_localListenAddresses", rpc_params![]),
    );

    let health = health_result.map_err(GetRcNodeNetworkError::SystemHealthFailed)?;
    let local_peer_id = peer_id_result.map_err(GetRcNodeNetworkError::LocalPeerIdFailed)?;
    let node_roles_raw = roles_result.map_err(GetRcNodeNetworkError::NodeRolesFailed)?;
    let local_listen_addresses =
        addresses_result.map_err(GetRcNodeNetworkError::LocalListenAddressesFailed)?;

    let node_roles: Vec<Value> = node_roles_raw
        .into_iter()
        .map(|role| json!({ role.to_lowercase(): null }))
        .collect();

    let num_peers = health.get("peers").and_then(|v| v.as_u64()).unwrap_or(0);
    let is_syncing = health
        .get("isSyncing")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let should_have_peers = health
        .get("shouldHavePeers")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let peers_info = match relay_rpc_client
        .request::<Value>("system_peers", rpc_params![])
        .await
    {
        Ok(peers) => {
            if let Value::Array(peers_array) = peers {
                let transformed: Vec<Value> = peers_array
                    .into_iter()
                    .filter_map(|peer| {
                        if let Value::Object(peer_obj) = peer {
                            let mut transformed_peer = serde_json::Map::new();

                            let peer_id = peer_obj
                                .get("peerId")
                                .or_else(|| peer_obj.get("peer_id"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            if let Some(pid) = peer_id {
                                transformed_peer.insert("peerId".to_string(), Value::String(pid));
                            }

                            if let Some(roles) = peer_obj.get("roles") {
                                let roles_str = match roles {
                                    Value::String(s) => s.clone(),
                                    Value::Array(arr) => arr
                                        .iter()
                                        .filter_map(|v| v.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                    _ => roles.to_string(),
                                };
                                transformed_peer
                                    .insert("roles".to_string(), Value::String(roles_str));
                            }

                            if let Some(protocol_version) = peer_obj
                                .get("protocolVersion")
                                .or_else(|| peer_obj.get("protocol_version"))
                            {
                                let protocol_version_str = match protocol_version {
                                    Value::Number(n) => n.to_string(),
                                    Value::String(s) => s.clone(),
                                    _ => protocol_version.to_string(),
                                };
                                transformed_peer.insert(
                                    "protocolVersion".to_string(),
                                    Value::String(protocol_version_str),
                                );
                            }

                            if let Some(best_hash) = peer_obj
                                .get("bestHash")
                                .or_else(|| peer_obj.get("best_hash"))
                            {
                                let best_hash_str = match best_hash {
                                    Value::String(s) => s.clone(),
                                    _ => best_hash.to_string(),
                                };
                                transformed_peer
                                    .insert("bestHash".to_string(), Value::String(best_hash_str));
                            }

                            if let Some(best_number) = peer_obj
                                .get("bestNumber")
                                .or_else(|| peer_obj.get("best_number"))
                            {
                                let best_number_str = match best_number {
                                    Value::Number(n) => n.to_string(),
                                    Value::String(s) => s.clone(),
                                    _ => best_number.to_string(),
                                };
                                transformed_peer.insert(
                                    "bestNumber".to_string(),
                                    Value::String(best_number_str),
                                );
                            }

                            if transformed_peer.contains_key("peerId") {
                                Some(Value::Object(transformed_peer))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect();
                Value::Array(transformed)
            } else {
                Value::Array(vec![])
            }
        }
        Err(_) => Value::String("Cannot query system_peers from node.".to_string()),
    };

    Ok(Json(NodeNetworkResponse {
        node_roles,
        num_peers,
        is_syncing,
        should_have_peers,
        local_peer_id,
        local_listen_addresses,
        peers_info,
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
    async fn test_get_rc_node_network_success() {
        let relay_mock = MockRpcClient::builder()
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
                        "peerId": "12D3KooWCamelCasePeer",
                        "roles": "Full",
                        "protocolVersion": 7,
                        "bestHash": "0xabc123",
                        "bestNumber": 12345678
                    },
                    {
                        "peer_id": "12D3KooWSnakeCasePeer",
                        "roles": "Authority",
                        "protocol_version": 8,
                        "best_hash": "0xdef456",
                        "best_number": 999999
                    }
                ]))
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock);
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

            let camel_peer = &peers[0];
            assert_eq!(
                camel_peer.get("peerId").and_then(|v| v.as_str()),
                Some("12D3KooWCamelCasePeer")
            );
            assert_eq!(
                camel_peer.get("protocolVersion").and_then(|v| v.as_str()),
                Some("7")
            );

            let snake_peer = &peers[1];
            assert_eq!(
                snake_peer.get("peerId").and_then(|v| v.as_str()),
                Some("12D3KooWSnakeCasePeer")
            );
            assert_eq!(
                snake_peer.get("protocolVersion").and_then(|v| v.as_str()),
                Some("8")
            );
            assert_eq!(
                snake_peer.get("bestNumber").and_then(|v| v.as_str()),
                Some("999999")
            );
        } else {
            panic!("Expected peers_info to be an array");
        }
    }

    #[tokio::test]
    async fn test_get_rc_node_network_peers_unavailable() {
        let relay_mock = MockRpcClient::builder()
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

        let state = create_test_state_with_relay_mock(relay_mock);
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
