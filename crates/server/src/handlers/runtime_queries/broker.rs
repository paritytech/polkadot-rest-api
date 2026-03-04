// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Broker pallet storage query functions.
//!
//! This module provides standalone functions for querying Broker pallet storage items
//! used by coretime endpoints.
//!
//! # Storage Items Covered
//! - `Broker::Leases` - Active leases on cores
//! - `Broker::Reservations` - Reserved cores
//! - `Broker::Configuration` - Broker configuration
//! - `Broker::SaleInfo` - Current sale information
//! - `Broker::Status` - Broker status
//! - `Broker::Workload` - Core workload assignments

use parity_scale_codec::{Decode, Encode};
use subxt::ext::scale_decode::DecodeAsType;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying Broker pallet storage.
#[derive(Debug, Error)]
pub enum BrokerStorageError {
    /// The Broker pallet is not available on this chain.
    #[error("Broker pallet not available")]
    PalletNotAvailable,

    /// Failed to fetch storage.
    #[error("Failed to fetch {pallet}::{entry}")]
    StorageFetchFailed {
        pallet: &'static str,
        entry: &'static str,
    },

    /// Failed to decode storage value.
    #[error("Failed to decode {pallet}::{entry}: {details}")]
    StorageDecodeFailed {
        pallet: &'static str,
        entry: &'static str,
        details: String,
    },

    /// Failed to iterate storage.
    #[error("Failed to iterate {pallet}::{entry}: {details}")]
    StorageIterationError {
        pallet: &'static str,
        entry: &'static str,
        details: String,
    },

    /// Failed to fetch constant.
    #[error("Failed to fetch constant {pallet}::{constant}")]
    ConstantFetchFailed {
        pallet: &'static str,
        constant: &'static str,
    },

    /// Failed to fetch storage version.
    #[error("Failed to fetch storage version for {pallet}")]
    StorageVersionFetchFailed { pallet: &'static str },
}

// ================================================================================================
// SCALE Decode Types
// ================================================================================================

/// Lease record item from Broker::Leases.
#[derive(Debug, Clone, Decode, Encode)]
pub struct LeaseRecordItem {
    pub until: u32,
    pub task: u32,
}

/// Schedule item used in reservations and workloads.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct ScheduleItem {
    pub mask: [u8; 10], // CoreMask is 80 bits = 10 bytes
    pub assignment: CoreAssignment,
}

/// Core assignment enum matching runtime definition.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub enum CoreAssignment {
    Idle,
    Pool,
    Task(u32),
}

impl CoreAssignment {
    /// Convert assignment to task string representation.
    pub fn to_task_string(&self) -> String {
        match self {
            CoreAssignment::Idle => "idle".to_string(),
            CoreAssignment::Pool => "pool".to_string(),
            CoreAssignment::Task(id) => id.to_string(),
        }
    }
}

// ================================================================================================
// Decoded Output Types
// ================================================================================================

/// Workload info with core index and optional task.
#[derive(Debug, Clone)]
pub struct WorkloadInfo {
    pub core: u32,
    pub task: Option<u32>,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Fetches all leases from Broker::Leases storage.
///
/// Returns an empty vector if no leases exist.
pub async fn get_leases(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<LeaseRecordItem>, BrokerStorageError> {
    let leases_addr = subxt::dynamic::storage::<(), ()>("Broker", "Leases");

    let leases_value = match client_at_block.storage().fetch(leases_addr, ()).await {
        Ok(value) => value,
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            return Ok(vec![]);
        }
        Err(_) => {
            return Err(BrokerStorageError::StorageFetchFailed {
                pallet: "Broker",
                entry: "Leases",
            });
        }
    };

    let raw_bytes = leases_value.into_bytes();

    Vec::<LeaseRecordItem>::decode(&mut &raw_bytes[..]).map_err(|e| {
        BrokerStorageError::StorageDecodeFailed {
            pallet: "Broker",
            entry: "Leases",
            details: e.to_string(),
        }
    })
}

/// Fetches all reservations from Broker::Reservations storage.
///
/// Returns an empty vector if no reservations exist.
pub async fn get_reservations(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<Vec<ScheduleItem>>, BrokerStorageError> {
    let reservations_addr = subxt::dynamic::storage::<(), ()>("Broker", "Reservations");

    let reservations_value = match client_at_block.storage().fetch(reservations_addr, ()).await {
        Ok(value) => value,
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            return Ok(vec![]);
        }
        Err(_) => {
            return Err(BrokerStorageError::StorageFetchFailed {
                pallet: "Broker",
                entry: "Reservations",
            });
        }
    };

    reservations_value
        .decode_as()
        .map_err(|e| BrokerStorageError::StorageDecodeFailed {
            pallet: "Broker",
            entry: "Reservations",
            details: e.to_string(),
        })
}

/// Fetches all workload entries from Broker::Workload storage map.
pub async fn iter_workloads(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<WorkloadInfo>, BrokerStorageError> {
    let workload_addr = subxt::dynamic::storage::<(u16,), ()>("Broker", "Workload");

    let mut workloads = Vec::new();

    let mut iter = client_at_block
        .storage()
        .iter(workload_addr, ())
        .await
        .map_err(|e| BrokerStorageError::StorageIterationError {
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

        // Extract core index using subxt's key decoder
        let core: u32 = match entry.key() {
            Ok(storage_key) => match storage_key.decode() {
                Ok(key) => key.0 as u32,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        // Decode workload value into typed struct and extract task
        let task = entry
            .value()
            .decode_as::<Vec<ScheduleItem>>()
            .ok()
            .and_then(extract_task_from_workload);

        workloads.push(WorkloadInfo { core, task });
    }

    Ok(workloads)
}

/// Extracts the task ID from a typed workload schedule.
fn extract_task_from_workload(items: Vec<ScheduleItem>) -> Option<u32> {
    items.first().and_then(|item| match item.assignment {
        CoreAssignment::Task(id) => Some(id),
        _ => None,
    })
}

/// Fetches Broker::Configuration storage value.
///
/// Returns None if the configuration doesn't exist.
pub async fn get_configuration<T>(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<T>, BrokerStorageError>
where
    T: subxt::ext::scale_decode::DecodeAsType,
{
    let config_addr = subxt::dynamic::storage::<(), T>("Broker", "Configuration");

    match client_at_block.storage().fetch(config_addr, ()).await {
        Ok(storage_value) => {
            let decoded =
                storage_value
                    .decode()
                    .map_err(|e| BrokerStorageError::StorageDecodeFailed {
                        pallet: "Broker",
                        entry: "Configuration",
                        details: e.to_string(),
                    })?;
            Ok(Some(decoded))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            tracing::debug!("Could not find Broker.Configuration storage entry.");
            Ok(None)
        }
        Err(e) => {
            tracing::debug!(
                "Failed to retrieve Broker.Configuration: {:?}",
                format!("{e}")
            );
            Ok(None)
        }
    }
}

/// Fetches Broker::SaleInfo storage value.
///
/// Returns None if the sale info doesn't exist.
pub async fn get_sale_info<T>(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<T>, BrokerStorageError>
where
    T: subxt::ext::scale_decode::DecodeAsType,
{
    let sale_addr = subxt::dynamic::storage::<(), T>("Broker", "SaleInfo");

    match client_at_block.storage().fetch(sale_addr, ()).await {
        Ok(storage_value) => {
            let decoded =
                storage_value
                    .decode()
                    .map_err(|e| BrokerStorageError::StorageDecodeFailed {
                        pallet: "Broker",
                        entry: "SaleInfo",
                        details: e.to_string(),
                    })?;
            Ok(Some(decoded))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            tracing::debug!("Could not find Broker.SaleInfo storage entry.");
            Ok(None)
        }
        Err(e) => {
            tracing::debug!("Failed to retrieve Broker.SaleInfo: {:?}", format!("{e}"));
            Ok(None)
        }
    }
}

/// Fetches Broker::Status storage value.
///
/// Returns None if the status doesn't exist.
pub async fn get_status<T>(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<T>, BrokerStorageError>
where
    T: subxt::ext::scale_decode::DecodeAsType,
{
    let status_addr = subxt::dynamic::storage::<(), T>("Broker", "Status");

    match client_at_block.storage().fetch(status_addr, ()).await {
        Ok(storage_value) => {
            let decoded =
                storage_value
                    .decode()
                    .map_err(|e| BrokerStorageError::StorageDecodeFailed {
                        pallet: "Broker",
                        entry: "Status",
                        details: e.to_string(),
                    })?;
            Ok(Some(decoded))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            tracing::debug!("Could not find Broker.Status storage entry.");
            Ok(None)
        }
        Err(e) => {
            tracing::debug!("Failed to retrieve Broker.Status: {:?}", format!("{e}"));
            Ok(None)
        }
    }
}

/// Fetches TimeslicePeriod constant from Broker pallet.
pub async fn get_timeslice_period(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, BrokerStorageError> {
    let addr = subxt::dynamic::constant::<u32>("Broker", "TimeslicePeriod");
    client_at_block
        .constants()
        .entry(addr)
        .map_err(|_| BrokerStorageError::ConstantFetchFailed {
            pallet: "Broker",
            constant: "TimeslicePeriod",
        })
}

// ================================================================================================
// Region Types and Queries
// ================================================================================================

/// CoreMask size in bytes (80 bits = 10 bytes).
pub const CORE_MASK_SIZE: usize = 10;

/// RegionId from the Broker pallet storage key.
/// Matches the pallet_broker::RegionId type.
#[derive(Debug, Clone, Decode, Encode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct RegionId {
    /// The begin timeslice of this region.
    pub begin: u32,
    /// The core index this region is for.
    pub core: u16,
    /// The CoreMask (80 bits = 10 bytes).
    pub mask: [u8; CORE_MASK_SIZE],
}

/// RegionRecord from the Broker pallet storage value.
/// Matches the pallet_broker::RegionRecord<AccountId, Balance> type.
#[derive(Debug, Clone, Decode, Encode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct RegionRecord {
    /// The end timeslice of this region.
    pub end: u32,
    /// The owner of this region (Option<AccountId32>).
    pub owner: Option<[u8; 32]>,
    /// The amount paid for this region (optional).
    pub paid: Option<u128>,
}

/// A region entry combining the key (RegionId) and value (RegionRecord).
#[derive(Debug, Clone)]
pub struct RegionEntry {
    /// The region ID from the storage key.
    pub id: RegionId,
    /// The region record from the storage value (may be None if decoding fails).
    pub record: Option<RegionRecord>,
}

/// Fetches all regions from Broker::Regions storage map.
///
/// Returns a vector of RegionEntry containing both the key (RegionId) and value (RegionRecord).
pub async fn get_regions(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<RegionEntry>, BrokerStorageError> {
    let regions_addr = subxt::dynamic::storage::<(u32, u16, [u8; CORE_MASK_SIZE]), RegionRecord>(
        "Broker", "Regions",
    );

    let mut regions = Vec::new();

    let mut iter = client_at_block
        .storage()
        .iter(regions_addr, ())
        .await
        .map_err(|e| BrokerStorageError::StorageIterationError {
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

        regions.push(RegionEntry {
            id: region_id,
            record,
        });
    }

    Ok(regions)
}

// ================================================================================================
// Potential Renewal Types and Queries
// ================================================================================================

/// CoreMask type (80 bits = 10 bytes).
pub type CoreMask = [u8; CORE_MASK_SIZE];

/// Storage key data offset (pallet hash 16 + entry hash 16 + twox64 8 = 40).
const STORAGE_KEY_DATA_OFFSET: usize = 40;

/// Renewal key size (u16 core + u32 when = 6 bytes).
const RENEWAL_KEY_DATA_SIZE: usize = std::mem::size_of::<u16>() + std::mem::size_of::<u32>();

/// Minimum length of the storage key to extract renewal ID fields.
const RENEWAL_KEY_MIN_LENGTH: usize = STORAGE_KEY_DATA_OFFSET + RENEWAL_KEY_DATA_SIZE;

/// CompletionStatus enum matching the Broker pallet.
#[derive(Debug, Clone, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub enum CompletionStatus {
    Partial(CoreMask),
    Complete(Vec<ScheduleItem>),
}

/// PotentialRenewalRecord matching the Broker pallet storage value.
#[derive(Debug, Clone, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct PotentialRenewalRecord {
    pub price: u128,
    pub completion: CompletionStatus,
}

/// A potential renewal entry combining key and value.
#[derive(Debug, Clone)]
pub struct PotentialRenewalEntry {
    /// The core index.
    pub core: u32,
    /// The timeslice when this renewal becomes available.
    pub when: u32,
    /// The renewal record.
    pub record: PotentialRenewalRecord,
}

/// Fetches all potential renewals from Broker::PotentialRenewals storage map.
pub async fn get_potential_renewals(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<PotentialRenewalEntry>, BrokerStorageError> {
    let renewals_addr = subxt::dynamic::storage::<(u16, u32), PotentialRenewalRecord>(
        "Broker",
        "PotentialRenewals",
    );

    let mut renewals = Vec::new();

    let mut iter = client_at_block
        .storage()
        .iter(renewals_addr, ())
        .await
        .map_err(|e| BrokerStorageError::StorageIterationError {
            pallet: "Broker",
            entry: "PotentialRenewals",
            details: e.to_string(),
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

        renewals.push(PotentialRenewalEntry { core, when, record });
    }

    Ok(renewals)
}

/// Extracts (core, when) from storage key bytes using SCALE decoding.
fn extract_renewal_key(key_bytes: &[u8]) -> Option<(u32, u32)> {
    if key_bytes.len() < RENEWAL_KEY_MIN_LENGTH {
        return None;
    }

    // Position cursor at the start of the key data (after pallet hash + entry hash + twox64)
    let cursor = &mut &key_bytes[STORAGE_KEY_DATA_OFFSET..];

    // Decode core (u16) and when (u32) using SCALE codec
    let core = u16::decode(cursor).ok()? as u32;
    let when = u32::decode(cursor).ok()?;

    Some((core, when))
}

// ================================================================================================
// Workload and Workplan Full Iteration
// ================================================================================================

/// Workload entry with full schedule information.
#[derive(Debug, Clone)]
pub struct WorkloadWithSchedule {
    /// The core index.
    pub core: u32,
    /// The schedule items.
    pub items: Vec<ScheduleItem>,
}

/// Workplan entry with full schedule information.
#[derive(Debug, Clone)]
pub struct WorkplanWithSchedule {
    /// The core index.
    pub core: u32,
    /// The timeslice this workplan is for.
    pub timeslice: u32,
    /// The schedule items.
    pub items: Vec<ScheduleItem>,
}

/// Fetches all workload entries from Broker::Workload storage with full schedule data.
pub async fn iter_workloads_full(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<WorkloadWithSchedule>, BrokerStorageError> {
    let workload_addr = subxt::dynamic::storage::<(u16,), Vec<ScheduleItem>>("Broker", "Workload");

    let mut workloads = Vec::new();

    let mut iter = client_at_block
        .storage()
        .iter(workload_addr, ())
        .await
        .map_err(|e| BrokerStorageError::StorageIterationError {
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

        // Extract core from key using subxt's structured key API
        let core: u32 = match entry
            .key()
            .ok()
            .and_then(|k| k.part(0))
            .and_then(|p| p.decode_as::<u16>().ok().flatten())
        {
            Some(c) => c as u32,
            None => continue,
        };

        // Decode workload value as Vec<ScheduleItem> using DecodeAsType
        let items = match entry.value().decode_as::<Vec<ScheduleItem>>() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to decode workload for core {}: {:?}", core, e);
                Vec::new()
            }
        };

        workloads.push(WorkloadWithSchedule { core, items });
    }

    // Sort by core
    workloads.sort_by_key(|w| w.core);

    Ok(workloads)
}

/// Fetches all workplan entries from Broker::Workplan storage with full schedule data.
pub async fn iter_workplans(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<WorkplanWithSchedule>, BrokerStorageError> {
    let workplan_addr =
        subxt::dynamic::storage::<(u32, u16), Vec<ScheduleItem>>("Broker", "Workplan");

    let mut workplans = Vec::new();

    let mut iter = client_at_block
        .storage()
        .iter(workplan_addr, ())
        .await
        .map_err(|e| BrokerStorageError::StorageIterationError {
            pallet: "Broker",
            entry: "Workplan",
            details: e.to_string(),
        })?;

    while let Some(result) = iter.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Error iterating workplan: {:?}", e);
                continue;
            }
        };

        // Extract (timeslice, core) from key
        let key = match entry.key() {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("Failed to parse workplan key: {:?}", e);
                continue;
            }
        };

        // Try to decode as tuple first (single key component)
        let (timeslice, core): (u32, u32) = if let Some((t, c)) = key
            .part(0)
            .and_then(|p| p.decode_as::<(u32, u16)>().ok().flatten())
        {
            (t, c as u32)
        } else {
            // Fallback: try as separate key parts
            let timeslice = match key
                .part(0)
                .and_then(|p| p.decode_as::<u32>().ok().flatten())
            {
                Some(t) => t,
                None => {
                    tracing::warn!("Failed to decode workplan timeslice");
                    continue;
                }
            };
            let core = match key
                .part(1)
                .and_then(|p| p.decode_as::<u16>().ok().flatten())
            {
                Some(c) => c as u32,
                None => {
                    tracing::warn!("Failed to decode workplan core");
                    continue;
                }
            };
            (timeslice, core)
        };

        // Decode workplan value using DecodeAsType
        let items = match entry.value().decode_as::<Vec<ScheduleItem>>() {
            Ok(v) => v,
            Err(_) => {
                // OptionQuery might wrap the value
                match entry.value().decode_as::<Option<Vec<ScheduleItem>>>() {
                    Ok(Some(v)) => v,
                    Ok(None) => Vec::new(),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to decode workplan for timeslice {}, core {}: {:?}",
                            timeslice,
                            core,
                            e
                        );
                        Vec::new()
                    }
                }
            }
        };

        // Only add non-empty workplans
        if !items.is_empty() {
            workplans.push(WorkplanWithSchedule {
                core,
                timeslice,
                items,
            });
        }
    }

    // Sort by core, then timeslice
    workplans.sort_by(|a, b| a.core.cmp(&b.core).then(a.timeslice.cmp(&b.timeslice)));

    Ok(workplans)
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_assignment_to_task_string() {
        assert_eq!(CoreAssignment::Idle.to_task_string(), "idle");
        assert_eq!(CoreAssignment::Pool.to_task_string(), "pool");
        assert_eq!(CoreAssignment::Task(1000).to_task_string(), "1000");
    }

    // ------------------------------------------------------------------------
    // extract_task_from_workload tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_extract_task_from_workload_empty() {
        assert_eq!(extract_task_from_workload(vec![]), None);
    }

    #[test]
    fn test_extract_task_from_workload_task() {
        let items = vec![ScheduleItem {
            mask: [0u8; CORE_MASK_SIZE],
            assignment: CoreAssignment::Task(2000),
        }];
        assert_eq!(extract_task_from_workload(items), Some(2000));
    }

    #[test]
    fn test_extract_task_from_workload_idle() {
        let items = vec![ScheduleItem {
            mask: [0u8; CORE_MASK_SIZE],
            assignment: CoreAssignment::Idle,
        }];
        assert_eq!(extract_task_from_workload(items), None);
    }

    #[test]
    fn test_extract_task_from_workload_pool() {
        let items = vec![ScheduleItem {
            mask: [0u8; CORE_MASK_SIZE],
            assignment: CoreAssignment::Pool,
        }];
        assert_eq!(extract_task_from_workload(items), None);
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
    // extract_renewal_key tests
    // ------------------------------------------------------------------------

    /// Helper to create a mock storage key with the given core and when values.
    fn make_storage_key(core: u16, when: u32) -> Vec<u8> {
        let mut key_bytes = vec![0u8; STORAGE_KEY_DATA_OFFSET];
        // Append SCALE-encoded core and when
        key_bytes.extend(core.encode());
        key_bytes.extend(when.encode());
        key_bytes
    }

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
        let key_bytes = vec![0u8; RENEWAL_KEY_MIN_LENGTH - 1];
        assert_eq!(extract_renewal_key(&key_bytes), None);
    }

    #[test]
    fn test_extract_renewal_key_empty() {
        assert_eq!(extract_renewal_key(&[]), None);
    }

    // ------------------------------------------------------------------------
    // Storage key constants tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_storage_key_constants() {
        // STORAGE_KEY_DATA_OFFSET = pallet hash (16) + entry hash (16) + twox64 (8) = 40
        assert_eq!(STORAGE_KEY_DATA_OFFSET, 40);

        // RENEWAL_KEY_DATA_SIZE = sizeof(u16) + sizeof(u32) = 2 + 4 = 6
        assert_eq!(RENEWAL_KEY_DATA_SIZE, 6);

        // RENEWAL_KEY_MIN_LENGTH = STORAGE_KEY_DATA_OFFSET + RENEWAL_KEY_DATA_SIZE = 40 + 6 = 46
        assert_eq!(RENEWAL_KEY_MIN_LENGTH, 46);
    }
}
