use crate::state::AppState;
use crate::utils::{extract_block_number_from_header, extract_bytes_from_json};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use scale_value::Value;
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt::{OnlineClient, SubstrateConfig};
use thiserror::Error;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParasInclusionQueryParams {
    /// Search depth for relay chain blocks (max 100, default 10)
    pub depth: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParasInclusionResponse {
    pub parachain_block: u64,
    pub parachain_block_hash: String,
    pub parachain_id: u32,
    pub relay_parent_number: u64,
    pub inclusion_number: Option<u64>,
    pub found: bool,
}

#[derive(Debug, Error)]
pub enum ParasInclusionError {
    #[error("Invalid depth parameter. Must be a positive integer.")]
    InvalidDepth,

    #[error("Depth parameter cannot exceed 100 to prevent excessive network requests.")]
    DepthTooLarge,

    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(String),

    #[error("Block {0} not found")]
    BlockNotFound(u64),

    #[error(
        "Block does not contain setValidationData extrinsic. Cannot determine relay parent number."
    )]
    NoValidationData,

    #[error("This endpoint requires a parachain connection (parachainInfo pallet not found)")]
    NotAParachain,

    #[error("Relay chain api must be available")]
    RelayChainNotAvailable,

    #[error("RPC call failed: {0}")]
    RpcFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to decode: {0}")]
    DecodeFailed(String),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(String),

    #[error("Failed to fetch events: {0}")]
    EventsFetchFailed(String),
}

impl IntoResponse for ParasInclusionError {
    fn into_response(self) -> Response {
        let status = match &self {
            ParasInclusionError::InvalidDepth
            | ParasInclusionError::DepthTooLarge
            | ParasInclusionError::InvalidBlockParam(_)
            | ParasInclusionError::BlockNotFound(_)
            | ParasInclusionError::NotAParachain => StatusCode::BAD_REQUEST,

            ParasInclusionError::NoValidationData
            | ParasInclusionError::RelayChainNotAvailable
            | ParasInclusionError::DecodeFailed(_)
            | ParasInclusionError::ClientAtBlockFailed(_)
            | ParasInclusionError::EventsFetchFailed(_) => StatusCode::INTERNAL_SERVER_ERROR,

            ParasInclusionError::RpcFailed(err) => {
                let (status, message) = crate::utils::rpc_error_to_status(err);
                return (status, Json(json!({ "error": message }))).into_response();
            }
        };

        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

const DEFAULT_SEARCH_DEPTH: u32 = 10;
const MAX_DEPTH: u32 = 100;

/// Handler for GET /paras/{number}/inclusion
pub async fn get_paras_inclusion(
    State(state): State<AppState>,
    Path(number): Path<String>,
    Query(params): Query<ParasInclusionQueryParams>,
) -> Result<Json<ParasInclusionResponse>, ParasInclusionError> {
    let search_depth = validate_depth(params.depth)?;

    let block_number: u64 = number
        .parse()
        .map_err(|_| ParasInclusionError::InvalidBlockParam(number.clone()))?;

    let block_hash = state
        .get_block_hash_at_number(block_number)
        .await
        .map_err(ParasInclusionError::RpcFailed)?
        .ok_or(ParasInclusionError::BlockNotFound(block_number))?;

    let para_id = get_parachain_id(&state).await?;
    let relay_parent_number = extract_relay_parent_number(&state, &block_hash).await?;

    let relay_client = state
        .get_relay_chain_client()
        .ok_or(ParasInclusionError::RelayChainNotAvailable)?;

    let inclusion_number = search_for_inclusion_block(
        relay_client,
        para_id,
        block_number,
        relay_parent_number,
        search_depth,
    )
    .await;

    Ok(Json(ParasInclusionResponse {
        parachain_block: block_number,
        parachain_block_hash: block_hash,
        parachain_id: para_id,
        relay_parent_number,
        inclusion_number,
        found: inclusion_number.is_some(),
    }))
}

fn validate_depth(depth: Option<String>) -> Result<u32, ParasInclusionError> {
    match depth {
        Some(depth_str) => {
            let parsed: i32 = depth_str
                .parse()
                .map_err(|_| ParasInclusionError::InvalidDepth)?;

            if parsed <= 0 {
                return Err(ParasInclusionError::InvalidDepth);
            }
            if parsed > MAX_DEPTH as i32 {
                return Err(ParasInclusionError::DepthTooLarge);
            }

            Ok(parsed as u32)
        }
        None => Ok(DEFAULT_SEARCH_DEPTH),
    }
}

async fn get_parachain_id(state: &AppState) -> Result<u32, ParasInclusionError> {
    let client_at_block = state
        .client
        .at_current_block()
        .await
        .map_err(|e| ParasInclusionError::ClientAtBlockFailed(e.to_string()))?;

    let addr = subxt::dynamic::storage::<(), Value<()>>("ParachainInfo", "ParachainId");

    let result = match client_at_block.storage().fetch(addr, ()).await {
        Ok(v) => v,
        Err(_) => return Err(ParasInclusionError::NotAParachain),
    };

    let value: Value<()> = result
        .decode_as()
        .map_err(|e| ParasInclusionError::DecodeFailed(e.to_string()))?;

    // ParachainId is a simple u32
    let json = serde_json::to_value(&value)
        .map_err(|e| ParasInclusionError::DecodeFailed(e.to_string()))?;

    extract_u32_from_json(&json)
        .ok_or_else(|| ParasInclusionError::DecodeFailed("Failed to decode parachain ID".into()))
}

async fn extract_relay_parent_number(
    state: &AppState,
    block_hash: &str,
) -> Result<u64, ParasInclusionError> {
    let block_hash_h256 = block_hash
        .parse::<subxt::utils::H256>()
        .map_err(|e| ParasInclusionError::InvalidBlockParam(e.to_string()))?;

    let client_at_block = state
        .client
        .at_block(block_hash_h256)
        .await
        .map_err(|e| ParasInclusionError::ClientAtBlockFailed(e.to_string()))?;

    let extrinsics = client_at_block
        .extrinsics()
        .fetch()
        .await
        .map_err(|e| ParasInclusionError::DecodeFailed(e.to_string()))?;

    for ext_result in extrinsics.iter() {
        let ext = match ext_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        if ext.pallet_name() == "ParachainSystem" && ext.call_name() == "set_validation_data" {
            // Decode call data as dynamic Value
            let call_data: Value<()> = ext
                .decode_call_data_as()
                .map_err(|e| ParasInclusionError::DecodeFailed(e.to_string()))?;

            let json = serde_json::to_value(&call_data)
                .map_err(|e| ParasInclusionError::DecodeFailed(e.to_string()))?;

            // Navigate: data -> validation_data -> relay_parent_number
            if let Some(relay_parent) = json
                .get("data")
                .or_else(|| json.get(0))
                .and_then(|d| d.get("validation_data").or_else(|| d.get(0)))
                .and_then(|v| v.get("relay_parent_number").or_else(|| v.get(0)))
                .and_then(|n| n.as_u64())
            {
                return Ok(relay_parent);
            }
        }
    }

    Err(ParasInclusionError::NoValidationData)
}

/// Search relay chain blocks for inclusion of a parachain block
async fn search_for_inclusion_block(
    relay_client: &OnlineClient<SubstrateConfig>,
    para_id: u32,
    parachain_block_number: u64,
    relay_parent_number: u64,
    max_depth: u32,
) -> Option<u64> {
    // Search blocks starting from relay_parent_number + 1
    // Most inclusions happen within 2-4 blocks
    for offset in 0..max_depth {
        let block_num = relay_parent_number + offset as u64 + 1;

        if let Some(found) =
            check_block_for_inclusion(relay_client, block_num, para_id, parachain_block_number)
                .await
        {
            return Some(found);
        }
    }

    None
}

/// Check a single relay chain block for parachain inclusion using subxt's events API
async fn check_block_for_inclusion(
    relay_client: &OnlineClient<SubstrateConfig>,
    block_num: u64,
    para_id: u32,
    parachain_block_number: u64,
) -> Option<u64> {
    // Get client at this block
    let client_at_block = relay_client.at_block(block_num).await.ok()?;

    // Fetch events using subxt's proper events API
    let events = client_at_block.events().fetch().await.ok()?;

    // Iterate through events looking for ParaInclusion::CandidateIncluded
    for event_result in events.iter() {
        let event = match event_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Use the clean pallet_name() and event_name() API
        if event.pallet_name() != "ParaInclusion" || event.event_name() != "CandidateIncluded" {
            continue;
        }

        // Decode the event fields as a dynamic Value
        let fields: Value<()> = match event.decode_fields_unchecked_as() {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Check if this event matches our parachain
        if let Some(block_num) = extract_inclusion_info(&fields, para_id, parachain_block_number) {
            return Some(block_num);
        }
    }

    None
}

/// Extract parachain block number from CandidateIncluded event if it matches target para_id
fn extract_inclusion_info(
    fields: &Value<()>,
    target_para_id: u32,
    expected_block_number: u64,
) -> Option<u64> {
    let json = serde_json::to_value(fields).ok()?;

    // CandidateIncluded has: [CandidateReceipt, HeadData, CoreIndex, GroupIndex]
    // Get as array (unnamed fields)
    let arr = json.as_array()?;
    if arr.len() < 2 {
        return None;
    }

    // Extract para_id from CandidateReceipt.descriptor.para_id
    let candidate_receipt = &arr[0];
    let para_id = candidate_receipt
        .get("descriptor")
        .or_else(|| candidate_receipt.get(0))
        .and_then(|d| d.get("para_id").or_else(|| d.get(0)))
        .and_then(extract_u32_from_json)?;

    if para_id != target_para_id {
        return None;
    }

    // Extract block number from HeadData (parachain header bytes)
    let head_data = &arr[1];
    let header_bytes = extract_bytes_from_json(head_data)?;
    let block_number = extract_block_number_from_header(&header_bytes)?;

    if block_number == expected_block_number {
        Some(block_number)
    } else {
        None
    }
}

/// Extract u32 from various JSON representations (handles arbitrary nesting)
fn extract_u32_from_json(json: &serde_json::Value) -> Option<u32> {
    // Direct number
    if let Some(n) = json.as_u64() {
        return u32::try_from(n).ok();
    }

    // Array - recurse into first element
    if let Some(arr) = json.as_array()
        && let Some(first) = arr.first()
    {
        return extract_u32_from_json(first);
    }

    // Object - recurse into first value
    if let Some(obj) = json.as_object()
        && let Some(val) = obj.values().next()
    {
        return extract_u32_from_json(val);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_depth_default() {
        assert_eq!(validate_depth(None).unwrap(), 10);
    }

    #[test]
    fn test_validate_depth_valid() {
        assert_eq!(validate_depth(Some("5".to_string())).unwrap(), 5);
        assert_eq!(validate_depth(Some("10".to_string())).unwrap(), 10);
        assert_eq!(validate_depth(Some("100".to_string())).unwrap(), 100);
    }

    #[test]
    fn test_validate_depth_invalid() {
        assert!(matches!(
            validate_depth(Some("0".to_string())),
            Err(ParasInclusionError::InvalidDepth)
        ));
        assert!(matches!(
            validate_depth(Some("-5".to_string())),
            Err(ParasInclusionError::InvalidDepth)
        ));
        assert!(matches!(
            validate_depth(Some("abc".to_string())),
            Err(ParasInclusionError::InvalidDepth)
        ));
    }

    #[test]
    fn test_validate_depth_too_large() {
        assert!(matches!(
            validate_depth(Some("101".to_string())),
            Err(ParasInclusionError::DepthTooLarge)
        ));
    }

    #[test]
    fn test_error_messages() {
        assert_eq!(
            ParasInclusionError::InvalidDepth.to_string(),
            "Invalid depth parameter. Must be a positive integer."
        );
        assert_eq!(
            ParasInclusionError::NoValidationData.to_string(),
            "Block does not contain setValidationData extrinsic. Cannot determine relay parent number."
        );
        assert_eq!(
            ParasInclusionError::RelayChainNotAvailable.to_string(),
            "Relay chain api must be available"
        );
    }
}
