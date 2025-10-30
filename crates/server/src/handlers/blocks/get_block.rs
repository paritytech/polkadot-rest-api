use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetBlockError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),
}

impl IntoResponse for GetBlockError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetBlockError::InvalidBlockParam(_) | GetBlockError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Basic block information
#[derive(Debug, Serialize)]
pub struct BlockResponse {
    pub number: String,
    pub hash: String,
    // TODO: Add more fields (extrinsics, logs, onInitialize, onFinalize, etc.)
}

/// Handler for GET /blocks/{blockId}
///
/// Returns block information for a given block identifier (hash or number)
pub async fn get_block(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
) -> Result<Json<BlockResponse>, GetBlockError> {
    // Parse the block identifier
    let block_id = block_id.parse::<utils::BlockId>()?;

    // Resolve the block
    let resolved_block = utils::resolve_block(&state, Some(block_id)).await?;

    // Build basic response (more fields to be added)
    let response = BlockResponse {
        number: resolved_block.number.to_string(),
        hash: resolved_block.hash,
    };

    Ok(Json(response))
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
    async fn test_get_block_by_number() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let result = get_block(State(state), Path("100".to_string())).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.number, "100");
        assert_eq!(
            response.hash,
            "0x1234567890123456789012345678901234567890123456789012345678901234"
        );
    }

    #[tokio::test]
    async fn test_get_block_by_hash() {
        let test_hash = "0xabcdef1234567890123456789012345678901234567890123456789012345678";

        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64", // Block 100
                }))
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let result = get_block(State(state), Path(test_hash.to_string())).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.number, "100");
        assert_eq!(response.hash, test_hash);
    }

    #[tokio::test]
    async fn test_get_block_invalid_param() {
        let mock_client = MockRpcClient::builder().build();
        let state = create_test_state_with_mock(mock_client);

        let result = get_block(State(state), Path("invalid".to_string())).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetBlockError::InvalidBlockParam(_)
        ));
    }

    #[tokio::test]
    async fn test_get_block_not_found() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson(serde_json::Value::Null)
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let result = get_block(State(state), Path("999999".to_string())).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetBlockError::BlockResolveFailed(_)
        ));
    }
}
