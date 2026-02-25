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

use parity_scale_codec::Decode;
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
#[derive(Debug, Clone, Decode)]
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

/// Workload schedule item with assignment info.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct WorkloadScheduleItem {
    pub mask: [u8; 10],
    pub assignment: WorkloadAssignment,
}

/// Workload assignment variants.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub enum WorkloadAssignment {
    Idle,
    Pool,
    Task(u32),
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
            .decode_as::<Vec<WorkloadScheduleItem>>()
            .ok()
            .and_then(extract_task_from_workload);

        workloads.push(WorkloadInfo { core, task });
    }

    Ok(workloads)
}

/// Extracts the task ID from a typed workload schedule.
fn extract_task_from_workload(items: Vec<WorkloadScheduleItem>) -> Option<u32> {
    items.first().and_then(|item| match item.assignment {
        WorkloadAssignment::Task(id) => Some(id),
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
}
