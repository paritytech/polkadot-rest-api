// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Coretime pallet storage query functions.
//!
//! This module provides standalone functions for querying Coretime-related pallet storage items
//! on relay chains and coretime parachains.
//!
//! # Storage Items Covered
//! - `Coretime::BrokerId` - Broker parachain ID constant
//! - `CoretimeAssignmentProvider::CoreDescriptors` - Core descriptor information
//! - `CoretimeAssignmentProvider` storage version
//! - `OnDemand::MaxHistoricalRevenue` / `OnDemandAssignmentProvider::MaxHistoricalRevenue`

use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying Coretime pallet storage.
#[derive(Debug, Error)]
pub enum CoretimeStorageError {
    /// The Coretime pallet is not available on this chain.
    #[error("Coretime pallet not available")]
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
// Storage Query Functions
// ================================================================================================

/// Fetches BrokerId constant from Coretime pallet.
pub async fn get_broker_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u32>, CoretimeStorageError> {
    let addr = subxt::dynamic::constant::<u32>("Coretime", "BrokerId");
    match client_at_block.constants().entry(addr) {
        Ok(value) => Ok(Some(value)),
        Err(_) => Err(CoretimeStorageError::ConstantFetchFailed {
            pallet: "Coretime",
            constant: "BrokerId",
        }),
    }
}

/// Fetches storage version for CoretimeAssignmentProvider pallet.
pub async fn get_assignment_provider_storage_version(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u16>, CoretimeStorageError> {
    let version = client_at_block
        .storage()
        .storage_version("CoretimeAssignmentProvider")
        .await
        .map_err(|_| CoretimeStorageError::StorageVersionFetchFailed {
            pallet: "CoretimeAssignmentProvider",
        })?;

    Ok(Some(version))
}

/// Fetches MaxHistoricalRevenue constant from OnDemand or OnDemandAssignmentProvider.
///
/// Tries the newer "OnDemand" pallet first, then falls back to legacy "OnDemandAssignmentProvider".
pub async fn get_max_historical_revenue(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u32>, CoretimeStorageError> {
    // Try new pallet name first
    let addr = subxt::dynamic::constant::<u32>("OnDemand", "MaxHistoricalRevenue");
    if let Ok(value) = client_at_block.constants().entry(addr) {
        return Ok(Some(value));
    }

    // Fall back to legacy pallet name
    let legacy_addr =
        subxt::dynamic::constant::<u32>("OnDemandAssignmentProvider", "MaxHistoricalRevenue");
    match client_at_block.constants().entry(legacy_addr) {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

/// Fetches a core descriptor from CoretimeAssignmentProvider::CoreDescriptors.
///
/// Returns None if the core doesn't exist.
pub async fn get_core_descriptor<T>(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    core_idx: u32,
) -> Result<Option<T>, CoretimeStorageError>
where
    T: subxt::ext::scale_decode::DecodeAsType,
{
    let addr =
        subxt::dynamic::storage::<(u32,), T>("CoretimeAssignmentProvider", "CoreDescriptors");

    match client_at_block.storage().fetch(addr, (core_idx,)).await {
        Ok(value) => {
            let decoded =
                value
                    .decode_as::<T>()
                    .map_err(|e| CoretimeStorageError::StorageDecodeFailed {
                        pallet: "CoretimeAssignmentProvider",
                        entry: "CoreDescriptors",
                        details: e.to_string(),
                    })?;
            Ok(Some(decoded))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => Ok(None),
        Err(_) => Err(CoretimeStorageError::StorageFetchFailed {
            pallet: "CoretimeAssignmentProvider",
            entry: "CoreDescriptors",
        }),
    }
}

/// Fetches core descriptors for a batch of cores in parallel.
///
/// Returns a vector of (core_index, descriptor) pairs for cores that have data.
/// Cores with empty descriptors (queue: None, current_work: None) are filtered out.
pub async fn get_core_descriptors_batch<T>(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    start_idx: u32,
    end_idx: u32,
) -> Vec<(u32, T)>
where
    T: subxt::ext::scale_decode::DecodeAsType + Send + 'static,
{
    let addr =
        subxt::dynamic::storage::<(u32,), T>("CoretimeAssignmentProvider", "CoreDescriptors");

    let futures: Vec<_> = (start_idx..end_idx)
        .map(|core_idx| {
            let addr = addr.clone();
            async move {
                let result = client_at_block.storage().fetch(addr, (core_idx,)).await;
                (core_idx, result)
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    let mut descriptors = Vec::new();
    for (core_idx, result) in results {
        if let Ok(value) = result
            && let Ok(descriptor) = value.decode_as::<T>()
        {
            descriptors.push((core_idx, descriptor));
        }
    }

    descriptors
}
