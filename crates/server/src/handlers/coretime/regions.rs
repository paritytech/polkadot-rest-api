// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for /coretime/regions endpoint.
//!
//! Returns all regions registered on a coretime chain (parachain with Broker pallet).
//! Each region includes the core ID, begin timeslice, end timeslice, owner, paid amount,
//! and the CoreMask.
//!
//! Regions represent purchased coretime that can be traded or used.

use crate::extractors::JsonQuery;
use crate::handlers::coretime::common::{
    AtResponse,
    CoretimeError,
    CoretimeQueryParams,
    // Shared functions
    has_broker_pallet,
};
use crate::handlers::runtime_queries::broker;
use crate::state::AppState;
use crate::utils::{BlockId, decode_address_to_ss58, resolve_block};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use primitive_types::H256;
use serde::Serialize;
use std::str::FromStr;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

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
#[utoipa::path(
    get,
    path = "/v1/coretime/regions",
    tag = "coretime",
    summary = "Get coretime regions",
    description = "Returns all regions on a coretime chain including begin/end timeslices, core, owner, and mask.",
    params(
        ("at" = Option<String>, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Coretime regions", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn coretime_regions(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<CoretimeQueryParams>,
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

/// Fetches all regions from Broker::Regions storage map and converts to RegionInfo.
///
/// Uses the centralized runtime_queries::broker module for storage access.
pub async fn fetch_regions(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<Vec<RegionInfo>, CoretimeError> {
    let region_entries = broker::get_regions(client_at_block)
        .await
        .map_err(|e| CoretimeError::StorageQueryFailed {
            details: e.to_string(),
        })?;

    let mut regions = Vec::new();

    for entry in region_entries {
        // Extract fields from RegionRecord if available
        let (end, owner_bytes, paid) = match entry.record {
            Some(r) => (Some(r.end), r.owner, r.paid),
            None => (None, None, None),
        };

        // Convert owner from bytes to SS58 format if available
        let owner_ss58 = owner_bytes.and_then(|bytes| {
            let hex_owner = format!("0x{}", hex::encode(bytes));
            decode_address_to_ss58(&hex_owner, ss58_prefix)
        });

        regions.push(RegionInfo {
            core: entry.id.core as u32,
            begin: entry.id.begin,
            end,
            owner: owner_ss58,
            paid: paid.map(|p| p.to_string()),
            mask: format!("0x{}", hex::encode(entry.id.mask)),
        });
    }

    Ok(regions)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use broker::{CORE_MASK_SIZE, RegionId, RegionRecord};
    use parity_scale_codec::{Decode, Encode};

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
        // Empty bytes should fail to decode
        let result = RegionRecord::decode(&mut &[][..]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_region_record_invalid() {
        // Not enough bytes for a valid RegionRecord
        let bytes = vec![0x00, 0x01];
        let result = RegionRecord::decode(&mut &bytes[..]);
        assert!(result.is_err());
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
