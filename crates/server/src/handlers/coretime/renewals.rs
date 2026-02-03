//! Handler for /coretime/renewals endpoint.
//!
//! Returns all potential renewals registered on a coretime chain (parachain with Broker pallet).
//! Each renewal includes the core ID, timeslice when it can be renewed, price, completion status,
//! mask, and task assignment info.
//!
//! Potential renewals represent coretime allocations that can be renewed by the holder
//! before the next sale period begins.

use crate::handlers::coretime::common::{
    AtResponse, CoreAssignment, CoreMask, CoretimeError, CoretimeQueryParams,
    STORAGE_KEY_DATA_OFFSET, has_broker_pallet, is_storage_item_not_found_error,
};
use crate::state::AppState;
use crate::utils::{BlockId, resolve_block};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use parity_scale_codec::Decode;
use primitive_types::H256;
use serde::Serialize;
use std::str::FromStr;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

// ============================================================================
// Storage Key Constants (PotentialRenewals-specific)
// ============================================================================

// Storage key format for Broker::PotentialRenewals:
// - 16 bytes: pallet prefix (xxhash128 of "Broker")
// - 16 bytes: entry prefix (xxhash128 of "PotentialRenewals")
// - 8 bytes: twox64 hash of the key
// - 2 bytes: core index (u16, little-endian SCALE encoded)
// - 4 bytes: when timeslice (u32, little-endian SCALE encoded)

/// Size of the key data portion (u16 + u32 = 6 bytes).
const KEY_DATA_SIZE: usize = std::mem::size_of::<u16>() + std::mem::size_of::<u32>();

/// Minimum length of the storage key to extract renewal ID fields.
const STORAGE_KEY_MIN_LENGTH: usize = STORAGE_KEY_DATA_OFFSET + KEY_DATA_SIZE;

// ============================================================================
// SCALE Decode Types (matching Broker pallet types)
// ============================================================================

/// CoreAssignment enum matching the Broker pallet.
/// Decoded automatically by subxt using DecodeAsType.
#[derive(Debug, Clone, scale_decode::DecodeAsType)]
enum ScaleCoreAssignment {
    Idle,
    Pool,
    Task(u32),
}

impl From<ScaleCoreAssignment> for CoreAssignment {
    fn from(scale: ScaleCoreAssignment) -> Self {
        match scale {
            ScaleCoreAssignment::Idle => CoreAssignment::Idle,
            ScaleCoreAssignment::Pool => CoreAssignment::Pool,
            ScaleCoreAssignment::Task(id) => CoreAssignment::Task(id),
        }
    }
}

/// ScheduleItem matching the Broker pallet.
#[derive(Debug, Clone, scale_decode::DecodeAsType)]
struct ScaleScheduleItem {
    mask: CoreMask,
    assignment: ScaleCoreAssignment,
}

/// CompletionStatus enum matching the Broker pallet.
#[derive(Debug, Clone, scale_decode::DecodeAsType)]
enum ScaleCompletionStatus {
    Partial(CoreMask),
    Complete(Vec<ScaleScheduleItem>),
}

/// PotentialRenewalRecord matching the Broker pallet.
/// This is the storage value type for Broker::PotentialRenewals.
#[derive(Debug, Clone, scale_decode::DecodeAsType)]
struct ScalePotentialRenewalRecord {
    price: u128,
    completion: ScaleCompletionStatus,
}

// ============================================================================
// Response Types
// ============================================================================

/// Information about a single potential renewal.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenewalInfo {
    /// The completion status type ("Complete" or "Partial").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    /// The core index this renewal applies to.
    pub core: u32,
    /// The CoreMask as a hex string (0x-prefixed), or null if not available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    /// The renewal price in plancks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// The task assignment: task ID as string, "Pool", "Idle", or empty string.
    pub task: String,
    /// The timeslice when this renewal becomes available.
    pub when: u32,
}

/// Response for GET /coretime/renewals endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoretimeRenewalsResponse {
    /// Block context (hash and height).
    pub at: AtResponse,
    /// List of potential renewals sorted by core.
    pub renewals: Vec<RenewalInfo>,
}

// ============================================================================
// Main Handler
// ============================================================================

/// Handler for GET /coretime/renewals endpoint.
///
/// Returns all potential renewals registered on a coretime chain. Each renewal includes:
/// - core: The core index this renewal applies to
/// - when: The timeslice when this renewal becomes available
/// - price: The renewal price in plancks
/// - completion: The completion status type ("Complete" or "Partial")
/// - mask: The CoreMask as a hex string
/// - task: The task assignment (task ID, "Pool", "Idle", or empty)
///
/// Potential renewals are sorted by core index.
///
/// Query Parameters:
/// - at: Optional block number or hash to query at (defaults to latest finalized)
pub async fn coretime_renewals(
    State(state): State<AppState>,
    Query(params): Query<CoretimeQueryParams>,
) -> Result<Response, CoretimeError> {
    // Parse the block ID if provided
    let block_id = match &params.at {
        None => None,
        Some(at_str) => Some(at_str.parse::<BlockId>()?),
    };

    // Resolve the block
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

    // Fetch potential renewals
    let mut renewals = fetch_potential_renewals(&client_at_block).await?;

    // Sort by core index
    renewals.sort_by_key(|r| r.core);

    Ok((
        StatusCode::OK,
        Json(CoretimeRenewalsResponse { at, renewals }),
    )
        .into_response())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Fetches all potential renewals from Broker::PotentialRenewals storage map.
async fn fetch_potential_renewals(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<RenewalInfo>, CoretimeError> {
    // Use typed dynamic storage with DecodeAsType for automatic decoding
    let renewals_addr = subxt::dynamic::storage::<(u16, u32), ScalePotentialRenewalRecord>(
        "Broker",
        "PotentialRenewals",
    );

    let mut renewals = Vec::new();

    // Iterate over all potential renewal entries
    let mut iter = client_at_block
        .storage()
        .iter(renewals_addr, ())
        .await
        .map_err(|e| {
            // Check if this is a "storage item not found" error, which means
            // PotentialRenewals didn't exist at this block (added in a later runtime upgrade)
            if is_storage_item_not_found_error(&e) {
                CoretimeError::StorageItemNotAvailableAtBlock {
                    pallet: "Broker",
                    entry: "PotentialRenewals",
                }
            } else {
                CoretimeError::StorageIterationError {
                    pallet: "Broker",
                    entry: "PotentialRenewals",
                    details: e.to_string(),
                }
            }
        })?;

    while let Some(result) = iter.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Error iterating potential renewals: {:?}", e);
                continue;
            }
        };

        // Extract key fields (core, when) from storage key bytes
        let key_bytes = entry.key_bytes();
        let Some((core, when)) = extract_renewal_key(key_bytes) else {
            tracing::warn!("PotentialRenewals key too short: {} bytes", key_bytes.len());
            continue;
        };

        // Decode the storage value
        let record = match entry.value().decode() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "Failed to decode PotentialRenewalRecord for core={}, when={}: {:?}",
                    core,
                    when,
                    e
                );
                continue;
            }
        };

        // Convert to response format
        let renewal_info = convert_to_renewal_info(core, when, &record);
        renewals.push(renewal_info);
    }

    Ok(renewals)
}

/// Extracts (core, when) from storage key bytes using SCALE decoding.
fn extract_renewal_key(key_bytes: &[u8]) -> Option<(u32, u32)> {
    if key_bytes.len() < STORAGE_KEY_MIN_LENGTH {
        return None;
    }

    // Position cursor at the start of the key data (after pallet hash + entry hash + twox64)
    let cursor = &mut &key_bytes[STORAGE_KEY_DATA_OFFSET..];

    // Decode core (u16) and when (u32) using SCALE codec
    let core = u16::decode(cursor).ok()? as u32;
    let when = u32::decode(cursor).ok()?;

    Some((core, when))
}

/// Converts a ScalePotentialRenewalRecord to the API response RenewalInfo.
fn convert_to_renewal_info(
    core: u32,
    when: u32,
    record: &ScalePotentialRenewalRecord,
) -> RenewalInfo {
    let (completion_type, mask, task) = match &record.completion {
        ScaleCompletionStatus::Complete(items) => {
            if let Some(first_item) = items.first() {
                let mask_hex = format!("0x{}", hex::encode(first_item.mask));
                let assignment: CoreAssignment = first_item.assignment.clone().into();
                let task_str = match &assignment {
                    CoreAssignment::Idle => "Idle".to_string(),
                    CoreAssignment::Pool => "Pool".to_string(),
                    CoreAssignment::Task(id) => id.to_string(),
                };
                (Some("Complete".to_string()), Some(mask_hex), task_str)
            } else {
                (Some("Complete".to_string()), None, String::new())
            }
        }
        ScaleCompletionStatus::Partial(mask) => {
            let mask_hex = format!("0x{}", hex::encode(mask));
            (Some("Partial".to_string()), Some(mask_hex), String::new())
        }
    };

    RenewalInfo {
        completion: completion_type,
        core,
        mask,
        price: Some(record.price.to_string()),
        task,
        when,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::coretime::common::CORE_MASK_SIZE;
    use parity_scale_codec::Encode;

    /// Helper to create a mock storage key with the given core and when values.
    fn make_storage_key(core: u16, when: u32) -> Vec<u8> {
        let mut key_bytes = vec![0u8; STORAGE_KEY_DATA_OFFSET];
        // Append SCALE-encoded core and when
        key_bytes.extend(core.encode());
        key_bytes.extend(when.encode());
        key_bytes
    }

    // ------------------------------------------------------------------------
    // extract_renewal_key tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_extract_renewal_key_valid() {
        let key_bytes = make_storage_key(5, 1000);
        let result = extract_renewal_key(&key_bytes);
        assert_eq!(result, Some((5, 1000)));
    }

    #[test]
    fn test_extract_renewal_key_large_core() {
        let key_bytes = make_storage_key(1000, 50000);
        let result = extract_renewal_key(&key_bytes);
        assert_eq!(result, Some((1000, 50000)));
    }

    #[test]
    fn test_extract_renewal_key_too_short() {
        let key_bytes = vec![0u8; STORAGE_KEY_MIN_LENGTH - 1];
        assert_eq!(extract_renewal_key(&key_bytes), None);
    }

    #[test]
    fn test_extract_renewal_key_empty() {
        assert_eq!(extract_renewal_key(&[]), None);
    }

    // ------------------------------------------------------------------------
    // convert_to_renewal_info tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_convert_to_renewal_info_complete_task() {
        let record = ScalePotentialRenewalRecord {
            price: 1_000_000_000_000,
            completion: ScaleCompletionStatus::Complete(vec![ScaleScheduleItem {
                mask: [0xFF; CORE_MASK_SIZE],
                assignment: ScaleCoreAssignment::Task(2000),
            }]),
        };

        let info = convert_to_renewal_info(5, 1234, &record);
        assert_eq!(info.core, 5);
        assert_eq!(info.when, 1234);
        assert_eq!(info.price, Some("1000000000000".to_string()));
        assert_eq!(info.completion, Some("Complete".to_string()));
        assert_eq!(info.mask, Some("0xffffffffffffffffffff".to_string()));
        assert_eq!(info.task, "2000");
    }

    #[test]
    fn test_convert_to_renewal_info_complete_pool() {
        let record = ScalePotentialRenewalRecord {
            price: 500_000_000_000,
            completion: ScaleCompletionStatus::Complete(vec![ScaleScheduleItem {
                mask: [0xAA; CORE_MASK_SIZE],
                assignment: ScaleCoreAssignment::Pool,
            }]),
        };

        let info = convert_to_renewal_info(3, 5678, &record);
        assert_eq!(info.core, 3);
        assert_eq!(info.task, "Pool");
        assert_eq!(info.completion, Some("Complete".to_string()));
    }

    #[test]
    fn test_convert_to_renewal_info_complete_idle() {
        let record = ScalePotentialRenewalRecord {
            price: 100_000_000_000,
            completion: ScaleCompletionStatus::Complete(vec![ScaleScheduleItem {
                mask: [0xFF; CORE_MASK_SIZE],
                assignment: ScaleCoreAssignment::Idle,
            }]),
        };

        let info = convert_to_renewal_info(1, 100, &record);
        assert_eq!(info.task, "Idle");
    }

    #[test]
    fn test_convert_to_renewal_info_partial() {
        let record = ScalePotentialRenewalRecord {
            price: 200_000_000_000,
            completion: ScaleCompletionStatus::Partial([0xBB; CORE_MASK_SIZE]),
        };

        let info = convert_to_renewal_info(2, 200, &record);
        assert_eq!(info.core, 2);
        assert_eq!(info.when, 200);
        assert_eq!(info.completion, Some("Partial".to_string()));
        assert_eq!(info.mask, Some("0xbbbbbbbbbbbbbbbbbbbb".to_string()));
        assert_eq!(info.task, ""); // Empty for Partial
    }

    #[test]
    fn test_convert_to_renewal_info_complete_empty_items() {
        let record = ScalePotentialRenewalRecord {
            price: 50_000_000_000,
            completion: ScaleCompletionStatus::Complete(vec![]),
        };

        let info = convert_to_renewal_info(0, 50, &record);
        assert_eq!(info.core, 0);
        assert_eq!(info.completion, Some("Complete".to_string()));
        assert!(info.mask.is_none());
        assert_eq!(info.task, "");
    }

    // ------------------------------------------------------------------------
    // RenewalInfo serialization tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_renewal_info_serialization_complete() {
        let info = RenewalInfo {
            completion: Some("Complete".to_string()),
            core: 5,
            mask: Some("0xffffffffffffffffffff".to_string()),
            price: Some("1000000000000".to_string()),
            task: "2000".to_string(),
            when: 1234,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"completion\":\"Complete\""));
        assert!(json.contains("\"core\":5"));
        assert!(json.contains("\"mask\":\"0xffffffffffffffffffff\""));
        assert!(json.contains("\"price\":\"1000000000000\""));
        assert!(json.contains("\"task\":\"2000\""));
        assert!(json.contains("\"when\":1234"));
    }

    #[test]
    fn test_renewal_info_serialization_skips_none() {
        let info = RenewalInfo {
            completion: None,
            core: 0,
            mask: None,
            price: None,
            task: String::new(),
            when: 100,
        };

        let json = serde_json::to_string(&info).unwrap();
        // None fields should be skipped
        assert!(!json.contains("\"completion\""));
        assert!(!json.contains("\"mask\""));
        assert!(!json.contains("\"price\""));
        // Required fields should be present
        assert!(json.contains("\"core\":0"));
        assert!(json.contains("\"task\":\"\""));
        assert!(json.contains("\"when\":100"));
    }

    #[test]
    fn test_renewal_info_equality() {
        let a = RenewalInfo {
            completion: Some("Complete".to_string()),
            core: 5,
            mask: Some("0xff".to_string()),
            price: Some("100".to_string()),
            task: "2000".to_string(),
            when: 123,
        };
        let b = RenewalInfo {
            completion: Some("Complete".to_string()),
            core: 5,
            mask: Some("0xff".to_string()),
            price: Some("100".to_string()),
            task: "2000".to_string(),
            when: 123,
        };
        assert_eq!(a, b);
    }

    // ------------------------------------------------------------------------
    // CoretimeRenewalsResponse serialization tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_renewals_response_serialization() {
        let response = CoretimeRenewalsResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "12345".to_string(),
            },
            renewals: vec![
                RenewalInfo {
                    completion: Some("Complete".to_string()),
                    core: 0,
                    mask: Some("0xffffffffffffffffffff".to_string()),
                    price: Some("1000000000000".to_string()),
                    task: "2000".to_string(),
                    when: 100,
                },
                RenewalInfo {
                    completion: Some("Partial".to_string()),
                    core: 1,
                    mask: Some("0xaaaaaaaaaaaaaaaaaaa".to_string()),
                    price: Some("500000000000".to_string()),
                    task: String::new(),
                    when: 200,
                },
            ],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"hash\":\"0xabc123\""));
        assert!(json.contains("\"height\":\"12345\""));
        assert!(json.contains("\"renewals\""));
        assert!(json.contains("\"core\":0"));
        assert!(json.contains("\"core\":1"));
    }

    // ------------------------------------------------------------------------
    // Sorting tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_renewals_sorting_by_core() {
        let mut renewals = vec![
            RenewalInfo {
                completion: None,
                core: 3,
                mask: None,
                price: None,
                task: String::new(),
                when: 100,
            },
            RenewalInfo {
                completion: None,
                core: 1,
                mask: None,
                price: None,
                task: String::new(),
                when: 100,
            },
            RenewalInfo {
                completion: None,
                core: 2,
                mask: None,
                price: None,
                task: String::new(),
                when: 100,
            },
        ];

        renewals.sort_by_key(|r| r.core);

        assert_eq!(renewals[0].core, 1);
        assert_eq!(renewals[1].core, 2);
        assert_eq!(renewals[2].core, 3);
    }

    // ------------------------------------------------------------------------
    // Constants tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_storage_key_constants() {
        use crate::handlers::coretime::common::{
            ENTRY_HASH_SIZE, PALLET_HASH_SIZE, STORAGE_KEY_BASE_OFFSET, STORAGE_KEY_DATA_OFFSET,
            TWOX64_HASH_SIZE,
        };

        // Verify the shared constant values
        assert_eq!(PALLET_HASH_SIZE, 16);
        assert_eq!(ENTRY_HASH_SIZE, 16);
        assert_eq!(STORAGE_KEY_BASE_OFFSET, 32);
        assert_eq!(TWOX64_HASH_SIZE, 8);
        assert_eq!(STORAGE_KEY_DATA_OFFSET, 40);

        // Verify the local constant values
        // KEY_DATA_SIZE = sizeof(u16) + sizeof(u32) = 2 + 4 = 6
        assert_eq!(KEY_DATA_SIZE, 6);
        // STORAGE_KEY_MIN_LENGTH = STORAGE_KEY_DATA_OFFSET + KEY_DATA_SIZE = 40 + 6 = 46
        assert_eq!(STORAGE_KEY_MIN_LENGTH, 46);
    }
}
