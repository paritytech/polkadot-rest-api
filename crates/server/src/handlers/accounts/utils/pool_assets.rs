// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pool Assets data fetching utilities for accounts handlers.
//!
//! This module provides wrapper functions that delegate to the centralized
//! `runtime_queries::pool_assets` module for storage queries.

use crate::handlers::accounts::{AccountsError, PoolAssetBalance};
use crate::handlers::runtime_queries::pool_assets as pool_assets_queries;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

/// Fetch all pool asset IDs from storage.
///
/// Delegates to `runtime_queries::pool_assets::get_all_pool_asset_ids`.
pub async fn query_all_pool_assets_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    pool_assets_queries::get_all_pool_asset_ids(client_at_block)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Query pool asset balances for an account.
///
/// Delegates to `runtime_queries::pool_assets::get_pool_asset_balances`.
pub async fn query_pool_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    assets: &[u32],
) -> Result<Vec<PoolAssetBalance>, AccountsError> {
    let balances = pool_assets_queries::get_pool_asset_balances(client_at_block, account, assets)
        .await
        .map_err(|_| {
            AccountsError::DecodeFailed(parity_scale_codec::Error::from(
                "Failed to query pool asset balances",
            ))
        })?;

    Ok(balances
        .into_iter()
        .map(|(asset_id, decoded)| PoolAssetBalance {
            asset_id,
            balance: decoded.balance,
            is_frozen: decoded.is_frozen,
            is_sufficient: decoded.is_sufficient,
        })
        .collect())
}
