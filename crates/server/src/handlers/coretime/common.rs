// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common types and utilities for coretime endpoints.
//!
//! This module provides shared error types, response types,
//! and utility functions used by coretime endpoints.

use axum::{Json, http::StatusCode, response::IntoResponse};
use parity_scale_codec::{Decode, Encode};
use scale_decode::DecodeAsType;
use serde::Serialize;
use serde_json::json;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};
use thiserror::Error;

// ============================================================================
// Constants - Broker Pallet SCALE Encoding
// ============================================================================

/// Size of CoreMask in bytes (80 bits = 10 bytes).
pub const CORE_MASK_SIZE: usize = 10;

// ============================================================================
// Storage Key Constants
// ============================================================================

// Substrate storage keys consist of:
// - 16 bytes: pallet prefix (xxhash128 of pallet name)
// - 16 bytes: entry prefix (xxhash128 of storage entry name)
// - Variable: key data (depends on hasher type)
//
// For Twox64Concat hasher (common for small keys like u16, u32):
// - 8 bytes: twox64 hash of the key
// - N bytes: the raw key bytes (concatenated)

/// Size of pallet name hash in storage key (xxhash128 = 16 bytes).
pub const PALLET_HASH_SIZE: usize = 16;

/// Size of storage entry name hash in storage key (xxhash128 = 16 bytes).
pub const ENTRY_HASH_SIZE: usize = 16;

/// Base offset where map keys start (pallet hash + entry hash).
pub const STORAGE_KEY_BASE_OFFSET: usize = PALLET_HASH_SIZE + ENTRY_HASH_SIZE;

/// Size of twox64 hash prefix used in Twox64Concat hasher.
pub const TWOX64_HASH_SIZE: usize = 8;

/// Offset where the actual key data starts (after base + twox64 hash).
pub const STORAGE_KEY_DATA_OFFSET: usize = STORAGE_KEY_BASE_OFFSET + TWOX64_HASH_SIZE;

/// Size of u16 in bytes (for core index fields).
pub const U16_SIZE: usize = 2;

/// Size of u32 in bytes (for timeslice, task ID fields).
pub const U32_SIZE: usize = 4;

/// Size of u128 in bytes (for price/balance fields).
pub const U128_SIZE: usize = 16;

/// CoreMask type alias - 80 bits represented as 10 bytes.
pub type CoreMask = [u8; CORE_MASK_SIZE];

// ============================================================================
// Core Type Condition Constants
// ============================================================================

/// Core type condition: core is assigned via a lease.
pub const CORE_TYPE_LEASE: &str = "lease";

/// Core type condition: core is purchased via bulk coretime.
pub const CORE_TYPE_BULK: &str = "bulk";

/// Core type condition: core is reserved for system parachains.
pub const CORE_TYPE_RESERVATION: &str = "reservation";

/// Core type condition: core contributes to the on-demand (instantaneous) pool.
pub const CORE_TYPE_ONDEMAND: &str = "ondemand";

/// Task string for cores contributing to the instantaneous pool.
pub const TASK_POOL: &str = "Pool";

// ============================================================================
// Shared Types
// ============================================================================

/// CoreAssignment enum representing how a core is assigned.
/// Matches the Broker pallet's CoreAssignment type.
/// Derives Decode/Encode for SCALE codec support, and DecodeAsType for subxt typed decoding.
#[derive(Debug, Clone, PartialEq, Decode, Encode, DecodeAsType)]
pub enum CoreAssignment {
    /// Core is idle (not assigned).
    Idle,
    /// Core contributes to the instantaneous coretime pool.
    Pool,
    /// Core is assigned to a specific task (parachain ID).
    Task(u32),
}

impl CoreAssignment {
    /// Returns the task string representation for JSON serialization.
    /// - Task(id) -> "id"
    /// - Pool -> TASK_POOL ("Pool")
    /// - Idle -> ""
    pub fn to_task_string(&self) -> String {
        match self {
            CoreAssignment::Idle => String::new(),
            CoreAssignment::Pool => TASK_POOL.to_string(),
            CoreAssignment::Task(id) => id.to_string(),
        }
    }
}

/// ScheduleItem from the Broker pallet.
/// Contains a CoreMask and CoreAssignment.
/// Used in Workload and Reservations storage.
/// DecodeAsType enables efficient typed decoding via subxt's storage iteration.
#[derive(Debug, Clone, PartialEq, Decode, Encode, DecodeAsType)]
pub struct ScheduleItem {
    pub mask: CoreMask,
    pub assignment: CoreAssignment,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Error)]
pub enum CoretimeError {
    // ========================================================================
    // Block/Client Errors
    // ========================================================================
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("{0}")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Invalid block hash format")]
    InvalidBlockHash,

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[from] subxt::error::OnlineClientAtBlockError),

    // ========================================================================
    // Chain Type Errors
    // ========================================================================
    #[error("This endpoint is only available on coretime chains (chains with the Broker pallet)")]
    NotCoretimeChain,

    // ========================================================================
    // Pallet Errors
    // ========================================================================
    #[error("Broker pallet not found at this block")]
    BrokerPalletNotFound,

    #[error("Failed to fetch {pallet}::{entry} storage")]
    StorageFetchFailed {
        pallet: &'static str,
        entry: &'static str,
    },

    #[error("Failed to retrieve storageVersion for {pallet}")]
    StorageVersionFetchFailed { pallet: &'static str },

    #[error("Failed to decode {pallet}::{entry} storage: {details}")]
    StorageDecodeFailed {
        pallet: &'static str,
        entry: &'static str,
        details: String,
    },

    #[error("Storage iteration error for {pallet}::{entry}: {details}")]
    StorageIterationError {
        pallet: &'static str,
        entry: &'static str,
        details: String,
    },

    #[error("Failed to fetch constant {pallet}::{constant}")]
    ConstantFetchFailed {
        pallet: &'static str,
        constant: &'static str,
    },

    #[error("Coretime pallet not found at this block (relay chain endpoint)")]
    CoretimePalletNotFound,

    #[error("This endpoint is only available on relay chains or coretime chains")]
    UnsupportedChainType,

    #[error(
        "{pallet}::{entry} is not available at this block. This storage item was introduced in a later runtime upgrade."
    )]
    StorageItemNotAvailableAtBlock {
        pallet: &'static str,
        entry: &'static str,
    },
}

impl IntoResponse for CoretimeError {
    fn into_response(self) -> axum::response::Response {
        // Match Sidecar's error handling behavior:
        // - Generic thrown errors become 500 Internal Server Error
        // - BadRequest (http-errors) becomes 400
        // - Response format: { code: number, message: string }
        let (status, message) = match &self {
            // Block/Client errors - these map to Sidecar's BadRequest (400)
            CoretimeError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            CoretimeError::BlockResolveFailed(inner) => {
                let status = if matches!(inner, crate::utils::BlockResolveError::NotFound(_)) {
                    // Block not found - Sidecar returns 400 with "Specified block number is larger..."
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::BAD_REQUEST
                };
                (status, self.to_string())
            }
            CoretimeError::InvalidBlockHash => (StatusCode::BAD_REQUEST, self.to_string()),
            CoretimeError::ClientAtBlockFailed(err) => {
                if crate::utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Service temporarily unavailable: {}", err),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }

            // Chain type errors
            CoretimeError::NotCoretimeChain => (StatusCode::BAD_REQUEST, self.to_string()),
            CoretimeError::UnsupportedChainType => (StatusCode::BAD_REQUEST, self.to_string()),

            // Pallet errors — the requested resource doesn't exist at this block
            CoretimeError::BrokerPalletNotFound => (StatusCode::NOT_FOUND, self.to_string()),
            CoretimeError::CoretimePalletNotFound => (StatusCode::NOT_FOUND, self.to_string()),
            CoretimeError::StorageFetchFailed { .. } => (StatusCode::NOT_FOUND, self.to_string()),

            // Decode/iteration errors — genuine server-side failures
            CoretimeError::StorageVersionFetchFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            CoretimeError::StorageDecodeFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            CoretimeError::StorageIterationError { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            CoretimeError::ConstantFetchFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            CoretimeError::StorageItemNotAvailableAtBlock { .. } => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
        };

        // Match Sidecar's response format: { code: number, message: string }
        let body = Json(json!({
            "code": status.as_u16(),
            "message": message,
        }));

        (status, body).into_response()
    }
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct AtResponse {
    pub hash: String,
    pub height: String,
}

// ============================================================================
// Query Parameters
// ============================================================================

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoretimeQueryParams {
    /// Block number or 0x-prefixed block hash to query at.
    /// If not provided, queries at the latest finalized block.
    pub at: Option<String>,
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Checks if the Broker pallet exists in the runtime metadata.
pub fn has_broker_pallet(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> bool {
    let metadata = client_at_block.metadata();
    metadata.pallet_by_name("Broker").is_some()
}

/// Checks if the CoretimeAssignmentProvider pallet exists in the runtime metadata.
/// This pallet is present on relay chains and contains core descriptors and schedules.
pub fn has_coretime_assignment_provider_pallet(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> bool {
    let metadata = client_at_block.metadata();
    metadata
        .pallet_by_name("CoretimeAssignmentProvider")
        .is_some()
}

/// Checks if the Paras pallet exists in the runtime metadata.
/// This pallet is present on relay chains and contains parachain lifecycle information.
pub fn has_paras_pallet(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> bool {
    let metadata = client_at_block.metadata();
    metadata.pallet_by_name("Paras").is_some()
}

/// Checks if an error indicates that a storage item was not found in metadata.
/// This typically happens when querying historical blocks before a runtime upgrade
/// that introduced the storage item.
pub fn is_storage_item_not_found_error(error: &subxt::error::StorageError) -> bool {
    // Check both Display and Debug representations (case-insensitive)
    let display_str = error.to_string().to_lowercase();
    let debug_str = format!("{:?}", error).to_lowercase();

    // Look for common patterns indicating storage item not found
    let patterns = ["storage item not found", "storageitemnotfound", "not found"];

    for pattern in patterns {
        if display_str.contains(pattern) || debug_str.contains(pattern) {
            return true;
        }
    }

    false
}

/// Checks if the Coretime pallet exists in the runtime metadata (relay chain).
pub fn has_coretime_pallet(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> bool {
    let metadata = client_at_block.metadata();
    metadata.pallet_by_name("Coretime").is_some()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    // ------------------------------------------------------------------------
    // Error message tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_coretime_error_not_coretime_chain_message() {
        let err = CoretimeError::NotCoretimeChain;
        assert_eq!(
            err.to_string(),
            "This endpoint is only available on coretime chains (chains with the Broker pallet)"
        );
    }

    #[test]
    fn test_coretime_error_broker_pallet_not_found_message() {
        let err = CoretimeError::BrokerPalletNotFound;
        assert_eq!(err.to_string(), "Broker pallet not found at this block");
    }

    #[test]
    fn test_coretime_error_storage_fetch_failed_message() {
        let err = CoretimeError::StorageFetchFailed {
            pallet: "Broker",
            entry: "Leases",
        };
        assert_eq!(err.to_string(), "Failed to fetch Broker::Leases storage");
    }

    #[test]
    fn test_coretime_error_storage_decode_failed_message() {
        let err = CoretimeError::StorageDecodeFailed {
            pallet: "Broker",
            entry: "Leases",
            details: "invalid data".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Failed to decode Broker::Leases storage: invalid data"
        );
    }

    #[test]
    fn test_coretime_error_storage_iteration_error_message() {
        let err = CoretimeError::StorageIterationError {
            pallet: "Broker",
            entry: "Workload",
            details: "connection lost".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Storage iteration error for Broker::Workload: connection lost"
        );
    }

    #[test]
    fn test_coretime_error_storage_item_not_available_message() {
        let err = CoretimeError::StorageItemNotAvailableAtBlock {
            pallet: "Broker",
            entry: "PotentialRenewals",
        };
        assert_eq!(
            err.to_string(),
            "Broker::PotentialRenewals is not available at this block. This storage item was introduced in a later runtime upgrade."
        );
    }

    // ------------------------------------------------------------------------
    // HTTP Status code tests
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_coretime_error_not_coretime_chain_status() {
        let err = CoretimeError::NotCoretimeChain;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_coretime_error_broker_pallet_not_found_status() {
        let err = CoretimeError::BrokerPalletNotFound;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_coretime_error_storage_fetch_failed_status() {
        let err = CoretimeError::StorageFetchFailed {
            pallet: "Broker",
            entry: "Leases",
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_coretime_error_storage_decode_failed_status() {
        let err = CoretimeError::StorageDecodeFailed {
            pallet: "Broker",
            entry: "Leases",
            details: "invalid".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_coretime_error_storage_iteration_error_status() {
        let err = CoretimeError::StorageIterationError {
            pallet: "Broker",
            entry: "Workload",
            details: "error".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_coretime_error_storage_item_not_available_status() {
        let err = CoretimeError::StorageItemNotAvailableAtBlock {
            pallet: "Broker",
            entry: "PotentialRenewals",
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_coretime_error_response_body_format() {
        let err = CoretimeError::BrokerPalletNotFound;
        let response = err.into_response();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        // Response format: { code: number, message: string }
        assert!(body_str.contains("\"code\":404"));
        assert!(body_str.contains("\"message\""));
        assert!(body_str.contains("Broker pallet not found"));
    }

    // ------------------------------------------------------------------------
    // AtResponse tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_at_response_serialization() {
        let at = AtResponse {
            hash: "0xabc123".to_string(),
            height: "12345".to_string(),
        };

        let json = serde_json::to_string(&at).unwrap();
        assert!(json.contains("\"hash\":\"0xabc123\""));
        assert!(json.contains("\"height\":\"12345\""));
    }

    #[test]
    fn test_at_response_clone() {
        let at = AtResponse {
            hash: "0xabc".to_string(),
            height: "100".to_string(),
        };

        let cloned = at.clone();
        assert_eq!(cloned.hash, "0xabc");
        assert_eq!(cloned.height, "100");
    }

    // ------------------------------------------------------------------------
    // CoretimeQueryParams tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_query_params_deserialize_with_at() {
        let json = r#"{"at": "12345"}"#;
        let params: CoretimeQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("12345".to_string()));
    }

    #[test]
    fn test_query_params_deserialize_with_at_hash() {
        let json = r#"{"at": "0xabc123"}"#;
        let params: CoretimeQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("0xabc123".to_string()));
    }

    #[test]
    fn test_query_params_deserialize_without_at() {
        let json = r#"{}"#;
        let params: CoretimeQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, None);
    }

    #[test]
    fn test_query_params_deserialize_null_at() {
        let json = r#"{"at": null}"#;
        let params: CoretimeQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, None);
    }

    // ------------------------------------------------------------------------
    // CoreAssignment tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_core_assignment_idle_to_string() {
        let assignment = CoreAssignment::Idle;
        assert_eq!(assignment.to_task_string(), "");
    }

    #[test]
    fn test_core_assignment_pool_to_string() {
        let assignment = CoreAssignment::Pool;
        assert_eq!(assignment.to_task_string(), TASK_POOL);
    }

    #[test]
    fn test_core_assignment_task_to_string() {
        let assignment = CoreAssignment::Task(1000);
        assert_eq!(assignment.to_task_string(), "1000");
    }

    #[test]
    fn test_core_assignment_equality() {
        assert_eq!(CoreAssignment::Idle, CoreAssignment::Idle);
        assert_eq!(CoreAssignment::Pool, CoreAssignment::Pool);
        assert_eq!(CoreAssignment::Task(100), CoreAssignment::Task(100));
        assert_ne!(CoreAssignment::Task(100), CoreAssignment::Task(200));
        assert_ne!(CoreAssignment::Idle, CoreAssignment::Pool);
    }

    // ------------------------------------------------------------------------
    // decode_compact_u32 tests
}
