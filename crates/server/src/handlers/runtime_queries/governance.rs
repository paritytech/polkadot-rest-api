// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Governance pallet storage query functions.
//!
//! This module provides standalone functions for querying governance-related storage items
//! including Referenda pallet queries.

use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// Referenda Pallet Queries
// ================================================================================================

/// Get the referendum count from Referenda::ReferendumCount.
/// Returns the total number of referenda that have been created.
pub async fn get_referendum_count(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<u32> {
    let storage_addr = subxt::dynamic::storage::<(), u32>("Referenda", "ReferendumCount");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    value.decode().ok()
}
