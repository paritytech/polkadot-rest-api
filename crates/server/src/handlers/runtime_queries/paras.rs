// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Paras pallet storage query functions.
//!
//! This module provides standalone functions for querying Paras pallet storage items
//! on relay chains.
//!
//! # Storage Items Covered
//! - `Paras::ParaLifecycles` - Parachain lifecycle states

use subxt::ext::scale_decode::DecodeAsType;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying Paras pallet storage.
#[derive(Debug, Error)]
pub enum ParasStorageError {
    /// The Paras pallet is not available on this chain.
    #[error("Paras pallet not available")]
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
}

// ================================================================================================
// SCALE Decode Types
// ================================================================================================

/// On-chain ParaLifecycle enum.
/// Matches polkadot_runtime_parachains::paras::ParaLifecycle.
#[derive(Debug, Clone, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub enum ParaLifecycleType {
    Onboarding,
    Parathread,
    Parachain,
    UpgradingParathread,
    DowngradingParachain,
    OffboardingParathread,
    OffboardingParachain,
}

impl ParaLifecycleType {
    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            ParaLifecycleType::Onboarding => "Onboarding",
            ParaLifecycleType::Parathread => "Parathread",
            ParaLifecycleType::Parachain => "Parachain",
            ParaLifecycleType::UpgradingParathread => "UpgradingParathread",
            ParaLifecycleType::DowngradingParachain => "DowngradingParachain",
            ParaLifecycleType::OffboardingParathread => "OffboardingParathread",
            ParaLifecycleType::OffboardingParachain => "OffboardingParachain",
        }
    }
}

// ================================================================================================
// Output Types
// ================================================================================================

/// Parachain lifecycle information.
#[derive(Debug, Clone)]
pub struct ParaLifecycle {
    /// The parachain ID.
    pub para_id: u32,
    /// The lifecycle type as a string.
    pub lifecycle_type: Option<String>,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Checks if the Paras pallet exists on this chain.
pub fn has_paras_pallet(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> bool {
    client_at_block
        .metadata()
        .pallet_by_name("Paras")
        .is_some()
}

/// Fetches all parachain lifecycles from Paras::ParaLifecycles storage.
pub async fn get_para_lifecycles(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<ParaLifecycle>, ParasStorageError> {
    if !has_paras_pallet(client_at_block) {
        return Ok(Vec::new());
    }

    let lifecycles_addr =
        subxt::dynamic::storage::<(u32,), ParaLifecycleType>("Paras", "ParaLifecycles");

    let mut lifecycles = Vec::new();

    let mut iter = client_at_block
        .storage()
        .iter(lifecycles_addr, ())
        .await
        .map_err(|e| ParasStorageError::StorageIterationError {
            pallet: "Paras",
            entry: "ParaLifecycles",
            details: e.to_string(),
        })?;

    while let Some(result) = iter.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Error iterating para lifecycles: {:?}", e);
                continue;
            }
        };

        // Extract para_id from storage key
        let para_id: u32 = match entry
            .key()
            .ok()
            .and_then(|k| k.part(0))
            .and_then(|p| p.decode_as::<u32>().ok().flatten())
        {
            Some(id) => id,
            None => {
                tracing::warn!("Failed to decode ParaId from key");
                continue;
            }
        };

        // Decode lifecycle type
        let lifecycle_type = entry
            .value()
            .decode_as::<ParaLifecycleType>()
            .ok()
            .map(|lt| lt.as_str().to_string());

        lifecycles.push(ParaLifecycle {
            para_id,
            lifecycle_type,
        });
    }

    Ok(lifecycles)
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_para_lifecycle_type_as_str() {
        assert_eq!(ParaLifecycleType::Onboarding.as_str(), "Onboarding");
        assert_eq!(ParaLifecycleType::Parachain.as_str(), "Parachain");
        assert_eq!(ParaLifecycleType::Parathread.as_str(), "Parathread");
        assert_eq!(
            ParaLifecycleType::UpgradingParathread.as_str(),
            "UpgradingParathread"
        );
        assert_eq!(
            ParaLifecycleType::DowngradingParachain.as_str(),
            "DowngradingParachain"
        );
    }
}
