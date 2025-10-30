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
pub enum GetBlockError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Failed to get block header")]
    HeaderFetchFailed(#[source] subxt_rpcs::Error),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),
}

impl IntoResponse for GetBlockError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetBlockError::InvalidBlockParam(_) | GetBlockError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetBlockError::HeaderFetchFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            GetBlockError::HeaderFieldMissing(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Represents a decoded digest log entry
#[derive(Debug, Serialize)]
pub struct DigestLog {
    #[serde(rename = "type")]
    pub log_type: String,
    pub index: u32,
    pub value: Value,
}

/// Extract author ID from block header digest logs
///
/// TODO: Implement author extraction using validator set and consensus-specific logic.
/// This requires:
/// 1. Fetching the validator set for the block
/// 2. Using consensus-specific logic (BABE/Aura/Nimbus) to map PreRuntime data to a validator
/// 3. Handling different consensus mechanisms
///
/// For now, this always returns None.
fn _extract_author_from_digest(_header_json: &Value) -> Option<String> {
    None
}

/// Decode digest logs from hex-encoded strings in the JSON response
/// Each hex string is a SCALE-encoded DigestItem
fn decode_digest_logs(header_json: &Value) -> Vec<DigestLog> {
    let logs = match header_json
        .get("digest")
        .and_then(|d| d.get("logs"))
        .and_then(|l| l.as_array())
    {
        Some(logs) => logs,
        None => return Vec::new(),
    };

    logs.iter()
        .enumerate()
        .filter_map(|(index, log_hex)| {
            let hex_str = log_hex.as_str()?;
            let hex_data = hex_str.strip_prefix("0x")?;
            let bytes = hex::decode(hex_data).ok()?;

            if bytes.is_empty() {
                return None;
            }

            // The first byte is the digest item type discriminant
            let discriminant = bytes[0];
            let data = &bytes[1..];

            let (log_type, value) = match discriminant {
                // PreRuntime: [consensus_engine_id (4 bytes), data]
                6 if data.len() >= 4 => {
                    let engine_id = String::from_utf8_lossy(&data[0..4]).to_string();
                    let payload = format!("0x{}", hex::encode(&data[4..]));
                    ("PreRuntime".to_string(), json!([engine_id, payload]))
                }
                // Consensus: [consensus_engine_id (4 bytes), data]
                4 if data.len() >= 4 => {
                    let engine_id = String::from_utf8_lossy(&data[0..4]).to_string();
                    let payload = format!("0x{}", hex::encode(&data[4..]));
                    ("Consensus".to_string(), json!([engine_id, payload]))
                }
                // Seal: [consensus_engine_id (4 bytes), data]
                5 if data.len() >= 4 => {
                    let engine_id = String::from_utf8_lossy(&data[0..4]).to_string();
                    let payload = format!("0x{}", hex::encode(&data[4..]));
                    ("Seal".to_string(), json!([engine_id, payload]))
                }
                // Other
                0 => (
                    "Other".to_string(),
                    json!(format!("0x{}", hex::encode(data))),
                ),
                // RuntimeEnvironmentUpdated
                8 => ("RuntimeEnvironmentUpdated".to_string(), Value::Null),
                // Unknown/Other discriminants
                _ => (
                    "Other".to_string(),
                    json!(format!("0x{}", hex::encode(bytes))),
                ),
            };

            Some(DigestLog {
                log_type,
                index: index as u32,
                value,
            })
        })
        .collect()
}

/// Basic block information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockResponse {
    pub number: String,
    pub hash: String,
    pub parent_hash: String,
    pub state_root: String,
    pub extrinsics_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    pub logs: Vec<DigestLog>,
    // TODO: Add more fields (extrinsics, onInitialize, onFinalize, etc.)
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

    // Fetch the header JSON
    let header_json = state
        .get_header_json(&resolved_block.hash)
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;

    // Extract header fields
    let parent_hash = header_json
        .get("parentHash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("parentHash".to_string()))?
        .to_string();

    let state_root = header_json
        .get("stateRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("stateRoot".to_string()))?
        .to_string();

    let extrinsics_root = header_json
        .get("extrinsicsRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("extrinsicsRoot".to_string()))?
        .to_string();

    // TODO: Extract author from digest logs requires validator set and consensus logic
    // For now, always return None
    let author_id = None;

    // Decode digest logs from hex strings into structured format
    let logs = decode_digest_logs(&header_json);

    // Build response
    let response = BlockResponse {
        number: resolved_block.number.to_string(),
        hash: resolved_block.hash,
        parent_hash,
        state_root,
        extrinsics_root,
        author_id,
        logs,
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
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64",
                    "parentHash": "0xabcdef0000000000000000000000000000000000000000000000000000000000",
                    "stateRoot": "0xdef0000000000000000000000000000000000000000000000000000000000000",
                    "extrinsicsRoot": "0x1230000000000000000000000000000000000000000000000000000000000000",
                    "digest": {
                        "logs": [
                            // PreRuntime log: discriminant (6) + engine_id ("BABE") + data
                            "0x064241424501020304"
                        ]
                    }
                }))
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
        assert_eq!(
            response.parent_hash,
            "0xabcdef0000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            response.state_root,
            "0xdef0000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            response.extrinsics_root,
            "0x1230000000000000000000000000000000000000000000000000000000000000"
        );
        // TODO: Author extraction not yet implemented
        assert_eq!(response.author_id, None);
        // Verify logs are decoded
        assert_eq!(response.logs.len(), 1);
        assert_eq!(response.logs[0].log_type, "PreRuntime");
        assert_eq!(response.logs[0].index, 0);
        // Verify the engine ID is "BABE" and payload is present
        if let Some(arr) = response.logs[0].value.as_array() {
            assert_eq!(arr[0].as_str(), Some("BABE"));
            assert!(arr[1].as_str().unwrap().starts_with("0x"));
        } else {
            panic!("Expected PreRuntime log value to be an array");
        }
    }

    #[tokio::test]
    async fn test_get_block_by_hash() {
        let test_hash = "0xabcdef1234567890123456789012345678901234567890123456789012345678";

        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64", // Block 100
                    "parentHash": "0x9999990000000000000000000000000000000000000000000000000000000000",
                    "stateRoot": "0x8888880000000000000000000000000000000000000000000000000000000000",
                    "extrinsicsRoot": "0x7777770000000000000000000000000000000000000000000000000000000000",
                    "digest": {
                        "logs": []
                    }
                }))
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let result = get_block(State(state), Path(test_hash.to_string())).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.number, "100");
        assert_eq!(response.hash, test_hash);
        assert_eq!(
            response.parent_hash,
            "0x9999990000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            response.state_root,
            "0x8888880000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            response.extrinsics_root,
            "0x7777770000000000000000000000000000000000000000000000000000000000"
        );
        // TODO: Author extraction not yet implemented
        assert_eq!(response.author_id, None);
        // Empty logs array
        assert_eq!(response.logs.len(), 0);
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
