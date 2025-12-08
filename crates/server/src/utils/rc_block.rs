use parity_scale_codec::{Compact, Decode};
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

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(String),

    #[error("Failed to fetch storage: {0}")]
    StorageFetchFailed(String),
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
    /// Relay Chain block hash where this AH block was included
    pub rc_block_hash: String,
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

/// Block header response for useRcBlock=true
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockHeaderRcResponse {
    pub parent_hash: String,
    pub number: String,
    pub state_root: String,
    pub extrinsics_root: String,
    pub digest: DigestInfo,
    pub ah_timestamp: String,
}

/// Response wrapper for useRcBlock=true with RC block info and parachains array
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlockWithParachainsResponse<T> {
    pub rc_block_hash: String,
    pub rc_block_parent_hash: String,
    pub rc_block_number: String,
    pub parachains: Vec<T>,
}

/// Type alias for header responses
pub type RcBlockHeaderWithParachainsResponse = RcBlockWithParachainsResponse<BlockHeaderRcResponse>;

/// Type alias for full block responses
pub type RcBlockFullWithParachainsResponse = RcBlockWithParachainsResponse<BlockRcResponse>;

/// Block response for useRcBlock=true
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnInitializeFinalize {
    pub events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRcResponse {
    pub number: String,
    pub hash: String,
    pub parent_hash: String,
    pub state_root: String,
    pub extrinsics_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    pub logs: Vec<crate::handlers::blocks::DigestLog>,
    pub on_initialize: OnInitializeFinalize,
    pub extrinsics: Vec<crate::handlers::blocks::ExtrinsicInfo>,
    pub on_finalize: OnInitializeFinalize,
    pub finalized: bool,
    pub ah_timestamp: String,
}

/// Runtime spec response for useRcBlock=true
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpecRcResponse {
    pub at: BlockInfo,
    pub authoring_version: String,
    pub chain_type: serde_json::Value,
    pub impl_version: String,
    pub spec_name: String,
    pub spec_version: String,
    pub transaction_version: String,
    pub properties: serde_json::Value,
    pub rc_block_hash: String,
    pub rc_block_number: String,
    pub ah_timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct DigestInfo {
    pub logs: Vec<DigestLog>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestLog {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_runtime: Option<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consensus: Option<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seal: Option<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
}

/// Find Asset Hub blocks that correspond to a specific Relay Chain block number
///
/// This function queries the Relay Chain block's events to find all Asset Hub blocks
/// that were included at that specific relay chain block. With elastic scaling, there may be
/// multiple Asset Hub blocks per relay chain block.
///
/// it uses subxt-historic to query system events
/// in the RC block, decode them properly using the runtime metadata, and filter for
/// `paraInclusion.CandidateIncluded` events for Asset Hub (paraId 1000).
pub async fn find_ah_blocks_by_rc_block(
    rc_client: &OnlineClient<SubstrateConfig>,
    rc_rpc_client: &RpcClient,
    rc_block_number: u64,
) -> Result<Vec<AssetHubBlock>, RcBlockError> {
    const ASSET_HUB_PARA_ID: u32 = 1000;
    
    let rc_block_hash: Option<String> = rc_rpc_client
        .request("chain_getBlockHash", rpc_params![rc_block_number])
        .await
        .map_err(RcBlockError::AssetHubQueryFailed)?;
    
    let rc_block_hash = rc_block_hash.ok_or_else(|| {
        RcBlockError::HeaderFieldMissing(format!("RC block {} not found", rc_block_number))
    })?;
    
    tracing::info!("Querying events for RC block {} (hash: {})", rc_block_number, rc_block_hash);
    
    // Get client at the specific block height
    let client_at_block = rc_client
        .at(rc_block_number)
        .await
        .map_err(|e| RcBlockError::ClientAtBlockFailed(format!("{:?}", e)))?;
    
    // Query System::Events storage using subxt-historic
    let storage_entry = client_at_block
        .storage()
        .entry("System", "Events")
        .map_err(|e| RcBlockError::StorageFetchFailed(format!("Failed to get storage entry: {:?}", e)))?;
    
    let plain_entry = storage_entry
        .into_plain()
        .map_err(|e| RcBlockError::StorageFetchFailed(format!("Storage entry is not plain: {:?}", e)))?;
    
    let events_value = plain_entry
        .fetch()
        .await
        .map_err(|e| RcBlockError::StorageFetchFailed(format!("Failed to fetch events: {:?}", e)))?
        .ok_or_else(|| RcBlockError::HeaderFieldMissing("Events not found".to_string()))?;
    
    let events_decoded: scale_value::Value<()> = events_value
        .decode()
        .map_err(|e| RcBlockError::EventsQueryFailed(format!("Failed to decode events: {:?}", e)))?;
    
    // Extract Asset Hub blocks from CandidateIncluded events
    let ah_blocks = extract_ah_blocks_from_events(&events_decoded, ASSET_HUB_PARA_ID, &rc_block_hash)?;
    
    tracing::info!("Found {} Asset Hub blocks in RC block {}", ah_blocks.len(), rc_block_number);
    
    Ok(ah_blocks)
}

/// Extract Asset Hub blocks from decoded events
fn extract_ah_blocks_from_events(
    events: &scale_value::Value<()>,
    target_para_id: u32,
    rc_block_hash: &str,
) -> Result<Vec<AssetHubBlock>, RcBlockError> {
    use scale_value::{ValueDef, Composite};
    
    let mut ah_blocks = Vec::new();
    
    let events_composite = match &events.value {
        ValueDef::Composite(composite) => composite,
        _ => {
            tracing::warn!("Events is not a composite type");
            return Ok(ah_blocks);
        }
    };
    
    let events_values = match events_composite {
        Composite::Unnamed(values) => values,
        Composite::Named(_) => {
            tracing::warn!("Events is named composite, expected unnamed");
            return Ok(ah_blocks);
        }
    };
    
    tracing::info!("Processing {} events", events_values.len());
    
    for (idx, event_record) in events_values.iter().enumerate() {
        let record_composite = match &event_record.value {
            ValueDef::Composite(c) => c,
            _ => continue,
        };
        
        let event_value = match record_composite {
            Composite::Named(fields) => {
                fields.iter()
                    .find(|(name, _)| name == "event")
                    .map(|(_, v)| v)
            }
            Composite::Unnamed(values) => values.get(1), // event is typically second field
        };
        
        let event = match event_value {
            Some(v) => v,
            None => continue,
        };
        
        let event_variant = match &event.value {
            ValueDef::Variant(variant) => variant,
            _ => continue,
        };
        
        let pallet_name = &event_variant.name;
        
        // Only process paraInclusion events
        if !(pallet_name.to_lowercase().contains("parainclusion") 
            || pallet_name.to_lowercase().contains("para_inclusion")
            || pallet_name == "ParaInclusion"
            || pallet_name == "paraInclusion") {
            continue;
        }
        
        tracing::info!("Found paraInclusion event at index {}, pallet_name: {}", idx, pallet_name);

        let (event_name, event_data) = match &event_variant.values {
            Composite::Unnamed(values) => {
                let first_val = match values.first() {
                    Some(v) => v,
                    None => continue,
                };
                match &first_val.value {
                    ValueDef::Variant(inner_variant) => {
                        (inner_variant.name.clone(), &inner_variant.values)
                    }
                    _ => {
                        ("Unknown".to_string(), &event_variant.values)
                    }
                }
            }
            Composite::Named(fields) => {
                let (name, val) = match fields.first() {
                    Some((n, v)) => (n, v),
                    None => continue,
                };
                match &val.value {
                    ValueDef::Variant(inner_variant) => {
                        (inner_variant.name.clone(), &inner_variant.values)
                    }
                    _ => {
                        (name.clone(), &event_variant.values)
                    }
                }
            }
        };
        
        if event_name != "CandidateIncluded" {
            continue;
        }
        
        if let Some(ah_block) = extract_ah_block_from_candidate_included(event_data, target_para_id, rc_block_hash) {
            tracing::info!("Extracted AH block: number={}, hash={}", ah_block.number, ah_block.hash);
            ah_blocks.push(ah_block);
        }
    }
    
    Ok(ah_blocks)
}

fn extract_ah_block_from_candidate_included(
    event_data: &scale_value::Composite<()>,
    target_para_id: u32,
    rc_block_hash: &str,
) -> Option<AssetHubBlock> {
    use sp_runtime::traits::BlakeTwo256;
    use sp_runtime::traits::Hash as HashT;
    use scale_value::Composite;
    
    let values: Vec<&scale_value::Value<()>> = match event_data {
        Composite::Named(fields) => fields.iter().map(|(_, v)| v).collect(),
        Composite::Unnamed(values) => values.iter().collect(),
    };
    
    if values.len() < 2 {
        return None;
    }

    let candidate_receipt = values.first()?;
    
    let para_id = match extract_para_id_from_receipt(candidate_receipt) {
        Some(id) => {
            id
        }
        None => {
            return None;
        }
    };
    
    if para_id != target_para_id {
        return None;
    }
    
    tracing::info!("Found CandidateIncluded for target para_id {}", target_para_id);
    
    let head_data = values.get(1)?;
    
    tracing::info!("Extracting header bytes from HeadData");
    let header_bytes = match serde_json::to_value(head_data)
        .ok()
        .and_then(|json| extract_bytes_from_json(&json))
    {
        Some(bytes) => {
            tracing::info!("Extracted {} bytes from HeadData", bytes.len());
            bytes
        }
        None => {
            tracing::warn!("Failed to extract bytes from HeadData - value structure may be different");
            return None;
        }
    };
    
    // Extract block number from header
    let block_number = extract_block_number_from_header(&header_bytes)?;

    let block_hash = BlakeTwo256::hash(&header_bytes);
    let block_hash_hex = format!("0x{}", hex::encode(block_hash.as_ref()));
    
    tracing::info!("Extracted AH block from CandidateIncluded: number={}, hash={}", 
        block_number, block_hash_hex);
    
    Some(AssetHubBlock {
        hash: block_hash_hex,
        number: block_number,
        timestamp: None,
        rc_block_hash: rc_block_hash.to_string(),
    })
}

fn extract_para_id_from_receipt(receipt: &scale_value::Value<()>) -> Option<u32> {
    let receipt_composite = as_composite(receipt)?;
    
    let descriptor_value = get_field_from_composite(
        receipt_composite,
        &["descriptor"],
        Some(0)
    )?;
    
    let descriptor_composite = as_composite(descriptor_value)?;
    
    let para_id_value = get_field_from_composite(
        descriptor_composite,
        &["para_id", "paraId"],
        Some(0) 
    )?;
    
    // Extract u32 using standard serde_json conversion
    serde_json::to_value(para_id_value)
        .ok()
        .and_then(|json| {
            json.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .or_else(|| {
                    json.as_array()
                        .and_then(|arr| arr.first())
                        .and_then(|first| first.as_u64())
                        .and_then(|n| u32::try_from(n).ok())
                })
                .or_else(|| {
                    json.as_object()
                        .and_then(|obj| obj.values().next())
                        .and_then(|val| val.as_u64())
                        .and_then(|n| u32::try_from(n).ok())
                })
        })
}


pub(crate) fn as_composite(value: &scale_value::Value<()>) -> Option<&scale_value::Composite<()>> {
    use scale_value::ValueDef;
    match &value.value {
        ValueDef::Composite(c) => Some(c),
        _ => None,
    }
}

pub(crate) fn get_field_from_composite<'a>(
    composite: &'a scale_value::Composite<()>,
    field_names: &[&str],
    unnamed_index: Option<usize>,
) -> Option<&'a scale_value::Value<()>> {
    match composite {
        scale_value::Composite::Named(fields) => {
            fields.iter()
                .find(|(name, _)| field_names.iter().any(|&n| n == *name))
                .map(|(_, v)| v)
        }
        scale_value::Composite::Unnamed(values) => {
            unnamed_index.and_then(|idx| values.get(idx))
        }
    }
}

fn extract_bytes_from_json(json: &serde_json::Value) -> Option<Vec<u8>> {
    match json {
        serde_json::Value::Array(arr) => {
            let bytes: Vec<u8> = arr
                .iter()
                .filter_map(|v| {
                    v.as_u64()
                        .and_then(|n| (n <= 255).then_some(n as u8))
                })
                .collect();
            
            if !bytes.is_empty() {
                return Some(bytes);
            }
            
            // If single element, recurse
            if arr.len() == 1 {
                return extract_bytes_from_json(&arr[0]);
            }
            
            None
        }
        serde_json::Value::Object(map) => {
            for field_name in ["data", "bytes", "0"] {
                if let Some(field_value) = map.get(field_name) {
                    if let Some(bytes) = extract_bytes_from_json(field_value) {
                        return Some(bytes);
                    }
                }
            }
            map.values().next().and_then(extract_bytes_from_json)
        }
        _ => None,
    }
}

fn extract_block_number_from_header(header_bytes: &[u8]) -> Option<u64> {
    use sp_core::H256;
    
    let mut cursor = &header_bytes[..];
    
    let _parent_hash = H256::decode(&mut cursor).ok()?;
    
    // Now decode the block number (Compact<u64>)
    Compact::<u64>::decode(&mut cursor)
        .ok()
        .map(|compact| compact.0)
}


/// Get Asset Hub block with full information including timestamp
pub async fn get_ah_block_with_timestamp(
    ah_rpc_client: &RpcClient,
    block_hash: &str,
    rc_block_hash: &str,
) -> Result<AssetHubBlock, RcBlockError> {
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

    // Get timestamp from storage query
    let timestamp = get_timestamp_from_storage(ah_rpc_client, block_hash).await;

    Ok(AssetHubBlock {
        hash: block_hash.to_string(),
        number,
        timestamp,
        rc_block_hash: rc_block_hash.to_string(),
    })
}

/// Get Relay Chain block header information (hash, parent hash, number)
pub async fn get_rc_block_header_info(
    rc_rpc_client: &RpcClient,
    rc_block_number: u64,
) -> Result<(String, String, String), RcBlockError> {
    // Get RC block hash
    let rc_block_hash: Option<String> = rc_rpc_client
        .request("chain_getBlockHash", rpc_params![rc_block_number])
        .await
        .map_err(RcBlockError::AssetHubQueryFailed)?;
    
    let rc_block_hash = rc_block_hash.ok_or_else(|| {
        RcBlockError::HeaderFieldMissing(format!("RC block {} not found", rc_block_number))
    })?;
    
    // Get RC block header to extract parent hash
    let header_json: serde_json::Value = rc_rpc_client
        .request("chain_getHeader", rpc_params![rc_block_hash.clone()])
        .await
        .map_err(RcBlockError::HeaderFetchFailed)?;
    
    let rc_block_parent_hash = header_json
        .get("parentHash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            RcBlockError::HeaderFieldMissing("parentHash".to_string())
        })?
        .to_string();
    
    Ok((
        rc_block_hash,
        rc_block_parent_hash,
        rc_block_number.to_string(),
    ))
}

/// Get timestamp from Timestamp::Now storage at a specific block
pub async fn get_timestamp_from_storage(
    rpc_client: &RpcClient,
    block_hash: &str,
) -> Option<String> {
    // Storage key for Timestamp::Now is: twox128("Timestamp") ++ twox128("Now")
    // Pre-computed: 0xf0c365c3cf59d671eb72da0e7a4113c49f1f0515f462cdcf84e0f1d6045dfcbb
    let timestamp_key = "0xf0c365c3cf59d671eb72da0e7a4113c49f1f0515f462cdcf84e0f1d6045dfcbb";
    
    let timestamp_hex: Option<String> = rpc_client
        .request("state_getStorage", rpc_params![timestamp_key, block_hash])
        .await
        .ok()?;
    
    let timestamp_hex = timestamp_hex?;
    
    let timestamp_bytes = hex::decode(timestamp_hex.trim_start_matches("0x")).ok()?;
    
    if timestamp_bytes.len() >= 8 {
        let timestamp = u64::from_le_bytes(timestamp_bytes[..8].try_into().ok()?);
        Some(timestamp.to_string())
    } else {
        None
    }
}
