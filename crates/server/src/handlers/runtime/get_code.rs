use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetCodeError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Failed to get runtime code")]
    CodeFailed(#[source] subxt_rpcs::Error),
}

impl IntoResponse for GetCodeError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetCodeError::InvalidBlockParam(_) | GetCodeError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
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
pub struct CodeResponse {
    pub at: BlockInfo,
    pub code: String,
}

const CODE_STORAGE_KEY: &str = "0x3a636f6465";

pub async fn runtime_code(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<CodeResponse>, GetCodeError> {
    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let code: Option<String> = state
        .rpc_client
        .request("state_getStorage", subxt_rpcs::rpc_params![CODE_STORAGE_KEY, &resolved_block.hash])
        .await
        .map_err(GetCodeError::CodeFailed)?;

    Ok(Json(CodeResponse {
        at: BlockInfo {
            hash: resolved_block.hash,
            height: resolved_block.number.to_string(),
        },
        code: code.unwrap_or_else(|| "0x".to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use config::SidecarConfig;
    use serde_json::json;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    /// Helper to create a test AppState with mocked RPC responses
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
        }
    }

    #[tokio::test]
    async fn test_runtime_code_at_finalized() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x2a", // Block 42
                }))
            })
            .method_handler("state_getStorage", async |_params| {
                // Return a mock Wasm code blob (just a sample hex string)
                MockJson("0x0061736d0100000001070160017f60037f7f7f")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam { at: None };
        let result = runtime_code(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.height, "42");
        assert!(response.code.starts_with("0x"));
    }

    #[tokio::test]
    async fn test_runtime_code_at_specific_hash() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson("0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64", // Block 100
                }))
            })
            .method_handler("state_getStorage", async |_params| {
                MockJson("0x0061736d0100000001070160017f60037f7f7f")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some("0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_string()),
        };
        let result = runtime_code(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.hash, "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789");
        assert_eq!(response.at.height, "100");
    }

    #[tokio::test]
    async fn test_runtime_code_invalid_block_param() {
        let mock_client = MockRpcClient::builder().build();
        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some("invalid-block".to_string()),
        };
        let result = runtime_code(State(state), axum::extract::Query(params)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_runtime_code_not_found() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x2a",
                }))
            })
            .method_handler("state_getStorage", async |_params| {
                // Return null if code storage is not found
                MockJson(serde_json::Value::Null)
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam { at: None };
        let result = runtime_code(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        // When not found, it should return empty code
        assert_eq!(response.code, "0x");
    }
}
