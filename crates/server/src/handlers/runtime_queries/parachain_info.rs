// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! ParachainInfo pallet storage query functions.
//!
//! This module provides standalone functions for querying ParachainInfo pallet storage items.
//!
//! # Storage Items Covered
//! - `ParachainInfo::ParachainId` - The parachain's ID

use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying ParachainInfo pallet storage.
#[derive(Debug, Error)]
pub enum ParachainInfoStorageError {
    /// The ParachainInfo pallet is not available (not a parachain).
    #[error("ParachainInfo pallet not available - not a parachain")]
    PalletNotAvailable,

    /// Failed to fetch storage.
    #[error("Failed to fetch ParachainInfo::{entry}")]
    StorageFetchFailed { entry: &'static str },

    /// Failed to decode storage value.
    #[error("Failed to decode ParachainInfo::{entry}: {details}")]
    StorageDecodeFailed { entry: &'static str, details: String },
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Fetches the parachain ID from ParachainInfo::ParachainId storage.
///
/// Returns an error if the pallet is not available (i.e., not a parachain).
pub async fn get_parachain_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, ParachainInfoStorageError> {
    let addr = subxt::dynamic::storage::<(), u32>("ParachainInfo", "ParachainId");

    let result = client_at_block
        .storage()
        .fetch(addr, ())
        .await
        .map_err(|_| ParachainInfoStorageError::PalletNotAvailable)?;

    result.decode().map_err(|e| {
        ParachainInfoStorageError::StorageDecodeFailed {
            entry: "ParachainId",
            details: e.to_string(),
        }
    })
}
