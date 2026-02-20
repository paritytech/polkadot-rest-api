// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::{AppState, RelayChainError};
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use serde_json::json;
use subxt::error::OnlineClientAtBlockError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetRcCodeError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error(transparent)]
    RelayChain(#[from] RelayChainError),

    #[error("Relay chain client not configured")]
    RelayChainNotConfigured,

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[source] Box<OnlineClientAtBlockError>),

    #[error("Failed to get runtime code")]
    GetCodeFailed(#[source] subxt::error::StorageError),

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),
}

impl From<utils::ResolveClientAtBlockError> for GetRcCodeError {
    fn from(err: utils::ResolveClientAtBlockError) -> Self {
        match err {
            utils::ResolveClientAtBlockError::ParseError(e) => GetRcCodeError::InvalidBlockParam(e),
            utils::ResolveClientAtBlockError::SubxtError(e) => {
                GetRcCodeError::ClientAtBlockFailed(Box::new(e))
            }
        }
    }
}

impl IntoResponse for GetRcCodeError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcCodeError::InvalidBlockParam(_) | GetRcCodeError::RelayChainNotConfigured => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcCodeError::RelayChain(RelayChainError::NotConfigured) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcCodeError::RelayChain(RelayChainError::ConnectionFailed(_))
            | GetRcCodeError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcCodeError::ClientAtBlockFailed(err) => {
                if utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Service temporarily unavailable".to_string(),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }
            GetRcCodeError::GetCodeFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Serialize)]
pub struct BlockInfo {
    pub hash: String,
    pub height: String,
}

#[derive(Debug, Serialize)]
pub struct RuntimeCodeResponse {
    pub at: BlockInfo,
    pub code: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtBlockParam {
    pub at: Option<String>,
}

/// Handler for GET /rc/runtime/code
///
/// Returns the Wasm code blob of the relay chain runtime at a given block.
///
/// Query parameters:
/// - `at` (optional): Block identifier (block number or block hash). Defaults to latest block.
///
/// Returns:
/// - `at`: Block number and hash at which the call was made
/// - `code`: Runtime code Wasm blob as hex string
#[utoipa::path(
    get,
    path = "/v1/rc/runtime/code",
    tag = "rc",
    summary = "RC get runtime code",
    description = "Returns the Wasm code blob of the relay chain runtime at a given block.",
    params(
        ("at" = Option<String>, Query, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Relay chain runtime code", body = Object),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_runtime_code(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<RuntimeCodeResponse>, GetRcCodeError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(GetRcCodeError::RelayChainNotConfigured)?;

    let client_at_block =
        utils::resolve_client_at_block(relay_client.as_ref(), params.at.as_ref()).await?;

    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    let wasm_blob: Vec<u8> = client_at_block
        .storage()
        .runtime_wasm_code()
        .await
        .map_err(GetRcCodeError::GetCodeFailed)?;

    let code = format!("0x{}", hex::encode(&wasm_blob));

    Ok(Json(RuntimeCodeResponse {
        at: BlockInfo {
            hash: block_hash,
            height: block_number.to_string(),
        },
        code,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_at_block_param_rejects_unknown_fields() {
        let json = r#"{"at": "123", "unknownField": true}"#;
        let result: Result<AtBlockParam, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
