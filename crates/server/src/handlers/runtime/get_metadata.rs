use crate::handlers::common::{AtBlockParam, BlockInfo};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Serialize;
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

    #[error(
        "Invalid metadata version format. Expected format: vX where X is a number (e.g., v14, v15)"
    )]
    InvalidMetadataVersion,

    #[error("Metadata version not available")]
    MetadataVersionNotAvailable,
}

impl IntoResponse for GetMetadataError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetMetadataError::InvalidBlockParam(_)
            | GetMetadataError::BlockResolveFailed(_)
            | GetMetadataError::InvalidMetadataVersion => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetMetadataError::MetadataVersionNotAvailable => {
                (StatusCode::NOT_FOUND, self.to_string())
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
#[serde(rename_all = "camelCase")]
pub struct MetadataResponse {
    pub at: BlockInfo,
    pub magic_number: Option<String>,
    pub metadata: Value,
}

fn extract_magic_number(metadata_hex: &str) -> Option<String> {
    let trimmed = metadata_hex.trim_start_matches("0x");
    if trimmed.len() < 8 {
        return None;
    }

    let magic_bytes = &trimmed[0..8];
    let byte0 = u8::from_str_radix(&magic_bytes[0..2], 16).ok()?;
    let byte1 = u8::from_str_radix(&magic_bytes[2..4], 16).ok()?;
    let byte2 = u8::from_str_radix(&magic_bytes[4..6], 16).ok()?;
    let byte3 = u8::from_str_radix(&magic_bytes[6..8], 16).ok()?;

    let magic_value = u32::from_be_bytes([byte0, byte1, byte2, byte3]);
    Some(magic_value.to_string())
}

pub async fn runtime_metadata(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<MetadataResponse>, GetMetadataError> {
    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let metadata_hex: String = state
        .rpc_client
        .request(
            "state_getMetadata",
            subxt_rpcs::rpc_params![&resolved_block.hash],
        )
        .await
        .map_err(GetMetadataError::MetadataFailed)?;

    let magic_number = extract_magic_number(&metadata_hex);

    Ok(Json(MetadataResponse {
        at: BlockInfo {
            hash: resolved_block.hash,
            height: resolved_block.number.to_string(),
        },
        magic_number,
        metadata: serde_json::json!(metadata_hex),
    }))
}

pub async fn runtime_metadata_versioned(
    State(state): State<AppState>,
    Path(metadata_version): Path<String>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<MetadataResponse>, GetMetadataError> {
    if !metadata_version.starts_with('v') && !metadata_version.starts_with('V') {
        return Err(GetMetadataError::InvalidMetadataVersion);
    }

    let version_str = &metadata_version[1..];
    let _version_number: u32 = version_str
        .parse()
        .map_err(|_| GetMetadataError::InvalidMetadataVersion)?;

    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let metadata_hex: String = state
        .rpc_client
        .request(
            "state_getMetadata",
            subxt_rpcs::rpc_params![&resolved_block.hash],
        )
        .await
        .map_err(GetMetadataError::MetadataFailed)?;

    let magic_number = extract_magic_number(&metadata_hex);

    Ok(Json(MetadataResponse {
        at: BlockInfo {
            hash: resolved_block.hash,
            height: resolved_block.number.to_string(),
        },
        magic_number,
        metadata: serde_json::json!(metadata_hex),
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
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
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
        assert_eq!(
            response.metadata,
            serde_json::json!("0x6d657461646174610a0000")
        );
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
            at: Some(
                "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_string(),
            ),
        };
        let result = runtime_metadata(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(
            response.at.hash,
            "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        );
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
            at: Some(
                "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_string(),
            ),
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
