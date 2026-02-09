use crate::handlers::runtime::{
    RuntimeSpecResponse, SpecBlockInfo, transform_chain_type, transform_properties,
};
use crate::state::{AppState, RelayChainError};
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::{Value, json};
use subxt::error::OnlineClientAtBlockError;
use subxt_rpcs::rpc_params;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetRcSpecError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error(transparent)]
    RelayChain(#[from] RelayChainError),

    #[error("Relay chain client not configured")]
    RelayChainNotConfigured,

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[source] Box<OnlineClientAtBlockError>),

    #[error("Failed to get runtime version")]
    RuntimeVersionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system properties")]
    SystemPropertiesFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system chain type")]
    SystemChainTypeFailed(#[source] subxt_rpcs::Error),

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),
}

impl IntoResponse for GetRcSpecError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcSpecError::InvalidBlockParam(_) | GetRcSpecError::RelayChainNotConfigured => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcSpecError::RelayChain(RelayChainError::NotConfigured) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcSpecError::RelayChain(RelayChainError::ConnectionFailed(_))
            | GetRcSpecError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcSpecError::ClientAtBlockFailed(err) => {
                if utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Service temporarily unavailable".to_string(),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }
            GetRcSpecError::RuntimeVersionFailed(err)
            | GetRcSpecError::SystemPropertiesFailed(err)
            | GetRcSpecError::SystemChainTypeFailed(err) => utils::rpc_error_to_status(err),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct AtBlockParam {
    pub at: Option<String>,
}

/// Handler for GET /rc/runtime/spec
///
/// Returns the runtime spec of the relay chain at a given block.
///
/// Query parameters:
/// - `at` (optional): Block identifier (block number or block hash). Defaults to latest block.
pub async fn get_rc_runtime_spec(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<RuntimeSpecResponse>, GetRcSpecError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(GetRcSpecError::RelayChainNotConfigured)?;
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(GetRcSpecError::RelayChainNotConfigured)?;
    let relay_legacy_rpc = state
        .get_relay_chain_rpc()
        .ok_or(GetRcSpecError::RelayChainNotConfigured)?;

    let client_at_block = match params.at {
        None => relay_client
            .at_current_block()
            .await
            .map_err(|e| GetRcSpecError::ClientAtBlockFailed(Box::new(e)))?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<crate::utils::BlockId>()?;
            match block_id {
                crate::utils::BlockId::Hash(hash) => relay_client.at_block(hash).await,
                crate::utils::BlockId::Number(number) => relay_client.at_block(number).await,
            }
            .map_err(|e| GetRcSpecError::ClientAtBlockFailed(Box::new(e)))?
        }
    };

    let block_hash_str = format!("{:#x}", client_at_block.block_hash());
    let block_height = client_at_block.block_number().to_string();

    // Execute RPC calls in parallel using relay chain RPC
    let (runtime_version_result, properties_result, chain_type_result) = tokio::join!(
        relay_rpc_client.request::<Value>("state_getRuntimeVersion", rpc_params![&block_hash_str]),
        relay_legacy_rpc.system_properties(),
        relay_rpc_client.request::<Value>("system_chainType", rpc_params![]),
    );

    let runtime_version = runtime_version_result.map_err(GetRcSpecError::RuntimeVersionFailed)?;
    let properties = properties_result.map_err(GetRcSpecError::SystemPropertiesFailed)?;
    let chain_type = chain_type_result.map_err(GetRcSpecError::SystemChainTypeFailed)?;

    let spec_name = runtime_version
        .get("specName")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let authoring_version = runtime_version
        .get("authoringVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let impl_version = runtime_version
        .get("implVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let spec_version = runtime_version
        .get("specVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let transaction_version = runtime_version
        .get("transactionVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .to_string();

    let response = RuntimeSpecResponse {
        at: SpecBlockInfo {
            hash: block_hash_str,
            height: block_height,
        },
        authoring_version,
        chain_type: transform_chain_type(chain_type),
        impl_version,
        spec_name,
        spec_version,
        transaction_version,
        properties: transform_properties(serde_json::to_value(properties).unwrap_or(json!({}))),
    };

    Ok(Json(response))
}
