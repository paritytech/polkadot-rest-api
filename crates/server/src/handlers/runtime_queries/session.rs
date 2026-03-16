// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Session pallet storage query functions.
//!
//! This module provides standalone functions for querying Session pallet storage items.
//!
//! # Storage Items Covered
//! - `Session::Validators` - Current validator set
//! - `Session::CurrentIndex` - Current session index

use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying Session pallet storage.
#[derive(Debug, Error)]
pub enum SessionStorageError {
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

    /// No validators found in storage.
    #[error("No validators found in storage")]
    NoValidatorsFound,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Fetches the current validator set from Session::Validators.
///
/// Returns the list of validators for the current session.
pub async fn get_validators(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<AccountId32>, SessionStorageError> {
    let addr = subxt::dynamic::storage::<(), Vec<[u8; 32]>>("Session", "Validators");

    let validators_raw = client_at_block
        .storage()
        .fetch(addr, ())
        .await
        .map_err(|_| SessionStorageError::StorageFetchFailed {
            pallet: "Session",
            entry: "Validators",
        })?
        .decode()
        .map_err(|e| SessionStorageError::StorageDecodeFailed {
            pallet: "Session",
            entry: "Validators",
            details: e.to_string(),
        })?;

    let validators: Vec<AccountId32> = validators_raw.into_iter().map(AccountId32::from).collect();

    if validators.is_empty() {
        return Err(SessionStorageError::NoValidatorsFound);
    }

    Ok(validators)
}

/// Fetches the current session index from Session::CurrentIndex.
///
/// Returns the index of the current session.
pub async fn get_current_index(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, SessionStorageError> {
    let addr = subxt::dynamic::storage::<(), u32>("Session", "CurrentIndex");

    client_at_block
        .storage()
        .fetch(addr, ())
        .await
        .map_err(|_| SessionStorageError::StorageFetchFailed {
            pallet: "Session",
            entry: "CurrentIndex",
        })?
        .decode()
        .map_err(|e| SessionStorageError::StorageDecodeFailed {
            pallet: "Session",
            entry: "CurrentIndex",
            details: e.to_string(),
        })
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_storage_error_display() {
        let err = SessionStorageError::StorageFetchFailed {
            pallet: "Session",
            entry: "Validators",
        };
        assert_eq!(err.to_string(), "Failed to fetch Session::Validators");

        let err = SessionStorageError::NoValidatorsFound;
        assert_eq!(err.to_string(), "No validators found in storage");
    }
}
