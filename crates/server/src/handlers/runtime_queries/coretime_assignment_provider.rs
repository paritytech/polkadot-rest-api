// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! CoretimeAssignmentProvider pallet storage query functions.
//!
//! This module provides standalone functions for querying CoretimeAssignmentProvider
//! pallet storage items used by relay chain coretime endpoints.
//!
//! # Storage Items Covered
//! - `CoretimeAssignmentProvider::CoreDescriptors` - Core assignment descriptors

use parity_scale_codec::Decode;
use subxt::ext::scale_decode::DecodeAsType;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying CoretimeAssignmentProvider pallet storage.
#[derive(Debug, Error)]
pub enum CoretimeAssignmentProviderStorageError {
    /// Failed to fetch storage.
    #[error("Failed to fetch CoretimeAssignmentProvider::{entry}")]
    StorageFetchFailed { entry: &'static str },

    /// Failed to decode storage value.
    #[error("Failed to decode CoretimeAssignmentProvider::{entry}: {details}")]
    StorageDecodeFailed { entry: &'static str, details: String },
}

// ================================================================================================
// SCALE Decode Types
// ================================================================================================

/// Internal type for decoding CoreAssignment from relay chain.
/// Matches pallet_broker::CoreAssignment.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub enum RelayCoreAssignment {
    Idle,
    Pool,
    Task(u32),
}

/// Internal type for decoding AssignmentState from relay chain.
/// Note: ratio and remaining are PartsOf57600(u16) on-chain, DecodeAsType handles newtype unwrapping.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct RelayAssignmentState {
    pub ratio: u16,
    pub remaining: u16,
}

/// Internal type for decoding WorkState from relay chain.
/// Note: assignments is Vec<(CoreAssignment, AssignmentState)> - a Vec of TUPLES.
/// step is PartsOf57600(u16) on-chain.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct RelayWorkState {
    /// Assignments as tuples (CoreAssignment, AssignmentState)
    pub assignments: Vec<(RelayCoreAssignment, RelayAssignmentState)>,
    pub end_hint: Option<u32>,
    pub pos: u16,
    pub step: u16,
}

/// Internal type for decoding QueueDescriptor from relay chain.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct RelayQueueState {
    pub first: u32,
    pub last: u32,
}

/// Internal type for decoding CoreDescriptor from relay chain.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct RelayCoreDescriptorRaw {
    pub queue: Option<RelayQueueState>,
    pub current_work: Option<RelayWorkState>,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Fetches all core descriptors from CoretimeAssignmentProvider::CoreDescriptors storage.
///
/// Note: CoreDescriptors uses Twox256 hasher which is opaque - the key cannot be extracted
/// from the storage key. We query specific core indices directly instead of iterating.
///
/// Queries are sent in parallel batches to minimize round-trip latency. Each batch fires
/// BATCH_SIZE concurrent RPC requests. If an entire batch returns only empty descriptors
/// (after we've already found some), we stop — no more cores exist beyond that point.
pub async fn get_core_descriptors(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<(u32, RelayCoreDescriptorRaw)>, CoretimeAssignmentProviderStorageError> {
    // Batch size for parallel queries. Sends this many concurrent RPC requests per round.
    // Typical relay chains have ~80-100 active cores, so 2-3 batches suffice.
    const BATCH_SIZE: u32 = 50;
    // Safety ceiling — stop even if batches never come back fully empty.
    const MAX_CORES: u32 = 500;

    let addr = subxt::dynamic::storage::<(u32,), RelayCoreDescriptorRaw>(
        "CoretimeAssignmentProvider",
        "CoreDescriptors",
    );

    let mut descriptors = Vec::new();
    let mut batch_start = 0u32;

    loop {
        let batch_end = (batch_start + BATCH_SIZE).min(MAX_CORES);

        // Fire all queries in this batch concurrently (batch is already
        // bounded by BATCH_SIZE so no extra concurrency limiting needed)
        let futures: Vec<_> = (batch_start..batch_end)
            .map(|core_idx| {
                let addr = addr.clone();
                async move {
                    let result = client_at_block.storage().fetch(addr, (core_idx,)).await;
                    (core_idx, result)
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        let mut batch_found_any = false;
        for (core_idx, result) in results {
            match result {
                Ok(value) => match value.decode_as::<RelayCoreDescriptorRaw>() {
                    Ok(descriptor) => {
                        // CoreDescriptors uses ValueQuery: non-existent cores return
                        // a default descriptor with queue: None, current_work: None.
                        let is_empty =
                            descriptor.current_work.is_none() && descriptor.queue.is_none();
                        if !is_empty {
                            batch_found_any = true;
                            descriptors.push((core_idx, descriptor));
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to decode core descriptor for core {}: {:?}",
                            core_idx,
                            e
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "Error fetching core descriptor for core {}: {:?}",
                        core_idx,
                        e
                    );
                }
            }
        }

        batch_start = batch_end;

        // Stop if this entire batch was empty (after we've found at least some cores)
        // or we've reached the safety ceiling.
        if (!batch_found_any && !descriptors.is_empty()) || batch_start >= MAX_CORES {
            break;
        }
    }

    // Sort by core index (parallel results may arrive out of order)
    descriptors.sort_by_key(|(core, _)| *core);

    Ok(descriptors)
}
