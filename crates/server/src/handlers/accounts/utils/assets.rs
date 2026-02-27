// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Assets data fetching utilities for accounts handlers.
//!
//! This module provides wrapper functions that delegate to the centralized
//! `runtime_queries::assets` module for storage queries.

use crate::handlers::accounts::{AccountsError, AssetBalance};
use crate::handlers::runtime_queries::assets as assets_queries;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

/// Fetch all asset IDs from storage.
///
/// Delegates to `runtime_queries::assets::get_all_asset_ids`.
pub async fn query_all_assets_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    assets_queries::get_all_asset_ids(client_at_block)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Query asset balances for an account.
///
/// Delegates to `runtime_queries::assets::get_asset_balances`.
pub async fn query_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    assets: &[u32],
) -> Result<Vec<AssetBalance>, AccountsError> {
    let balances = assets_queries::get_asset_balances(client_at_block, account, assets)
        .await
        .map_err(|_| {
            AccountsError::DecodeFailed(parity_scale_codec::Error::from(
                "Failed to query asset balances",
            ))
        })?;

    Ok(balances
        .into_iter()
        .map(|(asset_id, decoded)| AssetBalance {
            asset_id,
            balance: decoded.balance,
            is_frozen: decoded.is_frozen,
            is_sufficient: decoded.is_sufficient,
        })
        .collect())
}
