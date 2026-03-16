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
        Err(e) => {
            tracing::debug!("Failed to fetch Coretime::BrokerId constant: {e:?}");
            Err(CoretimeStorageError::ConstantFetchFailed {
                pallet: "Coretime",
                constant: "BrokerId",
            })
        }
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
        Err(e) => {
            tracing::debug!("Failed to fetch MaxHistoricalRevenue constant: {e:?}");
            Ok(None)
        }
    }
}
