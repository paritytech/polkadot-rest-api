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
    // Execute RPC calls in parallel, similar to the TypeScript implementation
    let (runtime_version_result, chain_result, version_result) = tokio::join!(
        state.legacy_rpc.state_get_runtime_version(None),
        state.rpc_client.request::<String>("system_chain", rpc_params![]),
        state.rpc_client.request::<String>("system_version", rpc_params![]),
    );

    let runtime_version = runtime_version_result.map_err(GetNodeVersionError::RuntimeVersionFailed)?;
    let chain = chain_result.map_err(GetNodeVersionError::SystemChainFailed)?;
    let client_version = version_result.map_err(GetNodeVersionError::SystemVersionFailed)?;

    // Extract implName from runtime version
    // The implName is in the "other" HashMap in the RuntimeVersion struct
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
