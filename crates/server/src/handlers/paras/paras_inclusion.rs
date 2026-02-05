use crate::state::AppState;
use crate::utils::extract_block_number_from_header;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::future::join_all;
use scale_decode::DecodeAsType;
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt::{OnlineClient, SubstrateConfig};
use thiserror::Error;

#[derive(DecodeAsType)]
struct SetValidationData {
    data: BasicParachainInherentData,
}

#[derive(DecodeAsType)]
struct BasicParachainInherentData {
    validation_data: PersistedValidationData,
}

#[derive(DecodeAsType)]
struct PersistedValidationData {
    relay_parent_number: u64,
}

#[derive(DecodeAsType)]
struct CandidateIncludedEvent {
    receipt: CandidateReceiptDecoded,
    head_data: Vec<u8>,
}

#[derive(DecodeAsType)]
struct CandidateReceiptDecoded {
    descriptor: CandidateDescriptorDecoded,
}

#[derive(DecodeAsType)]
struct CandidateDescriptorDecoded {
    para_id: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParasInclusionQueryParams {
    /// Search depth for relay chain blocks (max 100, default 10)
    #[serde(default = "default_depth")]
    pub depth: String,
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

    #[error("Depth parameter must be divisible by 5 for optimal performance.")]
    DepthNotOptimal,

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
            | ParasInclusionError::DepthNotOptimal
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

const DEFAULT_SEARCH_DEPTH: &str = "10";
const MAX_DEPTH: u32 = 100;
const BATCH_SIZE: u32 = 5;

fn default_depth() -> String {
    DEFAULT_SEARCH_DEPTH.to_string()
}

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

    let para_id = get_parachain_id(&state, block_number).await?;
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

fn validate_depth(depth: String) -> Result<u32, ParasInclusionError> {
    let parsed: i32 = depth
        .parse()
        .map_err(|_| ParasInclusionError::InvalidDepth)?;

    match parsed {
        x if x <= 0 => Err(ParasInclusionError::InvalidDepth),
        x if x > MAX_DEPTH as i32 => Err(ParasInclusionError::DepthTooLarge),
        x if x % 5 != 0 => Err(ParasInclusionError::DepthNotOptimal),
        _ => Ok(parsed as u32),
    }
}

async fn get_parachain_id(state: &AppState, block_number: u64) -> Result<u32, ParasInclusionError> {
    let client_at_block = state
        .client
        .at_block(block_number)
        .await
        .map_err(|e| ParasInclusionError::ClientAtBlockFailed(e.to_string()))?;

    let addr = subxt::dynamic::storage::<(), u32>("ParachainInfo", "ParachainId");

    let result = match client_at_block.storage().fetch(addr, ()).await {
        Ok(v) => v,
        Err(_) => return Err(ParasInclusionError::NotAParachain),
    };

    let id = result
        .decode()
        .map_err(|e| ParasInclusionError::DecodeFailed(e.to_string()))?;

    Ok(id)
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
            let call_data: SetValidationData = ext
                .decode_call_data_as()
                .map_err(|e| ParasInclusionError::DecodeFailed(e.to_string()))?;

            return Ok(call_data.data.validation_data.relay_parent_number);
        }
    }

    Err(ParasInclusionError::NoValidationData)
}

/// Search relay chain blocks for inclusion of a parachain block.
/// Checks blocks in parallel batches of 5 for faster lookups.
async fn search_for_inclusion_block(
    relay_client: &OnlineClient<SubstrateConfig>,
    para_id: u32,
    parachain_block_number: u64,
    relay_parent_number: u64,
    max_depth: u32,
) -> Option<u64> {
    // Search blocks starting from relay_parent_number + 1
    // Process in batches of BATCH_SIZE concurrently, returning the earliest match
    for batch_start in (0..max_depth).step_by(BATCH_SIZE as usize) {
        let futures: Vec<_> = (0..BATCH_SIZE)
            .map(|i| {
                let block_num = relay_parent_number + (batch_start + i) as u64 + 1;
                check_block_for_inclusion(relay_client, block_num, para_id, parachain_block_number)
            })
            .collect();

        let results = join_all(futures).await;

        // Return the earliest match within this batch
        if let Some(found) = results.into_iter().flatten().next() {
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

        let event_data: CandidateIncludedEvent = match event.decode_fields_unchecked_as() {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some(inclusion_block_num) =
            extract_inclusion_info(&event_data, para_id, parachain_block_number)
        {
            return Some(inclusion_block_num);
        }
    }

    None
}

/// Extract parachain block number from CandidateIncluded event if it matches target para_id
fn extract_inclusion_info(
    event: &CandidateIncludedEvent,
    target_para_id: u32,
    expected_block_number: u64,
) -> Option<u64> {
    if event.receipt.descriptor.para_id != target_para_id {
        return None;
    }

    let block_number = extract_block_number_from_header(&event.head_data)?;

    if block_number == expected_block_number {
        Some(block_number)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_depth_default() {
        assert_eq!(validate_depth(default_depth()).unwrap(), 10);
    }

    #[test]
    fn test_validate_depth_valid() {
        assert_eq!(validate_depth("5".to_string()).unwrap(), 5);
        assert_eq!(validate_depth("10".to_string()).unwrap(), 10);
        assert_eq!(validate_depth("100".to_string()).unwrap(), 100);
    }

    #[test]
    fn test_validate_depth_invalid() {
        assert!(matches!(
            validate_depth("0".to_string()),
            Err(ParasInclusionError::InvalidDepth)
        ));
        assert!(matches!(
            validate_depth("-5".to_string()),
            Err(ParasInclusionError::InvalidDepth)
        ));
        assert!(matches!(
            validate_depth("abc".to_string()),
            Err(ParasInclusionError::InvalidDepth)
        ));
    }

    #[test]
    fn test_validate_depth_too_large() {
        assert!(matches!(
            validate_depth("101".to_string()),
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
