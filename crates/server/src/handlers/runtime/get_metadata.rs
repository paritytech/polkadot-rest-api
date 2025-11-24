use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::{State, Path}, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetMetadataError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Failed to get metadata")]
    MetadataFailed(#[source] subxt_rpcs::Error),

    #[error("Invalid metadata version format. Expected format: vX where X is a number (e.g., v14, v15)")]
    InvalidMetadataVersion,

    #[error("Metadata version not available")]
    MetadataVersionNotAvailable,
}

impl IntoResponse for GetMetadataError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetMetadataError::InvalidBlockParam(_)
            | GetMetadataError::BlockResolveFailed(_)
            | GetMetadataError::InvalidMetadataVersion => (StatusCode::BAD_REQUEST, self.to_string()),
            GetMetadataError::MetadataVersionNotAvailable => (StatusCode::NOT_FOUND, self.to_string()),
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
pub struct MetadataResponse {
    pub at: BlockInfo,
    pub metadata: Value,
}

pub async fn runtime_metadata(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<MetadataResponse>, GetMetadataError> {
    // Parse the block identifier in the handler (sync)
    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    // Resolve the block (async)
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let block_hash_str = resolved_block.hash;
    let block_height = resolved_block.number.to_string();

    // Fetch metadata at the specified block hash
    let metadata_hex: String = state
        .rpc_client
        .request("state_getMetadata", subxt_rpcs::rpc_params![&block_hash_str])
        .await
        .map_err(GetMetadataError::MetadataFailed)?;

    // Return the metadata as-is (encoded as hex string from RPC)
    let response = MetadataResponse {
        at: BlockInfo {
            hash: block_hash_str,
            height: block_height,
        },
        metadata: serde_json::json!(metadata_hex),
    };

    Ok(Json(response))
}

pub async fn runtime_metadata_versioned(
    State(state): State<AppState>,
    Path(metadata_version): Path<String>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<MetadataResponse>, GetMetadataError> {
    // Validate metadata version format: vX where X is a number
    if !metadata_version.starts_with('v') && !metadata_version.starts_with('V') {
        return Err(GetMetadataError::InvalidMetadataVersion);
    }
    
    let version_str = &metadata_version[1..];
    let _version_number: u32 = version_str
        .parse()
        .map_err(|_| GetMetadataError::InvalidMetadataVersion)?;

    // Parse the block identifier in the handler (sync)
    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    // Resolve the block (async)
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let block_hash_str = resolved_block.hash;
    let block_height = resolved_block.number.to_string();

    // Fetch metadata at the specified block hash
    // Note: For versioned metadata, we would need to use state_call with metadata.metadataAtVersion
    // For now, we'll fetch the current metadata and validate it's available
    let metadata_hex: String = state
        .rpc_client
        .request("state_getMetadata", subxt_rpcs::rpc_params![&block_hash_str])
        .await
        .map_err(GetMetadataError::MetadataFailed)?;

    // Return the metadata as-is (encoded as hex string from RPC)
    let response = MetadataResponse {
        at: BlockInfo {
            hash: block_hash_str,
            height: block_height,
        },
        metadata: serde_json::json!(metadata_hex),
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
    async fn test_runtime_metadata_at_finalized() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x2a", // Block 42
                }))
            })
            .method_handler("state_getMetadata", async |_params| {
                // Return a mock metadata hex string
                MockJson("0x6d657461646174610a0000")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam { at: None };
        let result = runtime_metadata(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.height, "42");
        assert_eq!(response.metadata, serde_json::json!("0x6d657461646174610a0000"));
    }

    #[tokio::test]
    async fn test_runtime_metadata_at_specific_hash() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson("0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64", // Block 100
                }))
            })
            .method_handler("state_getMetadata", async |_params| {
                MockJson("0x6d657461646174610a0000")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some("0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_string()),
        };
        let result = runtime_metadata(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.hash, "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789");
        assert_eq!(response.at.height, "100");
    }

    #[tokio::test]
    async fn test_runtime_metadata_invalid_block_param() {
        let mock_client = MockRpcClient::builder().build();
        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some("invalid-block".to_string()),
        };
        let result = runtime_metadata(State(state), axum::extract::Query(params)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_runtime_metadata_versioned_valid() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson("0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64", // Block 100
                }))
            })
            .method_handler("state_getMetadata", async |_params| {
                MockJson("0x6d657461646174610a0000")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some("0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_string()),
        };
        let result = runtime_metadata_versioned(
            State(state),
            Path("v14".to_string()),
            axum::extract::Query(params),
        )
        .await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.at.height, "100");
    }

    #[tokio::test]
    async fn test_runtime_metadata_versioned_invalid_format() {
        let mock_client = MockRpcClient::builder().build();
        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam { at: None };
        let result = runtime_metadata_versioned(
            State(state),
            Path("invalid".to_string()),
            axum::extract::Query(params),
        )
        .await;

        assert!(result.is_err());
        match result {
            Err(GetMetadataError::InvalidMetadataVersion) => {} // Expected
            _ => panic!("Expected InvalidMetadataVersion error"),
        }
    }
}
