// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for /coretime/leases endpoint.
//!
//! Returns all leases registered on a coretime chain (parachain with Broker pallet).
//! Each lease includes the task ID (parachain ID), the until timeslice, and the
//! assigned core ID (correlated from workload data).

use crate::extractors::JsonQuery;
use crate::handlers::coretime::common::{
    AtResponse, CoretimeError, CoretimeQueryParams, has_broker_pallet,
};
use crate::handlers::runtime_queries::broker::{
    self as broker_queries, LeaseRecordItem, WorkloadInfo,
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

/// A single lease record with its assigned core.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseWithCore {
    /// The task ID (parachain ID) that holds this lease.
    pub task: String,
    /// The timeslice until which the lease is valid.
    pub until: u32,
    /// The core ID assigned to this lease (correlated from workload data).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core: Option<u32>,
}

/// Response for GET /coretime/leases endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoretimeLeasesResponse {
    /// Block context (hash and height).
    pub at: AtResponse,
    /// List of active leases with their assigned cores.
    pub leases: Vec<LeaseWithCore>,
}

// ============================================================================
// Main Handler
// ============================================================================

/// Handler for GET /coretime/leases endpoint.
///
/// Returns all leases registered on a coretime chain. Each lease includes:
/// - task: The parachain ID that holds the lease
/// - until: The timeslice until which the lease is valid
/// - core: The core ID assigned to this lease (if correlatable from workload)
///
/// Query Parameters:
/// - at: Optional block number or hash to query at (defaults to latest finalized)
#[utoipa::path(
    get,
    path = "/v1/coretime/leases",
    tag = "coretime",
    summary = "Get coretime leases",
    description = "Returns all leases registered on a coretime chain with task IDs and validity timeslices.",
    params(
        ("at" = Option<String>, Query, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Coretime leases", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn coretime_leases(
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

    // Fetch leases and workload data in parallel (independent RPC calls)
    let (leases, workloads) = tokio::try_join!(
        fetch_leases(&client_at_block),
        fetch_workloads(&client_at_block)
    )?;

    // Correlate leases with their assigned cores from workload data
    let leases_with_cores: Vec<LeaseWithCore> = leases
        .into_iter()
        .map(|lease| {
            // Find the core assigned to this task from workload data
            let core = workloads
                .iter()
                .find(|wl| wl.task == Some(lease.task))
                .map(|wl| wl.core);

            LeaseWithCore {
                task: lease.task.to_string(),
                until: lease.until,
                core,
            }
        })
        .collect();

    // Sort by core ID (leases with no core come last)
    let mut sorted_leases = leases_with_cores;
    sorted_leases.sort_by(|a, b| match (a.core, b.core) {
        (Some(a_core), Some(b_core)) => a_core.cmp(&b_core),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    Ok((
        StatusCode::OK,
        Json(CoretimeLeasesResponse {
            at,
            leases: sorted_leases,
        }),
    )
        .into_response())
}

// ============================================================================
// Helper Functions (wrappers around runtime_queries::broker)
// ============================================================================

/// Fetches all leases from Broker::Leases storage.
pub async fn fetch_leases(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<LeaseRecordItem>, CoretimeError> {
    broker_queries::get_leases(client_at_block)
        .await
        .map_err(|e| match e {
            broker_queries::BrokerStorageError::StorageFetchFailed { pallet, entry } => {
                CoretimeError::StorageFetchFailed { pallet, entry }
            }
            broker_queries::BrokerStorageError::StorageDecodeFailed {
                pallet,
                entry,
                details,
            } => CoretimeError::StorageDecodeFailed {
                pallet,
                entry,
                details,
            },
            _ => CoretimeError::StorageFetchFailed {
                pallet: "Broker",
                entry: "Leases",
            },
        })
}

/// Fetches all workload entries from Broker::Workload storage map.
async fn fetch_workloads(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<WorkloadInfo>, CoretimeError> {
    broker_queries::iter_workloads(client_at_block)
        .await
        .map_err(|e| match e {
            broker_queries::BrokerStorageError::StorageIterationError {
                pallet,
                entry,
                details,
            } => CoretimeError::StorageIterationError {
                pallet,
                entry,
                details,
            },
            _ => CoretimeError::StorageFetchFailed {
                pallet: "Broker",
                entry: "Workload",
            },
        })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: Decode tests for LeaseRecordItem, ScheduleItem, CoreAssignment
    // have been moved to the runtime_queries::broker module where these types are now defined.

    // ------------------------------------------------------------------------
    // LeaseWithCore serialization tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_lease_with_core_serialization_with_core() {
        let lease = LeaseWithCore {
            task: "2000".to_string(),
            until: 1234567,
            core: Some(5),
        };

        let json = serde_json::to_string(&lease).unwrap();
        assert!(json.contains("\"task\":\"2000\""));
        assert!(json.contains("\"until\":1234567"));
        assert!(json.contains("\"core\":5"));
    }

    #[test]
    fn test_lease_with_core_serialization_without_core() {
        let lease = LeaseWithCore {
            task: "2000".to_string(),
            until: 1234567,
            core: None,
        };

        let json = serde_json::to_string(&lease).unwrap();
        assert!(json.contains("\"task\":\"2000\""));
        assert!(json.contains("\"until\":1234567"));
        // core should be skipped when None
        assert!(!json.contains("\"core\""));
    }

    #[test]
    fn test_coretime_leases_response_serialization() {
        let response = CoretimeLeasesResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "12345".to_string(),
            },
            leases: vec![
                LeaseWithCore {
                    task: "2000".to_string(),
                    until: 100,
                    core: Some(0),
                },
                LeaseWithCore {
                    task: "2001".to_string(),
                    until: 200,
                    core: Some(1),
                },
            ],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"hash\":\"0xabc123\""));
        assert!(json.contains("\"height\":\"12345\""));
        assert!(json.contains("\"leases\""));
        assert!(json.contains("\"task\":\"2000\""));
        assert!(json.contains("\"task\":\"2001\""));
    }

    // ------------------------------------------------------------------------
    // Sorting tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_leases_sorting_by_core() {
        let mut leases = vec![
            LeaseWithCore {
                task: "2002".to_string(),
                until: 100,
                core: Some(2),
            },
            LeaseWithCore {
                task: "2000".to_string(),
                until: 100,
                core: Some(0),
            },
            LeaseWithCore {
                task: "2001".to_string(),
                until: 100,
                core: Some(1),
            },
        ];

        leases.sort_by(|a, b| match (a.core, b.core) {
            (Some(a_core), Some(b_core)) => a_core.cmp(&b_core),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        assert_eq!(leases[0].task, "2000");
        assert_eq!(leases[0].core, Some(0));
        assert_eq!(leases[1].task, "2001");
        assert_eq!(leases[1].core, Some(1));
        assert_eq!(leases[2].task, "2002");
        assert_eq!(leases[2].core, Some(2));
    }

    #[test]
    fn test_leases_sorting_none_cores_last() {
        let mut leases = vec![
            LeaseWithCore {
                task: "2003".to_string(),
                until: 100,
                core: None,
            },
            LeaseWithCore {
                task: "2000".to_string(),
                until: 100,
                core: Some(0),
            },
            LeaseWithCore {
                task: "2002".to_string(),
                until: 100,
                core: None,
            },
            LeaseWithCore {
                task: "2001".to_string(),
                until: 100,
                core: Some(1),
            },
        ];

        leases.sort_by(|a, b| match (a.core, b.core) {
            (Some(a_core), Some(b_core)) => a_core.cmp(&b_core),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        // Cores with Some values should come first, sorted by core ID
        assert_eq!(leases[0].core, Some(0));
        assert_eq!(leases[1].core, Some(1));
        // None cores should come last
        assert_eq!(leases[2].core, None);
        assert_eq!(leases[3].core, None);
    }

    // ------------------------------------------------------------------------
    // WorkloadInfo tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_workload_info_creation() {
        let workload = WorkloadInfo {
            core: 5,
            task: Some(2000),
        };

        assert_eq!(workload.core, 5);
        assert_eq!(workload.task, Some(2000));
    }

    #[test]
    fn test_workload_info_without_task() {
        let workload = WorkloadInfo {
            core: 3,
            task: None,
        };

        assert_eq!(workload.core, 3);
        assert_eq!(workload.task, None);
    }

    // ------------------------------------------------------------------------
    // Lease correlation tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_lease_workload_correlation() {
        let leases = vec![
            LeaseRecordItem {
                until: 100,
                task: 2000,
            },
            LeaseRecordItem {
                until: 200,
                task: 2001,
            },
            LeaseRecordItem {
                until: 300,
                task: 2002,
            },
        ];

        let workloads = vec![
            WorkloadInfo {
                core: 0,
                task: Some(2000),
            },
            WorkloadInfo {
                core: 1,
                task: Some(2001),
            },
            // Note: no workload for task 2002
        ];

        let leases_with_cores: Vec<LeaseWithCore> = leases
            .into_iter()
            .map(|lease| {
                let core = workloads
                    .iter()
                    .find(|wl| wl.task == Some(lease.task))
                    .map(|wl| wl.core);

                LeaseWithCore {
                    task: lease.task.to_string(),
                    until: lease.until,
                    core,
                }
            })
            .collect();

        assert_eq!(leases_with_cores.len(), 3);

        assert_eq!(leases_with_cores[0].task, "2000");
        assert_eq!(leases_with_cores[0].core, Some(0));

        assert_eq!(leases_with_cores[1].task, "2001");
        assert_eq!(leases_with_cores[1].core, Some(1));

        assert_eq!(leases_with_cores[2].task, "2002");
        assert_eq!(leases_with_cores[2].core, None); // No matching workload
    }
}
