// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for /coretime/renewals endpoint.
//!
//! Returns all potential renewals registered on a coretime chain (parachain with Broker pallet).
//! Each renewal includes the core ID, timeslice when it can be renewed, price, completion status,
//! mask, and task assignment info.
//!
//! Potential renewals represent coretime allocations that can be renewed by the holder
//! before the next sale period begins.

use crate::extractors::JsonQuery;
use crate::handlers::coretime::common::{
    AtResponse, CoretimeError, CoretimeQueryParams, has_broker_pallet,
};
use crate::handlers::runtime_queries::broker::{
    self, CompletionStatus, PotentialRenewalRecord,
};
use crate::state::AppState;
use crate::utils::{BlockId, resolve_block};
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
#[utoipa::path(
    get,
    path = "/v1/coretime/renewals",
    tag = "coretime",
    summary = "Get coretime potential renewals",
    description = "Returns potential renewals on a coretime chain sorted by core index, including price, completion status, and task assignment.",
    params(
        ("at" = Option<String>, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Coretime renewals", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn coretime_renewals(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<CoretimeQueryParams>,
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
///
/// Uses the centralized runtime_queries::broker module for storage access.
async fn fetch_potential_renewals(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<RenewalInfo>, CoretimeError> {
    let renewal_entries = broker::get_potential_renewals(client_at_block)
        .await
        .map_err(|e| {
            // Check for specific storage errors
            let err_str = e.to_string();
            if err_str.contains("StorageEntryNotFound") || err_str.contains("not found") {
                CoretimeError::StorageItemNotAvailableAtBlock {
                    pallet: "Broker",
                    entry: "PotentialRenewals",
                }
            } else {
                CoretimeError::StorageQueryFailed {
                    details: e.to_string(),
                }
            }
        })?;

    let renewals = renewal_entries
        .into_iter()
        .map(|entry| convert_to_renewal_info(entry.core, entry.when, &entry.record))
        .collect();

    Ok(renewals)
}

/// Converts a PotentialRenewalRecord to the API response RenewalInfo.
fn convert_to_renewal_info(
    core: u32,
    when: u32,
    record: &PotentialRenewalRecord,
) -> RenewalInfo {
    let (completion_type, mask, task) = match &record.completion {
        CompletionStatus::Complete(items) => {
            if let Some(first_item) = items.first() {
                let mask_hex = format!("0x{}", hex::encode(first_item.mask));
                let task_str = match &first_item.assignment {
                    broker::CoreAssignment::Idle => "Idle".to_string(),
                    broker::CoreAssignment::Pool => "Pool".to_string(),
                    broker::CoreAssignment::Task(id) => id.to_string(),
                };
                (Some("Complete".to_string()), Some(mask_hex), task_str)
            } else {
                (Some("Complete".to_string()), None, String::new())
            }
        }
        CompletionStatus::Partial(mask) => {
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
    use broker::{CORE_MASK_SIZE, ScheduleItem};

    // ------------------------------------------------------------------------
    // convert_to_renewal_info tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_convert_to_renewal_info_complete_task() {
        let record = PotentialRenewalRecord {
            price: 1_000_000_000_000,
            completion: CompletionStatus::Complete(vec![ScheduleItem {
                mask: [0xFF; CORE_MASK_SIZE],
                assignment: broker::CoreAssignment::Task(2000),
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
        let record = PotentialRenewalRecord {
            price: 500_000_000_000,
            completion: CompletionStatus::Complete(vec![ScheduleItem {
                mask: [0xAA; CORE_MASK_SIZE],
                assignment: broker::CoreAssignment::Pool,
            }]),
        };

        let info = convert_to_renewal_info(3, 5678, &record);
        assert_eq!(info.core, 3);
        assert_eq!(info.task, "Pool");
        assert_eq!(info.completion, Some("Complete".to_string()));
    }

    #[test]
    fn test_convert_to_renewal_info_complete_idle() {
        let record = PotentialRenewalRecord {
            price: 100_000_000_000,
            completion: CompletionStatus::Complete(vec![ScheduleItem {
                mask: [0xFF; CORE_MASK_SIZE],
                assignment: broker::CoreAssignment::Idle,
            }]),
        };

        let info = convert_to_renewal_info(1, 100, &record);
        assert_eq!(info.task, "Idle");
    }

    #[test]
    fn test_convert_to_renewal_info_partial() {
        let record = PotentialRenewalRecord {
            price: 200_000_000_000,
            completion: CompletionStatus::Partial([0xBB; CORE_MASK_SIZE]),
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
        let record = PotentialRenewalRecord {
            price: 50_000_000_000,
            completion: CompletionStatus::Complete(vec![]),
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
}
