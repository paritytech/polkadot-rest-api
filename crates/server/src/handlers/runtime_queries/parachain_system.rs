// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! ParachainSystem pallet storage query functions.
//!
//! This module provides standalone functions for querying ParachainSystem pallet storage items.
//!
//! # Storage Items Covered
//! - `ParachainSystem::LastRelayChainBlockNumber` - Last relay chain block number
//!
//! Note: `ParachainInfo::ParachainId` queries are in the `parachain_info` module.

use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying parachain storage.
#[derive(Debug, Error)]
pub enum ParachainStorageError {
    /// The pallet is not available on this chain.
    #[error("{pallet} pallet not available")]
    PalletNotAvailable { pallet: &'static str },

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
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Fetches the last relay chain block number from ParachainSystem pallet.
///
/// This is used on coretime parachains where sale_start and leadin_length
/// are stored as relay chain block numbers.
pub async fn get_last_relay_block_number(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u32>, ParachainStorageError> {
    let addr = subxt::dynamic::storage::<(), u32>("ParachainSystem", "LastRelayChainBlockNumber");

    match client_at_block.storage().fetch(addr, ()).await {
        Ok(storage_value) => {
            let decoded =
                storage_value
                    .decode()
                    .map_err(|e| ParachainStorageError::StorageDecodeFailed {
                        pallet: "ParachainSystem",
                        entry: "LastRelayChainBlockNumber",
                        details: e.to_string(),
                    })?;
            Ok(Some(decoded))
        }
        Err(e) => {
            tracing::debug!(
                "Failed to retrieve ParachainSystem.LastRelayChainBlockNumber: {:?}",
                format!("{e}")
            );
            Ok(None)
        }
    }
}
