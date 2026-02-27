// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common vesting info utilities shared across handler modules.

use crate::handlers::runtime_queries::balances as balances_queries;
use crate::utils::ResolvedBlock;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum VestingQueryError {
    #[error("The runtime does not include the vesting pallet at this block")]
    VestingPalletNotAvailable,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(Box<subxt::error::OnlineClientAtBlockError>),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(Box<subxt::error::StorageError>),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),
}

impl From<subxt::error::OnlineClientAtBlockError> for VestingQueryError {
    fn from(err: subxt::error::OnlineClientAtBlockError) -> Self {
        VestingQueryError::ClientAtBlockFailed(Box::new(err))
    }
}

impl From<subxt::error::StorageError> for VestingQueryError {
    fn from(err: subxt::error::StorageError) -> Self {
        VestingQueryError::StorageQueryFailed(Box::new(err))
    }
}

// ================================================================================================
// Data Types
// ================================================================================================

/// Raw vesting info data returned from storage query
#[derive(Debug)]
pub struct RawVestingInfo {
    /// Block information
    pub block: FormattedBlockInfo,
    /// Vesting schedules
    pub schedules: Vec<DecodedVestingSchedule>,
}

/// Block information for response
#[derive(Debug, Clone)]
pub struct FormattedBlockInfo {
    pub hash: String,
    pub number: u64,
}

/// Decoded vesting schedule from storage
#[derive(Debug, Clone)]
pub struct DecodedVestingSchedule {
    /// Total tokens locked at start of vesting
    pub locked: String,
    /// Tokens unlocked per block
    pub per_block: String,
    /// Block when vesting begins
    pub starting_block: String,
}

// ================================================================================================
// Core Query Function
// ================================================================================================

/// Query vesting info from storage
///
/// This is the shared function used by both `/accounts/:accountId/vesting-info`
/// and `/rc/accounts/:accountId/vesting-info` endpoints.
///
/// Parameters:
/// - `client`: The subxt client to query
/// - `account`: The account to query vesting for
/// - `block`: The block to query at
pub async fn query_vesting_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &ResolvedBlock,
) -> Result<RawVestingInfo, VestingQueryError> {
    // Check if Vesting pallet exists
    if client_at_block
        .storage()
        .entry(("Vesting", "Vesting"))
        .is_err()
    {
        return Err(VestingQueryError::VestingPalletNotAvailable);
    }

    // Use centralized query function
    let vesting_schedules = balances_queries::get_vesting_schedules(client_at_block, account).await;

    // Convert to decoded schedules
    let schedules: Vec<DecodedVestingSchedule> = vesting_schedules
        .iter()
        .map(|s| DecodedVestingSchedule {
            locked: s.locked.to_string(),
            per_block: s.per_block.to_string(),
            starting_block: s.starting_block.to_string(),
        })
        .collect();

    Ok(RawVestingInfo {
        block: FormattedBlockInfo {
            hash: block.hash.clone(),
            number: block.number,
        },
        schedules,
    })
}
