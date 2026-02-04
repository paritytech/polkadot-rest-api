//! Handler for /coretime/leases endpoint.
//!
//! Returns all leases registered on a coretime chain (parachain with Broker pallet).
//! Each lease includes the task ID (parachain ID), the until timeslice, and the
//! assigned core ID (correlated from workload data).

use crate::handlers::coretime::common::{
    ASSIGNMENT_IDLE_VARIANT, ASSIGNMENT_POOL_VARIANT, ASSIGNMENT_TASK_VARIANT, AtResponse,
    CORE_MASK_SIZE, CoretimeError, CoretimeQueryParams, TASK_ID_SIZE, decode_compact_u32,
    has_broker_pallet,
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
use parity_scale_codec::{Decode, Encode};
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
// Internal SCALE Decode Types
// ============================================================================

/// Internal representation of a lease record from Broker::Leases storage.
/// Matches the PalletBrokerLeaseRecordItem type from the Broker pallet.
#[derive(Debug, Clone, Decode, Encode)]
struct LeaseRecordItem {
    /// The timeslice until which the lease is valid.
    until: u32,
    /// The task ID (parachain ID).
    task: u32,
}

/// Workload info extracted from Broker::Workload storage.
#[derive(Debug, Clone)]
struct WorkloadInfo {
    core: u32,
    task: Option<u32>,
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
pub async fn coretime_leases(
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
// Helper Functions
// ============================================================================

/// Fetches all leases from Broker::Leases storage.
async fn fetch_leases(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<LeaseRecordItem>, CoretimeError> {
    // Broker::Leases is a StorageValue that contains a BoundedVec of LeaseRecordItem
    let leases_addr = subxt::dynamic::storage::<(), scale_value::Value>("Broker", "Leases");

    let leases_value = match client_at_block.storage().fetch(leases_addr, ()).await {
        Ok(value) => value,
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            // No leases storage entry means no leases
            return Ok(vec![]);
        }
        Err(_) => {
            return Err(CoretimeError::StorageFetchFailed {
                pallet: "Broker",
                entry: "Leases",
            });
        }
    };

    let raw_bytes = leases_value.into_bytes();

    // Decode as a Vec<LeaseRecordItem>
    // The storage value is a BoundedVec which decodes as a regular Vec
    let leases: Vec<LeaseRecordItem> = Vec::<LeaseRecordItem>::decode(&mut &raw_bytes[..])
        .map_err(|e| CoretimeError::StorageDecodeFailed {
            pallet: "Broker",
            entry: "Leases",
            details: e.to_string(),
        })?;

    Ok(leases)
}

/// Fetches all workload entries from Broker::Workload storage map.
async fn fetch_workloads(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<WorkloadInfo>, CoretimeError> {
    // Broker::Workload is a StorageMap with CoreIndex (u16) as key
    // Use tuple for the key type
    let workload_addr = subxt::dynamic::storage::<(u16,), scale_value::Value>("Broker", "Workload");

    let mut workloads = Vec::new();

    // Iterate over all workload entries using () as partial keys
    let mut iter = client_at_block
        .storage()
        .iter(workload_addr, ())
        .await
        .map_err(|e| CoretimeError::StorageIterationError {
            pallet: "Broker",
            entry: "Workload",
            details: e.to_string(),
        })?;

    while let Some(result) = iter.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Error iterating workload: {:?}", e);
                continue;
            }
        };

        let core: u32 = match entry
            .key()
            .ok()
            .and_then(|k| k.part(0))
            .and_then(|p| p.decode_as::<u16>().ok().flatten())
        {
            Some(c) => c as u32,
            None => continue,
        };

        let value_bytes = entry.value().bytes();
        // Parse the workload value to extract task assignment
        let task = extract_task_from_workload(value_bytes);

        workloads.push(WorkloadInfo { core, task });
    }

    Ok(workloads)
}

/// Extracts the task ID from workload schedule items.
/// Workload is a Vec<ScheduleItem> where each ScheduleItem has a mask and assignment.
fn extract_task_from_workload(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }

    // The workload is encoded as a Vec<ScheduleItem>
    // Each ScheduleItem contains: mask (CoreMask = [u8; 10]) + assignment (CoreAssignment enum)
    // The Vec starts with a compact-encoded length

    // Decode the vec length (compact encoding)
    let (vec_len, offset) = decode_compact_u32(bytes)?;
    if vec_len == 0 {
        return None;
    }

    // Process the first schedule item (index 0)
    // ScheduleItem = { mask: CoreMask, assignment: CoreAssignment }
    let item_start = offset;

    // Skip the mask to get to the assignment
    let assignment_start = item_start + CORE_MASK_SIZE;
    if assignment_start >= bytes.len() {
        return None;
    }

    // Decode the assignment (CoreAssignment enum)
    let assignment_byte = bytes[assignment_start];
    match assignment_byte {
        ASSIGNMENT_IDLE_VARIANT | ASSIGNMENT_POOL_VARIANT => {
            // Idle or Pool - no task ID associated
            None
        }
        ASSIGNMENT_TASK_VARIANT => {
            // Task variant: 1 byte discriminant + u32 task ID
            let task_id_start = assignment_start + 1;
            let task_id_end = task_id_start + TASK_ID_SIZE;
            if task_id_end <= bytes.len() {
                let task_bytes: [u8; 4] = bytes[task_id_start..task_id_end].try_into().ok()?;
                Some(u32::from_le_bytes(task_bytes))
            } else {
                None
            }
        }
        _ => None, // Unknown variant - treat as no task
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use parity_scale_codec::Encode;

    // Note: decode_compact_u32 tests are in common.rs

    // ------------------------------------------------------------------------
    // extract_task_from_workload tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_extract_task_from_workload_empty() {
        assert_eq!(extract_task_from_workload(&[]), None);
    }

    #[test]
    fn test_extract_task_from_workload_zero_length_vec() {
        // Compact-encoded 0 (empty vec)
        assert_eq!(extract_task_from_workload(&[0x00]), None);
    }

    #[test]
    fn test_extract_task_from_workload_idle_assignment() {
        // Vec with 1 item: compact(1) + mask + assignment(Idle)
        let mut bytes = vec![0x04]; // compact(1)
        bytes.extend_from_slice(&[0xFF; CORE_MASK_SIZE]); // mask
        bytes.push(ASSIGNMENT_IDLE_VARIANT); // Idle assignment

        assert_eq!(extract_task_from_workload(&bytes), None);
    }

    #[test]
    fn test_extract_task_from_workload_pool_assignment() {
        // Vec with 1 item: compact(1) + mask + assignment(Pool)
        let mut bytes = vec![0x04]; // compact(1)
        bytes.extend_from_slice(&[0xFF; CORE_MASK_SIZE]); // mask
        bytes.push(ASSIGNMENT_POOL_VARIANT); // Pool assignment

        assert_eq!(extract_task_from_workload(&bytes), None);
    }

    #[test]
    fn test_extract_task_from_workload_task_assignment() {
        // Vec with 1 item: compact(1) + mask + assignment(Task(2000))
        let mut bytes = vec![0x04]; // compact(1)
        bytes.extend_from_slice(&[0xFF; CORE_MASK_SIZE]); // mask
        bytes.push(ASSIGNMENT_TASK_VARIANT); // Task assignment variant
        bytes.extend_from_slice(&2000u32.to_le_bytes()); // task ID = 2000

        assert_eq!(extract_task_from_workload(&bytes), Some(2000));
    }

    #[test]
    fn test_extract_task_from_workload_task_assignment_different_ids() {
        // Test with task ID 1000
        let mut bytes = vec![0x04]; // compact(1)
        bytes.extend_from_slice(&[0x00; CORE_MASK_SIZE]); // mask
        bytes.push(ASSIGNMENT_TASK_VARIANT); // Task assignment variant
        bytes.extend_from_slice(&1000u32.to_le_bytes());

        assert_eq!(extract_task_from_workload(&bytes), Some(1000));

        // Test with task ID 3000
        let mut bytes = vec![0x04]; // compact(1)
        bytes.extend_from_slice(&[0x00; CORE_MASK_SIZE]); // mask
        bytes.push(ASSIGNMENT_TASK_VARIANT); // Task assignment variant
        bytes.extend_from_slice(&3000u32.to_le_bytes());

        assert_eq!(extract_task_from_workload(&bytes), Some(3000));
    }

    #[test]
    fn test_extract_task_from_workload_truncated_task_id() {
        // Task assignment but not enough bytes for task ID
        let mut bytes = vec![0x04]; // compact(1)
        bytes.extend_from_slice(&[0xFF; CORE_MASK_SIZE]); // mask
        bytes.push(ASSIGNMENT_TASK_VARIANT); // Task assignment variant
        bytes.extend_from_slice(&[0xD0, 0x07]); // only 2 bytes instead of TASK_ID_SIZE

        assert_eq!(extract_task_from_workload(&bytes), None);
    }

    #[test]
    fn test_extract_task_from_workload_truncated_mask() {
        // Vec length says 1 item but not enough bytes for mask
        let mut bytes = vec![0x04]; // compact(1)
        bytes.extend_from_slice(&[0xFF; 5]); // only 5 bytes of mask (need CORE_MASK_SIZE)

        assert_eq!(extract_task_from_workload(&bytes), None);
    }

    // ------------------------------------------------------------------------
    // LeaseRecordItem decode tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_lease_record_item_decode() {
        // LeaseRecordItem { until: u32, task: u32 }
        // SCALE encoding: until (4 bytes LE) + task (4 bytes LE)
        let until: u32 = 1234567;
        let task: u32 = 2000;

        let mut encoded = until.to_le_bytes().to_vec();
        encoded.extend_from_slice(&task.to_le_bytes());

        let decoded = LeaseRecordItem::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded.until, 1234567);
        assert_eq!(decoded.task, 2000);
    }

    #[test]
    fn test_lease_record_item_vec_decode() {
        // Vec<LeaseRecordItem> with 2 items
        let lease1 = LeaseRecordItem {
            until: 100,
            task: 2000,
        };
        let lease2 = LeaseRecordItem {
            until: 200,
            task: 2001,
        };

        // SCALE encode as Vec
        let encoded = vec![lease1.clone(), lease2.clone()].encode();

        let decoded: Vec<LeaseRecordItem> =
            Vec::<LeaseRecordItem>::decode(&mut &encoded[..]).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].until, 100);
        assert_eq!(decoded[0].task, 2000);
        assert_eq!(decoded[1].until, 200);
        assert_eq!(decoded[1].task, 2001);
    }

    #[test]
    fn test_lease_record_item_empty_vec_decode() {
        let encoded: Vec<u8> = Vec::<LeaseRecordItem>::new().encode();

        let decoded: Vec<LeaseRecordItem> =
            Vec::<LeaseRecordItem>::decode(&mut &encoded[..]).unwrap();

        assert!(decoded.is_empty());
    }

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
