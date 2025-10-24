use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetSpecError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockResolveError),

    #[error("Failed to get runtime version")]
    RuntimeVersionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get system properties")]
    SystemPropertiesFailed(#[source] subxt_rpcs::Error),
}

impl IntoResponse for GetSpecError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetSpecError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct AtBlockParam {
    pub at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BlockInfo {
    pub hash: String,
    pub height: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpecResponse {
    pub at: BlockInfo,
    pub authoring_version: String,
    pub chain_type: Value,
    pub impl_version: String,
    pub spec_name: String,
    pub spec_version: String,
    pub transaction_version: String,
    pub properties: Value,
}

pub async fn runtime_spec(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<RuntimeSpecResponse>, GetSpecError> {
    let resolved_block = utils::resolve_block(&state, params.at).await?;

    let block_hash_str = resolved_block.hash;
    let block_height = resolved_block.number.to_string();

    let runtime_version = state
        .get_runtime_version_at_hash(&block_hash_str)
        .await
        .map_err(GetSpecError::RuntimeVersionFailed)?;

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

    let properties = state
        .legacy_rpc
        .system_properties()
        .await
        .map_err(GetSpecError::SystemPropertiesFailed)?;

    // TODO: system_chain_type is not available in LegacyRpcMethods
    // Need to find the correct RPC method or use a different approach
    // For now, default to "live"
    let chain_type = serde_json::json!({
        "live": null
    });

    let response = RuntimeSpecResponse {
        at: BlockInfo {
            hash: block_hash_str,
            height: block_height,
        },
        authoring_version,
        chain_type,
        impl_version,
        spec_name,
        spec_version,
        transaction_version,
        properties: serde_json::to_value(properties).unwrap_or(serde_json::json!({})),
    };

    Ok(Json(response))
}
