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
use scale_value::ValueDef;
use serde::Serialize;
use std::str::FromStr;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

// ============================================================================
// Constants - Storage Key Layout
// ============================================================================

// Storage key format for Broker::Regions (Blake2_128Concat hasher):
// - 16 bytes: pallet prefix (twox128 of "Broker")
// - 16 bytes: entry prefix (twox128 of "Regions")
// - 16 bytes: blake2_128 hash of the key
// - Key bytes: RegionId (16 bytes)
//
// Total prefix before key data: 48 bytes

/// Offset where the RegionId starts in the storage key (after hash prefixes).
const KEY_PAYLOAD_OFFSET: usize = 48; // 16 + 16 + 16 (Blake2_128Concat)

// ============================================================================
// SCALE Decode Types
// ============================================================================

/// RegionId from the Broker pallet storage key.
/// Matches the pallet_broker::RegionId type.
#[derive(Debug, Clone, Decode, Encode)]
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

        // Extract RegionId from key bytes using SCALE decode
        let key_bytes = entry.key_bytes();
        let region_id = match decode_region_id(key_bytes) {
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

/// Decodes RegionId from the storage key bytes using SCALE codec.
fn decode_region_id(key_bytes: &[u8]) -> Option<RegionId> {
    if key_bytes.len() < KEY_PAYLOAD_OFFSET {
        return None;
    }

    let region_id_bytes = &key_bytes[KEY_PAYLOAD_OFFSET..];
    RegionId::decode(&mut &region_id_bytes[..]).ok()
}

/// Extracts RegionRecord fields from a scale_value::Value.
///
/// The storage value is Option<RegionRecord> where RegionRecord has:
/// - end: u32
/// - owner: Option<AccountId32>
/// - paid: Option<u128>
fn extract_region_record_fields<T>(
    value: &scale_value::Value<T>,
) -> (Option<u32>, Option<String>, Option<String>) {
    // The value is Option<RegionRecord>, so first unwrap the Option
    let inner_value = match &value.value {
        // If it's a Some variant, get the inner value
        ValueDef::Variant(variant) if variant.name == "Some" => match &variant.values {
            scale_value::Composite::Unnamed(vals) if !vals.is_empty() => &vals[0],
            _ => return (None, None, None),
        },
        // If it's directly a composite (not wrapped in Option), use it directly
        ValueDef::Composite(_) => value,
        // None variant or other - no record
        _ => return (None, None, None),
    };

    // Now extract fields from the RegionRecord
    match &inner_value.value {
        ValueDef::Composite(scale_value::Composite::Named(fields)) => {
            let end = fields
                .iter()
                .find(|(name, _)| name == "end")
                .and_then(|(_, val)| extract_u32(&val.value));

            let owner = fields
                .iter()
                .find(|(name, _)| name == "owner")
                .and_then(|(_, val)| extract_option_account_id(&val.value));

            let paid = fields
                .iter()
                .find(|(name, _)| name == "paid")
                .and_then(|(_, val)| extract_option_u128(&val.value));

            (end, owner, paid)
        }
        ValueDef::Composite(scale_value::Composite::Unnamed(fields)) => {
            let end = fields.get(0).and_then(|v| extract_u32(&v.value));
            let owner = fields
                .get(1)
                .and_then(|v| extract_option_account_id(&v.value));
            let paid = fields.get(2).and_then(|v| extract_option_u128(&v.value));
            (end, owner, paid)
        }
        _ => (None, None, None),
    }
}

/// Extract u32 from ValueDef (all integers are represented as U128)
fn extract_u32<T>(value: &ValueDef<T>) -> Option<u32> {
    match value {
        ValueDef::Primitive(scale_value::Primitive::U128(n)) => Some(*n as u32),
        _ => None,
    }
}

/// Extract Option<AccountId> from ValueDef (handles Some/None variants)
fn extract_option_account_id<T>(value: &ValueDef<T>) -> Option<String> {
    match value {
        ValueDef::Variant(variant) => {
            match variant.name.as_str() {
                "Some" => {
                    // Get the inner AccountId from Some variant
                    match &variant.values {
                        scale_value::Composite::Unnamed(vals) if !vals.is_empty() => {
                            extract_account_id_from_value(&vals[0])
                        }
                        scale_value::Composite::Named(fields) if !fields.is_empty() => {
                            extract_account_id_from_value(&fields[0].1)
                        }
                        _ => None,
                    }
                }
                _ => None, // None variant or other
            }
        }
        // If it's directly a composite (not wrapped in Option), try to extract
        ValueDef::Composite(_) => extract_account_id_from_value_def(value),
        _ => None,
    }
}

/// Extract AccountId from a scale_value::Value and format as hex
/// Handles multiple encoding formats
fn extract_account_id_from_value<T>(value: &scale_value::Value<T>) -> Option<String> {
    extract_account_id_from_value_def(&value.value)
}

/// Extract AccountId from ValueDef and format as hex
fn extract_account_id_from_value_def<T>(value: &ValueDef<T>) -> Option<String> {
    match value {
        // AccountId32 as an array of 32 U128 values (each representing a byte)
        ValueDef::Composite(scale_value::Composite::Unnamed(bytes)) => {
            let account_bytes: Vec<u8> = bytes
                .iter()
                .filter_map(|v| {
                    if let ValueDef::Primitive(scale_value::Primitive::U128(b)) = &v.value {
                        Some(*b as u8)
                    } else {
                        None
                    }
                })
                .collect();
            if account_bytes.len() == 32 {
                Some(format!("0x{}", hex::encode(account_bytes)))
            } else if bytes.len() == 1 {
                // If single element, might be nested
                extract_account_id_from_value(&bytes[0])
            } else {
                None
            }
        }
        // Named composite with an "Id" or similar field
        ValueDef::Composite(scale_value::Composite::Named(fields)) => {
            // Try common field names
            for field_name in ["Id", "id", "account"] {
                if let Some((_, val)) = fields.iter().find(|(name, _)| name == field_name) {
                    if let Some(account) = extract_account_id_from_value(val) {
                        return Some(account);
                    }
                }
            }
            // If only one field, try to extract from it
            if fields.len() == 1 {
                return extract_account_id_from_value(&fields[0].1);
            }
            None
        }
        // Handle variants like "Id" wrapping the account
        ValueDef::Variant(variant) => match variant.name.as_str() {
            "Some" | "Id" => match &variant.values {
                scale_value::Composite::Unnamed(vals) if !vals.is_empty() => {
                    extract_account_id_from_value(&vals[0])
                }
                scale_value::Composite::Named(fields) if !fields.is_empty() => {
                    extract_account_id_from_value(&fields[0].1)
                }
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// Extract Option<u128> from ValueDef (handles Some/None variants)
fn extract_option_u128<T>(value: &ValueDef<T>) -> Option<String> {
    match value {
        ValueDef::Variant(variant) => {
            if variant.name == "Some" {
                // Get the inner value from Some variant
                match &variant.values {
                    scale_value::Composite::Unnamed(vals) if !vals.is_empty() => {
                        extract_u128(&vals[0].value).map(|n| n.to_string())
                    }
                    _ => None,
                }
            } else {
                None // None variant
            }
        }
        _ => None,
    }
}

/// Extract u128 from ValueDef (all integers are represented as U128)
fn extract_u128<T>(value: &ValueDef<T>) -> Option<u128> {
    match value {
        ValueDef::Primitive(scale_value::Primitive::U128(n)) => Some(*n),
        _ => None,
    }
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

    #[test]
    fn test_decode_region_id_from_key_bytes() {
        // Simulate full storage key: 48 bytes prefix + RegionId
        let mut key_bytes = vec![0u8; KEY_PAYLOAD_OFFSET];

        // Append SCALE-encoded RegionId
        let region_id = RegionId {
            begin: 1000,
            core: 5,
            mask: [0xAA; CORE_MASK_SIZE],
        };
        key_bytes.extend_from_slice(&region_id.encode());

        let decoded = decode_region_id(&key_bytes).unwrap();
        assert_eq!(decoded.begin, 1000);
        assert_eq!(decoded.core, 5);
        assert_eq!(decoded.mask, [0xAA; CORE_MASK_SIZE]);
    }

    #[test]
    fn test_decode_region_id_insufficient_bytes() {
        let key_bytes = vec![0u8; KEY_PAYLOAD_OFFSET - 1];
        assert!(decode_region_id(&key_bytes).is_none());
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
