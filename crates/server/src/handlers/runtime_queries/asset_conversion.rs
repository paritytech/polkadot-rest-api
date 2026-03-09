// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! AssetConversion pallet storage query functions.
//!
//! This module provides standalone functions for querying AssetConversion pallet storage items.

use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// AssetConversion Pallet Queries
// ================================================================================================

/// Get the next pool asset ID from AssetConversion::NextPoolAssetId.
/// Returns the next available pool asset ID for liquidity pools.
pub async fn get_next_pool_asset_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<u32> {
    let storage_addr = subxt::dynamic::storage::<(), u32>("AssetConversion", "NextPoolAssetId");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    value.decode().ok()
}
