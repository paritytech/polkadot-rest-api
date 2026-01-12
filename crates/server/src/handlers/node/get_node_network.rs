use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subxt_rpcs::client::rpc_params;
use thiserror::Error;

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
    let (health_result, peer_id_result, roles_result, addresses_result) = tokio::join!(
        state
            .rpc_client
            .request::<Value>("system_health", rpc_params![]),
        state
            .rpc_client
            .request::<String>("system_localPeerId", rpc_params![]),
        state
            .rpc_client
            .request::<Vec<String>>("system_nodeRoles", rpc_params![]),
        state
            .rpc_client
            .request::<Vec<String>>("system_localListenAddresses", rpc_params![]),
    );

    let health = health_result.map_err(GetNodeNetworkError::SystemHealthFailed)?;
    let local_peer_id = peer_id_result.map_err(GetNodeNetworkError::LocalPeerIdFailed)?;
    let node_roles_raw = roles_result.map_err(GetNodeNetworkError::NodeRolesFailed)?;
    let local_listen_addresses =
        addresses_result.map_err(GetNodeNetworkError::LocalListenAddressesFailed)?;

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

    //TODO: check peers_info with a compatible node
    let peers_info = match state
        .rpc_client
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
                                    Value::Array(arr) => {
                                        // If it's an array, join or convert
                                        arr.iter()
                                            .filter_map(|v| v.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    }
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
