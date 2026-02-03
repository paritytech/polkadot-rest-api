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
/// Matches the pallet_broker::RegionRecord<AccountId, Balance> type.
/// DecodeAsType allows using `entry.value().decode_as::<RegionRecord>()` for efficient decoding.
///
/// On-chain structure (verified via debug logging):
/// - end: u32
/// - owner: Option<AccountId32> (the Option wrapper must be in our struct!)
/// - paid: Option<u128>
///
/// Note: DecodeAsType automatically handles:
/// - Number type conversions (e.g., u8 stored as u128 in scale_value)
/// - Newtype wrapper unwrapping (e.g., MyAccount(AccountId32) -> [u8; 32])
/// - Field matching by name or position
#[derive(Debug, Clone, Decode, Encode, DecodeAsType)]
struct RegionRecord {
    /// The end timeslice of this region.
    end: u32,
    /// The owner of this region (Option<AccountId32>).
    owner: Option<[u8; 32]>,
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
/// Uses typed DecodeAsType decoding for efficiency - no intermediate scale_value::Value.
async fn fetch_regions(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<Vec<RegionInfo>, CoretimeError> {
    let regions_addr = subxt::dynamic::storage::<(u32, u16, [u8; CORE_MASK_SIZE]), RegionRecord>(
        "Broker", "Regions",
    );

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

        // Decode RegionRecord directly using typed DecodeAsType
        // This is much more efficient than decoding to scale_value::Value first
        // DecodeAsType handles: number conversions, newtype unwrapping, field matching
        let record = match entry.value().decode_as::<RegionRecord>() {
            Ok(r) => Some(r),
            Err(e1) => {
                tracing::warn!("Failed to decode as RegionRecord: {:?}", e1);
                // Try decoding as Option<RegionRecord> (some runtimes wrap it)
                match entry.value().decode_as::<Option<RegionRecord>>() {
                    Ok(opt) => opt,
                    Err(e2) => {
                        tracing::warn!("Failed to decode as Option<RegionRecord>: {:?}", e2);
                        None
                    }
                }
            }
        };

        // Extract fields from RegionRecord
        // Note: owner is already Option<[u8; 32]> in the struct
        let (end, owner_bytes, paid) = match record {
            Some(r) => (Some(r.end), r.owner, r.paid),
            None => (None, None, None),
        };

        // Convert owner from bytes to SS58 format if available
        let owner_ss58 = owner_bytes.and_then(|bytes| {
            let hex_owner = format!("0x{}", hex::encode(bytes));
            decode_address_to_ss58(&hex_owner, ss58_prefix)
        });

        regions.push(RegionInfo {
            core: region_id.core as u32,
            begin: region_id.begin,
            end,
            owner: owner_ss58,
            paid: paid.map(|p| p.to_string()),
            mask: format!("0x{}", hex::encode(region_id.mask)),
        });
    }

    Ok(regions)
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
            owner: Some([0xAB; 32]),
            paid: Some(16168469809),
        };
        let encoded = original.encode();

        let decoded = RegionRecord::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded.end, 307725);
        assert_eq!(decoded.owner, Some([0xAB; 32]));
        assert_eq!(decoded.paid, Some(16168469809));
    }

    #[test]
    fn test_decode_region_record_without_paid() {
        let original = RegionRecord {
            end: 302685,
            owner: Some([0xCD; 32]),
            paid: None,
        };
        let encoded = original.encode();

        let decoded = RegionRecord::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded.end, 302685);
        assert_eq!(decoded.owner, Some([0xCD; 32]));
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
