//! Handler for GET /rc/blocks/{blockId}/header
//!
//! Returns the header of a specific relay chain block by block hash or block number.

use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::json;
use subxt::config::substrate::{ConsensusEngineId, DigestItem};
use thiserror::Error;

/// Relay chain block header response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlockHeaderResponse {
    pub parent_hash: String,
    pub number: String,
    pub state_root: String,
    pub extrinsics_root: String,
    pub digest: serde_json::Value,
}

/// Error types for /rc/blocks/{blockId}/header endpoint
#[derive(Debug, Error)]
pub enum GetRcBlockHeaderError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Failed to get block header: {0}")]
    HeaderFetchFailed(String),

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Relay chain API is not configured. Please configure SAS_SUBSTRATE_MULTI_CHAIN_URL")]
    RelayChainNotConfigured,

    #[error("Block not found: {0}")]
    BlockNotFound(String),
}

impl IntoResponse for GetRcBlockHeaderError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcBlockHeaderError::RelayChainNotConfigured
            | GetRcBlockHeaderError::InvalidBlockParam(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcBlockHeaderError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcBlockHeaderError::BlockNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            GetRcBlockHeaderError::HeaderFetchFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Handler for GET /rc/blocks/{blockId}/header
///
/// Returns the header of a relay chain block by block hash or block number.
///
/// # Path Parameters
/// - `blockId`: Block identifier (height number or block hash)
pub async fn get_rc_block_header(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
) -> Result<Response, GetRcBlockHeaderError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(GetRcBlockHeaderError::RelayChainNotConfigured)?;

    let block_id_parsed = block_id.parse::<utils::BlockId>()?;

    let client_at_block = match &block_id_parsed {
        utils::BlockId::Number(n) => relay_client.at_block(*n).await.map_err(|e| {
            if e.to_string().contains("not found") {
                GetRcBlockHeaderError::BlockNotFound(format!("Block at height {} not found", n))
            } else {
                GetRcBlockHeaderError::HeaderFetchFailed(e.to_string())
            }
        })?,
        utils::BlockId::Hash(h) => relay_client.at_block(*h).await.map_err(|e| {
            if e.to_string().contains("not found") {
                GetRcBlockHeaderError::BlockNotFound(format!("Block with hash {:#x} not found", h))
            } else {
                GetRcBlockHeaderError::HeaderFetchFailed(e.to_string())
            }
        })?,
    };

    let header = client_at_block
        .block_header()
        .await
        .map_err(|e| GetRcBlockHeaderError::HeaderFetchFailed(e.to_string()))?;

    let response = RcBlockHeaderResponse {
        parent_hash: format!("0x{}", hex::encode(header.parent_hash.0)),
        number: header.number.to_string(),
        state_root: format!("0x{}", hex::encode(header.state_root.0)),
        extrinsics_root: format!("0x{}", hex::encode(header.extrinsics_root.0)),
        digest: json!({
            "logs": convert_digest_logs(&header.digest.logs)
        }),
    };

    Ok(Json(response).into_response())
}

fn convert_digest_logs(logs: &[DigestItem]) -> Vec<serde_json::Value> {
    logs.iter()
        .map(|item| match item {
            DigestItem::PreRuntime(engine_id, data) => {
                json!({
                    "preRuntime": format_consensus_digest(engine_id, data)
                })
            }
            DigestItem::Consensus(engine_id, data) => {
                json!({
                    "consensus": format_consensus_digest(engine_id, data)
                })
            }
            DigestItem::Seal(engine_id, data) => {
                json!({
                    "seal": format_consensus_digest(engine_id, data)
                })
            }
            DigestItem::Other(data) => {
                json!({
                    "other": format!("0x{}", hex::encode(data))
                })
            }
            DigestItem::RuntimeEnvironmentUpdated => {
                json!({
                    "runtimeEnvironmentUpdated": null
                })
            }
        })
        .collect()
}

fn format_consensus_digest(engine_id: &ConsensusEngineId, data: &[u8]) -> serde_json::Value {
    json!([
        format!("0x{}", hex::encode(engine_id)),
        format!("0x{}", hex::encode(data))
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PARENT_HASH: &str =
        "0xb5531541d3c407569749190350c19784baee799e1e4b9ea52471e75150cd3ec1";
    const TEST_STATE_ROOT: &str =
        "0xa50e5b59f2978c4c6b3c5a54f905711d323741e0b512c8280835c75e8e9afb43";
    const TEST_EXTRINSICS_ROOT: &str =
        "0xefbdd47ab4826ccd29911e04f8b93df56ef43241c3c75f610f0171152a48c6b1";
    const TEST_BLOCK_NUMBER: u64 = 29639318;

    #[test]
    fn test_response_has_correct_structure() {
        let response = RcBlockHeaderResponse {
            parent_hash: TEST_PARENT_HASH.to_string(),
            number: TEST_BLOCK_NUMBER.to_string(),
            state_root: TEST_STATE_ROOT.to_string(),
            extrinsics_root: TEST_EXTRINSICS_ROOT.to_string(),
            digest: json!({
                "logs": [
                    {"preRuntime": ["0x42414245", "0x032d010000"]},
                    {"consensus": ["0x42454546", "0x03c41b16c2"]},
                    {"seal": ["0x42414245", "0x8e3d333ec6"]}
                ]
            }),
        };

        let json = serde_json::to_value(&response).unwrap();

        // Verify all required fields are present
        assert!(json.get("parentHash").is_some());
        assert!(json.get("number").is_some());
        assert!(json.get("stateRoot").is_some());
        assert!(json.get("extrinsicsRoot").is_some());
        assert!(json.get("digest").is_some());

        // Verify field types
        assert!(json["parentHash"].is_string());
        assert!(json["number"].is_string());
        assert!(json["stateRoot"].is_string());
        assert!(json["extrinsicsRoot"].is_string());
        assert!(json["digest"]["logs"].is_array());

        // Verify hash fields start with 0x
        assert!(json["parentHash"].as_str().unwrap().starts_with("0x"));
        assert!(json["stateRoot"].as_str().unwrap().starts_with("0x"));
        assert!(json["extrinsicsRoot"].as_str().unwrap().starts_with("0x"));

        // Verify number is a valid decimal string
        let number_str = json["number"].as_str().unwrap();
        assert!(number_str.parse::<u64>().is_ok());
    }
}
