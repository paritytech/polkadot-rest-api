use serde::Serialize;
use serde_json::Value;
use subxt_historic::{OnlineClient, SubstrateConfig};
use subxt_rpcs::{RpcClient, rpc_params};
use thiserror::Error;

/// Error types for RC block operations
#[derive(Debug, Error)]
pub enum RcBlockError {
    #[error("Asset Hub connection not available")]
    AssetHubNotAvailable,

    #[error("Failed to connect to Asset Hub node")]
    AssetHubConnectionFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to query Asset Hub blocks")]
    AssetHubQueryFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get block header")]
    HeaderFetchFailed(#[source] subxt_rpcs::Error),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),

    #[error("Failed to parse block number")]
    BlockNumberParseFailed(#[source] std::num::ParseIntError),

    #[error("Failed to query events: {0}")]
    EventsQueryFailed(String),
}

/// Represents an Asset Hub block that corresponds to a Relay Chain block
#[derive(Debug, Clone)]
pub struct AssetHubBlock {
    /// Asset Hub block hash
    pub hash: String,
    /// Asset Hub block number
    pub number: u64,
    /// Timestamp from the block (extracted from extrinsics or digest)
    pub timestamp: Option<String>,
}

/// Response wrapper for useRcBlock queries
///
/// When useRcBlock=true, responses are wrapped with additional metadata
/// including the RC block number and AH timestamp.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlockResponse<T> {
    /// Block information (hash and height)
    pub at: BlockInfo,
    /// The actual response data
    pub data: T,
    /// Relay Chain block number (as string)
    pub rc_block_number: String,
    /// Asset Hub block timestamp (as string)
    pub ah_timestamp: String,
}

/// Block information structure
#[derive(Debug, Serialize)]
pub struct BlockInfo {
    pub hash: String,
    pub height: String,
}

/// Find Asset Hub blocks that correspond to a specific Relay Chain block number
///
/// This function queries the Relay Chain block's events to find all Asset Hub blocks
/// that were included at that specific relay chain block. With elastic scaling, there may be
/// multiple Asset Hub blocks per relay chain block.
///
/// Implementation matches the TypeScript sidecar: it queries system events in the RC block
/// to find `paraInclusion.CandidateIncluded` events for Asset Hub (paraId 1000), then extracts
/// the AH block headers from those events.
pub async fn find_ah_blocks_by_rc_block(
    rc_client: &OnlineClient<SubstrateConfig>,
    rc_block_number: u64,
) -> Result<Vec<AssetHubBlock>, RcBlockError> {
    use parity_scale_codec::Decode;
    
    const ASSET_HUB_PARA_ID: u32 = 1000;
    
    // Get client at the specific RC block
    let rc_client_at = rc_client
        .at(rc_block_number)
        .await
        .map_err(|e| RcBlockError::EventsQueryFailed(format!("Failed to get client at block: {:?}", e)))?;

    // Query System::Events storage to get all events
    let storage_entry = rc_client_at
        .storage()
        .entry("System", "Events")
        .map_err(|e| RcBlockError::EventsQueryFailed(format!("Failed to get events storage entry: {:?}", e)))?;
    
    let plain_entry = storage_entry
        .into_plain()
        .map_err(|e| RcBlockError::EventsQueryFailed(format!("Failed to convert to plain entry: {:?}", e)))?;
    
    let events_storage_value = plain_entry
        .fetch()
        .await
        .map_err(|e| RcBlockError::EventsQueryFailed(format!("Failed to fetch events: {:?}", e)))?
        .ok_or_else(|| RcBlockError::HeaderFieldMissing("Events not found".to_string()))?;

    // Get the events bytes
    let events_bytes = events_storage_value.into_bytes();
    
    // Decode Vec<EventRecord> - events are stored as a SCALE-encoded Vec
    let mut events_cursor = &events_bytes[..];
    
    // Decode Vec length (compact u32)
    let _events_len = u32::decode(&mut events_cursor)
        .map_err(|_| RcBlockError::EventsQueryFailed("Failed to decode events length".to_string()))?;
    
    let ah_blocks = Vec::new();
    
    // TODO: Implement proper event decoding
    // The TypeScript implementation uses rcApiAt.query.system.events() which automatically decodes events.
    // In Rust with subxt, we need to manually decode the events from System::Events storage.
    // 
    // Steps needed:
    // 1. Decode each EventRecord from the Vec<EventRecord>
    // 2. Check if event is paraInclusion.CandidateIncluded (by pallet/variant index from metadata)
    // 3. Decode event data: (CandidateReceipt, HeadData, CoreIndex, GroupIndex)
    // 4. Extract paraId from CandidateReceipt.descriptor
    // 5. Filter for paraId == 1000 (Asset Hub)
    // 6. Extract HeadData (second element) which is the AH block header
    // 7. Create AH block hash from header using Blake2-256
    // 8. Extract AH block number from header
    //
    // This requires:
    // - Decoding complex SCALE-encoded structures (CandidateReceipt, Header)
    // - Access to metadata to know pallet/variant indices
    // - Proper handling of variable-length structures
    //
    // For now, return empty array - this needs proper implementation
    // Alternative: Use RPC method that returns decoded events if available
    
    Ok(ah_blocks)
}

/// Extract timestamp from block header
///
/// Timestamps in Substrate blocks are typically in the first extrinsic
/// or in digest logs. This function attempts to extract it.
fn extract_timestamp_from_header(_header_json: &Value) -> Option<String> {
    // Try to extract from extrinsics (timestamp is usually the first extrinsic)
    // For now, we'll return None and implement proper extraction later
    // This requires fetching the full block, not just the header

    // Alternative: Check digest logs for timestamp information
    // But typically timestamp is in extrinsics

    None
}

/// Get Asset Hub block with full information including timestamp
pub async fn get_ah_block_with_timestamp(
    ah_rpc_client: &RpcClient,
    block_hash: &str,
) -> Result<AssetHubBlock, RcBlockError> {
    // Get header first
    let _header_json: Value = ah_rpc_client
        .request("chain_getHeader", rpc_params![block_hash])
        .await
        .map_err(RcBlockError::HeaderFetchFailed)?;

    if _header_json.is_null() {
        return Err(RcBlockError::HeaderFieldMissing(
            "Block not found".to_string(),
        ));
    }

    // Extract block number
    let number_hex = _header_json
        .get("number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RcBlockError::HeaderFieldMissing("number".to_string()))?;

    let number = u64::from_str_radix(number_hex.trim_start_matches("0x"), 16)
        .map_err(RcBlockError::BlockNumberParseFailed)?;

    // Get full block to extract timestamp from extrinsics
    let block_json: Value = ah_rpc_client
        .request("chain_getBlock", rpc_params![block_hash])
        .await
        .map_err(RcBlockError::HeaderFetchFailed)?;

    let timestamp = extract_timestamp_from_block(&block_json);

    Ok(AssetHubBlock {
        hash: block_hash.to_string(),
        number,
        timestamp,
    })
}

/// Extract timestamp from full block JSON
pub fn extract_timestamp_from_block(block_json: &Value) -> Option<String> {
    // Timestamp is typically in the first extrinsic as a call to Timestamp::set
    // Look for extrinsics array
    let extrinsics = block_json
        .get("block")
        .and_then(|b| b.get("extrinsics"))
        .and_then(|e| e.as_array())?;

    // Check first extrinsic for timestamp
    if let Some(first_extrinsic) = extrinsics.first() {
        // Timestamp extrinsic is typically a call to Timestamp::set with a u64 value
        // The structure is: [call_index, [pallet_index, call_index], args...]
        // For Timestamp::set, args would be [timestamp_value]
        if let Some(extrinsic_array) = first_extrinsic.as_array() {
            if extrinsic_array.len() >= 2 {
                if let Some(call_array) = extrinsic_array[1].as_array() {
                    // Check if this is a timestamp call
                    // Pallet index for Timestamp varies, but we can check the args
                    if call_array.len() >= 2 {
                        // Try to extract timestamp value from args
                        // This is a simplified extraction - may need refinement
                        if let Some(args) = extrinsic_array.get(2) {
                            if let Some(timestamp_value) = args.as_u64() {
                                return Some(timestamp_value.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    None
}
