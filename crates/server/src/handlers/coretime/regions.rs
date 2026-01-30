//! Handler for /coretime/regions endpoint.
//!
//! Returns all regions registered on a coretime chain (parachain with Broker pallet).
//! Each region includes the core ID, begin timeslice, end timeslice, owner, paid amount,
//! and the CoreMask.
//!
//! Regions represent purchased coretime that can be traded or used.

use crate::handlers::coretime::common::{
    AtResponse,
    // Shared constants
    CORE_MASK_SIZE,
    CoretimeError,
    CoretimeQueryParams,
    // Shared functions
    has_broker_pallet,
};
use crate::state::AppState;
use crate::utils::{BlockId, decode_address_to_ss58, resolve_block};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use parity_scale_codec::{Decode, Encode};
use primitive_types::H256;
use scale_decode::DecodeAsType;
use scale_value::At;
use serde::Serialize;
use std::str::FromStr;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

// ============================================================================
// SCALE Decode Types
// ============================================================================

/// RegionId from the Broker pallet storage key.
/// Matches the pallet_broker::RegionId type.
/// DecodeAsType allows using subxt's `entry.key().part(0).decode_as::<RegionId>()`.
#[derive(Debug, Clone, Decode, Encode, DecodeAsType)]
struct RegionId {
    /// The begin timeslice of this region.
    begin: u32,
    /// The core index this region is for.
    core: u16,
    /// The CoreMask (80 bits = 10 bytes).
    mask: [u8; CORE_MASK_SIZE],
}

/// RegionRecord from the Broker pallet storage value.
/// Matches the pallet_broker::RegionRecord type.
#[derive(Debug, Clone, Decode, Encode)]
struct RegionRecord {
    /// The end timeslice of this region.
    end: u32,
    /// The owner of this region (AccountId32 = 32 bytes).
    owner: [u8; 32],
    /// The amount paid for this region (optional).
    paid: Option<u128>,
}

// ============================================================================
// Response Types
// ============================================================================

/// Information about a single region.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegionInfo {
    /// The core index this region is for.
    pub core: u32,
    /// The begin timeslice of this region.
    pub begin: u32,
    /// The end timeslice of this region (from RegionRecord, optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<u32>,
    /// The owner of this region (from RegionRecord, optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The amount paid for this region (from RegionRecord, optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paid: Option<String>,
    /// The CoreMask as a hex string (0x-prefixed).
    pub mask: String,
}

/// Response for GET /coretime/regions endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoretimeRegionsResponse {
    /// Block context (hash and height).
    pub at: AtResponse,
    /// List of regions with their info.
    pub regions: Vec<RegionInfo>,
}

// ============================================================================
// Main Handler
// ============================================================================

/// Handler for GET /coretime/regions endpoint.
///
/// Returns all regions registered on a coretime chain. Each region includes:
/// - core: The core index
/// - begin: The begin timeslice
/// - end: The end timeslice (if available)
/// - owner: The region owner (if available)
/// - paid: The amount paid for this region (if available)
/// - mask: The CoreMask as a hex string
///
/// Regions are sorted by core ID.
///
/// Query Parameters:
/// - at: Optional block number or hash to query at (defaults to latest finalized)
pub async fn coretime_regions(
    State(state): State<AppState>,
    Query(params): Query<CoretimeQueryParams>,
) -> Result<Response, CoretimeError> {
    // Parse the block ID if provided
    let block_id = match &params.at {
        None => None,
        Some(at_str) => Some(at_str.parse::<BlockId>()?),
    };

    // Resolve the block first to get a proper "Block not found" error
    // if the block doesn't exist (instead of a generic client error)
    let resolved_block = resolve_block(&state, block_id).await?;

    // Get client at the resolved block hash
    let block_hash =
        H256::from_str(&resolved_block.hash).map_err(|_| CoretimeError::InvalidBlockHash)?;
    let client_at_block = state.client.at_block(block_hash).await?;

    let at = AtResponse {
        hash: resolved_block.hash,
        height: resolved_block.number.to_string(),
    };

    // Verify that the Broker pallet exists at this block
    if !has_broker_pallet(&client_at_block) {
        return Err(CoretimeError::BrokerPalletNotFound);
    }

    // Fetch regions
    let mut regions = fetch_regions(&client_at_block, state.chain_info.ss58_prefix).await?;

    // Sort by core ID
    regions.sort_by_key(|r| r.core);

    Ok((
        StatusCode::OK,
        Json(CoretimeRegionsResponse { at, regions }),
    )
        .into_response())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Fetches all regions from Broker::Regions storage map.
///
/// Broker::Regions is a StorageMap with RegionId as key and RegionRecord as value.
async fn fetch_regions(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<Vec<RegionInfo>, CoretimeError> {
    let regions_addr = subxt::dynamic::storage::<
        (u32, u16, [u8; CORE_MASK_SIZE]),
        scale_value::Value,
    >("Broker", "Regions");

    let mut regions = Vec::new();

    // Iterate over all region entries
    let mut iter = client_at_block
        .storage()
        .iter(regions_addr, ())
        .await
        .map_err(|e| CoretimeError::StorageIterationError {
            pallet: "Broker",
            entry: "Regions",
            details: e.to_string(),
        })?;

    while let Some(result) = iter.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Error iterating regions: {:?}", e);
                continue;
            }
        };

        // Extract RegionId from storage key using subxt's structured key API
        // This automatically handles hasher-specific offsets (Blake2_128Concat, etc.)
        // Note: decode_as returns Result<Option<T>, Error>, so .ok().flatten() is needed
        let region_id = match entry
            .key()
            .ok()
            .and_then(|k| k.part(0))
            .and_then(|p| p.decode_as::<RegionId>().ok().flatten())
        {
            Some(id) => id,
            None => {
                tracing::warn!("Failed to decode RegionId from key");
                continue;
            }
        };

        // Extract RegionRecord fields from the scale_value::Value
        // Use decode_as() to get the decoded Value from StorageValue
        let (end, owner, paid) = match entry.value().decode_as::<scale_value::Value<()>>() {
            Ok(decoded) => extract_region_record_fields(&decoded),
            Err(e) => {
                tracing::warn!("Failed to decode RegionRecord: {:?}", e);
                (None, None, None)
            }
        };

        // Convert owner from hex to SS58 format if available
        let owner_ss58 =
            owner.and_then(|hex_owner| decode_address_to_ss58(&hex_owner, ss58_prefix));

        regions.push(RegionInfo {
            core: region_id.core as u32,
            begin: region_id.begin,
            end,
            owner: owner_ss58,
            paid,
            mask: format!("0x{}", hex::encode(region_id.mask)),
        });
    }

    Ok(regions)
}

/// Extracts RegionRecord fields from a scale_value::Value using the `At` trait.
///
/// The storage value is Option<RegionRecord> where RegionRecord has:
/// - end: u32
/// - owner: Option<AccountId32>
/// - paid: Option<u128>
///
/// Uses scale_value::At for cleaner field navigation instead of manual pattern matching.
fn extract_region_record_fields<T>(
    value: &scale_value::Value<T>,
) -> (Option<u32>, Option<String>, Option<String>) {
    // Handle Option<RegionRecord>:
    // - If value has "end" field directly, it's the RegionRecord itself
    // - If not, assume it's Option<RegionRecord> and unwrap with .at(0)
    //
    // We can't just do value.at(0).or(Some(value)) because if value is a Composite,
    // .at(0) returns the first FIELD (not the composite itself), which is wrong.
    let inner = if value.at("end").is_some() || value.at(0).and_then(|v| v.as_u128()).is_some() {
        // Value is the record itself (has "end" field or first field is a number)
        Some(value)
    } else {
        // Value is Option<RegionRecord>, unwrap the Some variant
        value.at(0)
    };

    // Extract 'end' field - integers are stored as U128 in scale_value
    let end = inner
        .at("end")
        .and_then(|v| v.as_u128())
        .map(|n| n as u32)
        // Fallback to positional access for unnamed composites
        .or_else(|| inner.at(0).and_then(|v| v.as_u128()).map(|n| n as u32));

    // Extract 'owner' field - Option<AccountId32>
    // First try named access, then positional
    let owner = inner
        .at("owner")
        .and_then(extract_option_account_id)
        .or_else(|| inner.at(1).and_then(extract_option_account_id));

    // Extract 'paid' field - Option<u128>
    // The value is Option<u128>, so .at(0) unwraps the Some variant
    let paid = inner
        .at("paid")
        .and_then(|v| v.at(0)) // unwrap Some variant
        .and_then(|v| v.as_u128())
        .map(|n| n.to_string())
        // Fallback to positional access
        .or_else(|| {
            inner
                .at(2)
                .and_then(|v| v.at(0))
                .and_then(|v| v.as_u128())
                .map(|n| n.to_string())
        });

    (end, owner, paid)
}

/// Extract Option<AccountId> from a Value (handles Some/None variants).
/// Returns the AccountId as a hex string (0x-prefixed).
fn extract_option_account_id<T>(value: &scale_value::Value<T>) -> Option<String> {
    // If it's a Some variant, get the inner AccountId at index 0
    // If it's directly the AccountId (not wrapped), try to extract directly
    let inner = value.at(0).or(Some(value));
    extract_account_id_bytes(inner).map(|bytes| format!("0x{}", hex::encode(bytes)))
}

/// Extract AccountId bytes from a Value.
/// AccountId32 is stored as an array of 32 U128 values (each representing a byte).
fn extract_account_id_bytes<T>(value: Option<&scale_value::Value<T>>) -> Option<Vec<u8>> {
    let value = value?;

    // Try to collect 32 bytes from unnamed composite (array of U128s)
    let bytes: Vec<u8> = (0..32)
        .filter_map(|i| value.at(i).and_then(|v| v.as_u128()).map(|n| n as u8))
        .collect();

    if bytes.len() == 32 {
        return Some(bytes);
    }

    // Try named field access for wrapped AccountId types (e.g., "Id" field)
    for field_name in ["Id", "id", "account"] {
        if let Some(bytes) = extract_account_id_bytes(value.at(field_name)) {
            return Some(bytes);
        }
    }

    // Try positional access (index 0) for single-element wrappers
    if let Some(bytes) = extract_account_id_bytes(value.at(0)) {
        return Some(bytes);
    }

    None
}

/// Decodes RegionRecord from the storage value bytes using SCALE codec.
#[allow(dead_code)]
fn decode_region_record(bytes: &[u8]) -> Option<RegionRecord> {
    if bytes.is_empty() {
        return None;
    }

    RegionRecord::decode(&mut &bytes[..]).ok()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // RegionId decode tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_decode_region_id_valid() {
        // Create RegionId and encode it
        let original = RegionId {
            begin: 302685,
            core: 48,
            mask: [0xFF; CORE_MASK_SIZE],
        };
        let encoded = original.encode();

        // Decode it back
        let decoded = RegionId::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded.begin, 302685);
        assert_eq!(decoded.core, 48);
        assert_eq!(decoded.mask, [0xFF; CORE_MASK_SIZE]);
    }

    // ------------------------------------------------------------------------
    // RegionRecord decode tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_decode_region_record_with_paid() {
        let original = RegionRecord {
            end: 307725,
            owner: [0xAB; 32],
            paid: Some(16168469809),
        };
        let encoded = original.encode();

        let decoded = RegionRecord::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded.end, 307725);
        assert_eq!(decoded.owner, [0xAB; 32]);
        assert_eq!(decoded.paid, Some(16168469809));
    }

    #[test]
    fn test_decode_region_record_without_paid() {
        let original = RegionRecord {
            end: 302685,
            owner: [0xCD; 32],
            paid: None,
        };
        let encoded = original.encode();

        let decoded = RegionRecord::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded.end, 302685);
        assert_eq!(decoded.owner, [0xCD; 32]);
        assert_eq!(decoded.paid, None);
    }

    #[test]
    fn test_decode_region_record_empty() {
        assert!(decode_region_record(&[]).is_none());
    }

    #[test]
    fn test_decode_region_record_invalid() {
        // Not enough bytes for a valid RegionRecord
        let bytes = vec![0x00, 0x01];
        assert!(decode_region_record(&bytes).is_none());
    }

    // ------------------------------------------------------------------------
    // RegionInfo tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_region_info_serialization_full() {
        let info = RegionInfo {
            core: 48,
            begin: 302685,
            end: Some(307725),
            owner: Some("0xabcd".to_string()),
            paid: Some("16168469809".to_string()),
            mask: "0xffffffffffffffffffff".to_string(),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"core\":48"));
        assert!(json.contains("\"begin\":302685"));
        assert!(json.contains("\"end\":307725"));
        assert!(json.contains("\"owner\":\"0xabcd\""));
        assert!(json.contains("\"paid\":\"16168469809\""));
        assert!(json.contains("\"mask\":\"0xffffffffffffffffffff\""));
    }

    #[test]
    fn test_region_info_serialization_minimal() {
        let info = RegionInfo {
            core: 48,
            begin: 302685,
            end: None,
            owner: None,
            paid: None,
            mask: "0xffffffffffffffffffff".to_string(),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"core\":48"));
        assert!(json.contains("\"begin\":302685"));
        assert!(json.contains("\"mask\":\"0xffffffffffffffffffff\""));
        // Optional fields should be skipped
        assert!(!json.contains("\"end\""));
        assert!(!json.contains("\"owner\""));
        assert!(!json.contains("\"paid\""));
    }

    #[test]
    fn test_region_info_equality() {
        let a = RegionInfo {
            core: 48,
            begin: 302685,
            end: Some(307725),
            owner: Some("0xabc".to_string()),
            paid: None,
            mask: "0xff".to_string(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    // ------------------------------------------------------------------------
    // Response serialization tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_regions_response_serialization() {
        let response = CoretimeRegionsResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "12345".to_string(),
            },
            regions: vec![
                RegionInfo {
                    core: 48,
                    begin: 302685,
                    end: Some(307725),
                    owner: Some("0xowner1".to_string()),
                    paid: Some("16168469809".to_string()),
                    mask: "0xffffffffffffffffffff".to_string(),
                },
                RegionInfo {
                    core: 51,
                    begin: 287565,
                    end: None,
                    owner: None,
                    paid: None,
                    mask: "0xffffffffffffffffffff".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"regions\""));
        assert!(json.contains("\"hash\":\"0xabc123\""));
        assert!(json.contains("\"height\":\"12345\""));
    }

    // ------------------------------------------------------------------------
    // Sorting tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_regions_sorting_by_core() {
        let mut regions = vec![
            RegionInfo {
                core: 52,
                begin: 100,
                end: None,
                owner: None,
                paid: None,
                mask: "0xff".to_string(),
            },
            RegionInfo {
                core: 48,
                begin: 100,
                end: None,
                owner: None,
                paid: None,
                mask: "0xff".to_string(),
            },
            RegionInfo {
                core: 51,
                begin: 100,
                end: None,
                owner: None,
                paid: None,
                mask: "0xff".to_string(),
            },
        ];

        regions.sort_by_key(|r| r.core);

        assert_eq!(regions[0].core, 48);
        assert_eq!(regions[1].core, 51);
        assert_eq!(regions[2].core, 52);
    }
}
