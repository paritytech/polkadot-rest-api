use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use serde_json::json;
use subxt_rpcs::rpc_params;
use thiserror::Error;

/// The storage key for `:code` - this is the well-known key for the runtime WASM blob
/// https://github.com/shawntabrizi/substrate-graph-benchmarks/blob/ae9b82f/js/extensions/known-keys.js#L21
const CODE_KEY: &str = "0x3a636f6465";

#[derive(Debug, Error)]
pub enum GetCodeError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Failed to get runtime code")]
    GetCodeFailed(#[source] subxt_rpcs::Error),
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
pub async fn runtime_code(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<RuntimeCodeResponse>, GetCodeError> {
    // Parse and resolve the block identifier
    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    // Get the runtime code from storage using the well-known :code key
    let code: Option<String> = state
        .rpc_client
        .request(
            "state_getStorage",
            rpc_params![CODE_KEY, &resolved_block.hash],
        )
        .await
        .map_err(GetCodeError::GetCodeFailed)?;

    // The code should always exist for a valid block
    let code = code.unwrap_or_default();

    Ok(Json(RuntimeCodeResponse {
        at: BlockInfo {
            hash: resolved_block.hash,
            height: resolved_block.number.to_string(),
        },
        code,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use axum::extract::{Query, State};
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
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            route_registry: crate::routes::RouteRegistry::new(),
        }
    }

    #[tokio::test]
    async fn test_runtime_code_at_block_hash() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getHeader", async |_params| {
                MockJson(serde_json::json!({
                    "number": "0x100",
                }))
            })
            .method_handler("state_getStorage", async |_params| {
                MockJson("0x0061736d0100000001")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);
        let params = AtBlockParam {
            at: Some(
                "0x1234567890123456789012345678901234567890123456789012345678901234".to_string(),
            ),
        };

        let result = runtime_code(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.at.height, "256");
        assert_eq!(response.code, "0x0061736d0100000001");
    }

    #[tokio::test]
    async fn test_runtime_code_at_block_number() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson("0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(serde_json::json!({
                    "number": "0x2710",
                }))
            })
            .method_handler("state_getStorage", async |_params| {
                MockJson("0x0061736d0100000001")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);
        let params = AtBlockParam {
            at: Some("10000".to_string()),
        };

        let result = runtime_code(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.at.height, "10000");
        assert_eq!(
            response.at.hash,
            "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
        );
        assert_eq!(response.code, "0x0061736d0100000001");
    }

    #[tokio::test]
    async fn test_runtime_code_latest_block() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson("0xfeedfacedeadbeef1234567890abcdef1234567890abcdef1234567890abcdef")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(serde_json::json!({
                    "number": "0x1e8480",
                }))
            })
            .method_handler("state_getStorage", async |_params| {
                MockJson("0x0061736d0100000001deadbeef")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);
        let params = AtBlockParam { at: None };

        let result = runtime_code(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.at.height, "2000000");
        assert_eq!(response.code, "0x0061736d0100000001deadbeef");
    }
}
