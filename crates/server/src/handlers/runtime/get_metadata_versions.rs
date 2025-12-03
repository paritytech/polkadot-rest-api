use crate::handlers::common::{AtBlockParam, BlockInfo};
use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use hex;
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetMetadataVersionsError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Function 'api.call.metadata.metadataVersions()' is not available at this block height.")]
    MetadataVersionsNotAvailable,

    #[error("Failed to get metadata versions")]
    MetadataVersionsFailed(#[source] subxt_rpcs::Error),
}

impl IntoResponse for GetMetadataVersionsError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetMetadataVersionsError::InvalidBlockParam(_)
            | GetMetadataVersionsError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "code": status.as_u16(),
            "message": message,
            "stack": format!("Error: {}", message),
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Serialize)]
pub struct MetadataVersionsResponse {
    pub at: BlockInfo,
    pub versions: Vec<String>,
}

pub async fn runtime_metadata_versions(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<MetadataVersionsResponse>, GetMetadataVersionsError> {
    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let versions_hex: String = state
        .rpc_client
        .request(
            "state_call",
            subxt_rpcs::rpc_params!["Metadata_metadata_versions", "0x", &resolved_block.hash],
        )
        .await
        .map_err(|e| {
            let error_message = e.to_string();
            if error_message.contains("Cannot resolve a callabel function") 
                || error_message.contains("failed to find a function")
                || error_message.contains("not found")
                || error_message.contains("does not exist") {
                GetMetadataVersionsError::MetadataVersionsNotAvailable
            } else {
                GetMetadataVersionsError::MetadataVersionsFailed(e)
            }
        })?;

    let versions_bytes = hex::decode(versions_hex.trim_start_matches("0x")).map_err(|_| {
        GetMetadataVersionsError::MetadataVersionsFailed(subxt_rpcs::Error::Client(
            "Failed to decode versions hex".into(),
        ))
    })?;

    let mut versions = Vec::new();
    for chunk in versions_bytes.chunks(4) {
        if chunk.len() == 4 {
            let version = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            versions.push(version.to_string());
        }
    }

    Ok(Json(MetadataVersionsResponse {
        at: BlockInfo {
            hash: resolved_block.hash,
            height: resolved_block.number.to_string(),
        },
        versions,
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
    async fn test_runtime_metadata_versions_at_finalized() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x2a", // Block 42
                }))
            })
            .method_handler("state_call", async |_params| {
                // Return hex-encoded versions: 14, 15 encoded as little-endian u32
                // Version 14: 0x0e000000, Version 15: 0x0f000000
                MockJson("0x0e0000000f000000")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam { at: None };
        let result = runtime_metadata_versions(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.at.height, "42");
        assert_eq!(response.versions, vec!["14".to_string(), "15".to_string()]);
    }

    #[tokio::test]
    async fn test_runtime_metadata_versions_at_specific_hash() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson("0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64", // Block 100
                }))
            })
            .method_handler("state_call", async |_params| {
                // Return hex-encoded versions: 14, 15
                MockJson("0x0e0000000f000000")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some(
                "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_string(),
            ),
        };
        let result = runtime_metadata_versions(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(
            response.at.hash,
            "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        );
        assert_eq!(response.at.height, "100");
    }

    #[tokio::test]
    async fn test_runtime_metadata_versions_invalid_block_param() {
        let mock_client = MockRpcClient::builder().build();
        let state = create_test_state_with_mock(mock_client);

        let params = AtBlockParam {
            at: Some("invalid-block".to_string()),
        };
        let result = runtime_metadata_versions(State(state), axum::extract::Query(params)).await;

        assert!(result.is_err());
    }
}
