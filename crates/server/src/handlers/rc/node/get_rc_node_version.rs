use crate::handlers::node::NodeVersionResponse;
use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use config::ChainType;
use serde_json::json;
use std::sync::Arc;
use subxt_historic::SubstrateConfig;
use subxt_rpcs::{LegacyRpcMethods, RpcClient, client::rpc_params};
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

    let (runtime_version_result, chain_result, version_result) = tokio::join!(
        relay_legacy_rpc.state_get_runtime_version(None),
        relay_rpc_client.request::<String>("system_chain", rpc_params![]),
        relay_rpc_client.request::<String>("system_version", rpc_params![]),
    );

    let runtime_version =
        runtime_version_result.map_err(GetRcNodeVersionError::RuntimeVersionFailed)?;
    let chain = chain_result.map_err(GetRcNodeVersionError::SystemChainFailed)?;
    let client_version = version_result.map_err(GetRcNodeVersionError::SystemVersionFailed)?;

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
