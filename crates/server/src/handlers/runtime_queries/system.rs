// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! System pallet storage query functions.
//!
//! This module provides standalone functions for querying System pallet storage items.
//!
//! # Storage Items Covered
//! - `System::Events` - Block events

use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying System pallet storage.
#[derive(Debug, Error)]
pub enum SystemStorageError {
    /// Failed to fetch storage.
    #[error("Failed to fetch System::{entry}")]
    StorageFetchFailed { entry: &'static str },

    /// Storage error.
    #[error("Storage error: {0}")]
    StorageError(#[from] subxt::error::StorageError),
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Fetches raw events from System::Events storage.
///
/// Returns the raw storage value that can be decoded/visited by the caller.
/// This allows callers to use custom visitors for type-aware decoding.
pub async fn get_events_raw(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<subxt::storage::StorageValue<'_, scale_value::Value>, SystemStorageError> {
    let addr = subxt::dynamic::storage::<(), scale_value::Value>("System", "Events");
    client_at_block
        .storage()
        .fetch(addr, ())
        .await
        .map_err(SystemStorageError::from)
}
