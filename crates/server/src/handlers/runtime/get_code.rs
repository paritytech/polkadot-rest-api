// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use serde_json::json;
use subxt::error::OnlineClientAtBlockError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetCodeError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[source] Box<OnlineClientAtBlockError>),

    #[error("Failed to get runtime code")]
    GetCodeFailed(#[source] subxt::error::StorageError),

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),
}

impl IntoResponse for GetCodeError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetCodeError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            GetCodeError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetCodeError::ClientAtBlockFailed(err) => {
                if utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Service temporarily unavailable".to_string(),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }
            GetCodeError::GetCodeFailed(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
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

/// Query parameters for the runtime code endpoint
#[derive(Debug, serde::Deserialize)]
pub struct AtBlockParam {
    pub at: Option<String>,
}

/// Handler for GET /runtime/code
///
/// Returns the Wasm code blob of the Substrate runtime at a given block.
///
/// Query parameters:
/// - `at` (optional): Block identifier (block number or block hash). Defaults to latest block.
///
/// Returns:
/// - `at`: Block number and hash at which the call was made
/// - `code`: Runtime code Wasm blob as hex string
#[utoipa::path(
    get,
    path = "/v1/runtime/code",
    tag = "runtime",
    summary = "Runtime Wasm code",
    description = "Returns the Wasm code blob of the Substrate runtime at a given block.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Runtime code", body = Object),
        (status = 400, description = "Invalid block parameter"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn runtime_code(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<RuntimeCodeResponse>, GetCodeError> {
    // Create client at the specified block - saves RPC calls by letting subxt
    // resolve hash<->number internally
    let client_at_block = match params.at {
        None => {
            // Use current finalized block
            state
                .client
                .at_current_block()
                .await
                .map_err(|e| GetCodeError::ClientAtBlockFailed(Box::new(e)))?
        }
        Some(ref at_str) => {
            let block_id = at_str.parse::<crate::utils::BlockId>()?;
            match block_id {
                crate::utils::BlockId::Hash(hash) => state.client.at_block(hash).await,
                crate::utils::BlockId::Number(number) => state.client.at_block(number).await,
            }
            .map_err(|e| GetCodeError::ClientAtBlockFailed(Box::new(e)))?
        }
    };

    // Extract hash and number from the resolved client
    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let block_number = client_at_block.block_number();

    // Get the runtime code using subxt's built-in helper
    let wasm_blob: Vec<u8> = client_at_block
        .storage()
        .runtime_wasm_code()
        .await
        .map_err(GetCodeError::GetCodeFailed)?;

    // Convert to hex string with 0x prefix
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
    use crate::state::AppState;
    use crate::test_fixtures::{TEST_BLOCK_HASH, TEST_BLOCK_NUMBER, mock_rpc_client_builder};
    use axum::extract::{Query, State};
    use polkadot_rest_api_config::SidecarConfig;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    async fn create_test_state_with_mock(mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let rpc_client = Arc::new(RpcClient::new(mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: polkadot_rest_api_config::ChainType::Relay,
            spec_name: "test".to_string(),
            spec_version: 1,
            ss58_prefix: 42,
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
            relay_rpc_client: None,
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: crate::routes::RouteRegistry::new(),
            relay_chain_rpc: None,
            lazy_relay_rpc: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Test WASM code blob (minimal valid WASM module)
    const TEST_WASM_CODE: &str = "0x0061736d0100000001";

    #[tokio::test]
    async fn test_runtime_code_at_block_hash() {
        // Use test fixtures builder and add state_getStorage handler for runtime code
        let mock_client = mock_rpc_client_builder()
            .method_handler("state_getStorage", async |_params| MockJson(TEST_WASM_CODE))
            .build();

        let state = create_test_state_with_mock(mock_client).await;
        let params = AtBlockParam {
            at: Some(TEST_BLOCK_HASH.to_string()),
        };

        let result = runtime_code(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.at.height, TEST_BLOCK_NUMBER.to_string());
        assert_eq!(response.code, TEST_WASM_CODE);
    }

    #[tokio::test]
    async fn test_runtime_code_at_block_number() {
        // Use test fixtures builder and add state_getStorage handler for runtime code
        let mock_client = mock_rpc_client_builder()
            .method_handler("state_getStorage", async |_params| MockJson(TEST_WASM_CODE))
            .build();

        let state = create_test_state_with_mock(mock_client).await;
        let params = AtBlockParam {
            at: Some(TEST_BLOCK_NUMBER.to_string()),
        };

        let result = runtime_code(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        // Block number should match the request
        assert_eq!(response.at.height, TEST_BLOCK_NUMBER.to_string());
        // Hash should be resolved from the mock
        assert_eq!(response.at.hash, TEST_BLOCK_HASH);
        assert_eq!(response.code, TEST_WASM_CODE);
    }

    #[tokio::test]
    async fn test_runtime_code_latest_block() {
        // Use test fixtures builder and add state_getStorage handler for runtime code
        let mock_client = mock_rpc_client_builder()
            .method_handler("state_getStorage", async |_params| MockJson(TEST_WASM_CODE))
            .build();

        let state = create_test_state_with_mock(mock_client).await;
        let params = AtBlockParam { at: None };

        let result = runtime_code(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        // Should use finalized head from test fixtures
        assert_eq!(response.at.height, TEST_BLOCK_NUMBER.to_string());
        assert_eq!(response.code, TEST_WASM_CODE);
    }
}
