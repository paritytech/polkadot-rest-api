// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared utilities for balance info queries.
//!
//! This module provides common functionality for querying account balance information
//! that is shared between the regular accounts endpoint and the RC (relay chain) endpoint.

use crate::handlers::runtime_queries::balances as balances_queries;
use crate::utils::ResolvedBlock;
use serde::Serialize;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// Re-export types from centralized module for backward compatibility
pub use balances_queries::{DecodedAccountData, DecodedBalanceLock};

/// Raw balance info data (before formatting)
#[derive(Debug, Clone)]
pub struct RawBalanceInfo {
    pub block: ResolvedBlock,
    pub token_symbol: String,
    pub token_decimals: u8,
    pub account_data: DecodedAccountData,
    pub locks: Vec<DecodedBalanceLock>,
    pub transferable: String,
}

/// Formatted balance lock for API response
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormattedBalanceLock {
    pub id: String,
    pub amount: String,
    pub reasons: String,
}

/// Block info for API response
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormattedBlockInfo {
    pub hash: String,
    pub height: String,
}

/// Error type for balance info queries
#[derive(Debug, thiserror::Error)]
pub enum BalanceQueryError {
    #[error("The runtime does not include the balances pallet at this block")]
    BalancesPalletNotAvailable,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(Box<subxt::error::OnlineClientAtBlockError>),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(Box<subxt::error::StorageError>),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Failed to fetch ExistentialDeposit constant from runtime")]
    ExistentialDepositFetchFailed,
}

impl From<subxt::error::OnlineClientAtBlockError> for BalanceQueryError {
    fn from(err: subxt::error::OnlineClientAtBlockError) -> Self {
        BalanceQueryError::ClientAtBlockFailed(Box::new(err))
    }
}

impl From<subxt::error::StorageError> for BalanceQueryError {
    fn from(err: subxt::error::StorageError) -> Self {
        BalanceQueryError::StorageQueryFailed(Box::new(err))
    }
}

// ================================================================================================
// Main Query Function
// ================================================================================================

/// Query balance information for an account at a specific block.
///
/// This is the main shared function that queries account balance data.
/// It returns raw data that can be formatted into either the regular or RC response format.
pub async fn query_balance_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    spec_name: &str,
    account: &AccountId32,
    block: &ResolvedBlock,
    token: Option<String>,
) -> Result<RawBalanceInfo, BalanceQueryError> {
    // Check if System pallet exists
    if client_at_block
        .storage()
        .entry(("System", "Account"))
        .is_err()
    {
        return Err(BalanceQueryError::BalancesPalletNotAvailable);
    }

    // Get token symbol (default based on chain type)
    let token_symbol = token.unwrap_or_else(|| get_default_token_symbol(spec_name));

    // Get token decimals
    let token_decimals = get_default_token_decimals(spec_name);

    // Fetch existential deposit from runtime constants (sync - reads from metadata)
    let existential_deposit = fetch_existential_deposit(client_at_block)?;

    let (account_data, locks) = tokio::join!(
        balances_queries::get_account_data_or_default(client_at_block, account),
        balances_queries::get_balance_locks(client_at_block, account)
    );

    // Calculate transferable balance using the dynamically fetched ED
    let transferable = calculate_transferable(existential_deposit, &account_data);

    Ok(RawBalanceInfo {
        block: block.clone(),
        token_symbol,
        token_decimals,
        account_data,
        locks,
        transferable,
    })
}

// ================================================================================================
// Token/Decimals Helpers
// ================================================================================================

/// Get the default token symbol for a given spec name
pub fn get_default_token_symbol(spec_name: &str) -> String {
    let spec_lower = spec_name.to_lowercase();
    match spec_lower.as_str() {
        "polkadot" | "statemint" => "DOT".to_string(),
        "kusama" | "statemine" => "KSM".to_string(),
        "westend" | "westmint" => "WND".to_string(),
        "rococo" => "ROC".to_string(),
        "paseo" => "PAS".to_string(),
        _ => "UNIT".to_string(),
    }
}

/// Get the default token decimals for a given spec name
pub fn get_default_token_decimals(spec_name: &str) -> u8 {
    let spec_lower = spec_name.to_lowercase();

    match spec_lower.as_str() {
        "polkadot" | "statemint" => 10,
        "kusama" | "statemine" => 12,
        "westend" | "westmint" => 12,
        "rococo" => 12,
        "paseo" => 12,
        _ => 12,
    }
}

/// Get the default existential deposit for a given spec name
pub fn get_default_existential_deposit(spec_name: &str) -> u128 {
    let spec_lower = spec_name.to_lowercase();

    match spec_lower.as_str() {
        "polkadot" | "statemint" => 10_000_000_000, // 1 DOT = 10^10 planck, ED = 1 DOT on Polkadot Asset Hub
        "kusama" | "statemine" => 33_333_333,       // ~0.000033 KSM on Kusama Asset Hub
        "westend" | "westmint" => 1_000_000_000_000, // 1 WND on Westend
        _ => 1_000_000_000_000,                     // Default
    }
}

/// Fetch the ExistentialDeposit constant from the Balances pallet at a specific block.
///
/// This queries the runtime constants dynamically rather than using hardcoded values,
/// ensuring accuracy across different chains and runtime upgrades.
pub fn fetch_existential_deposit(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u128, BalanceQueryError> {
    let addr = subxt::dynamic::constant::<u128>("Balances", "ExistentialDeposit");
    client_at_block
        .constants()
        .entry(addr)
        .map_err(|_| BalanceQueryError::ExistentialDepositFetchFailed)
}

// ================================================================================================
// Transferable Calculation
// ================================================================================================

/// Calculate the transferable balance based on the account data and existential deposit.
///
/// For modern runtimes with the `frozen` field:
/// `transferable = free - max(maybeED, frozen - reserved)`
/// where `maybeED = 0` if `frozen == 0 && reserved == 0`, else `existential_deposit`
///
/// # Arguments
/// * `existential_deposit` - The chain's existential deposit, fetched dynamically from runtime constants
/// * `account_data` - The decoded account data containing free, reserved, and frozen balances
pub fn calculate_transferable(
    existential_deposit: u128,
    account_data: &DecodedAccountData,
) -> String {
    if let Some(frozen) = account_data.frozen {
        let no_frozen_reserved = frozen == 0 && account_data.reserved == 0;
        let maybe_ed = if no_frozen_reserved {
            0
        } else {
            existential_deposit
        };

        let frozen_reserve_diff = frozen.saturating_sub(account_data.reserved);
        let max_deduction = std::cmp::max(maybe_ed, frozen_reserve_diff);
        let transferable = account_data.free.saturating_sub(max_deduction);

        transferable.to_string()
    } else {
        // For older runtimes, we can't calculate transferable accurately
        "transferable formula not supported for this runtime".to_string()
    }
}

// ================================================================================================
// Response Formatting
// ================================================================================================

/// Apply denomination to a balance amount
pub fn apply_denomination(amount: u128, decimals: usize) -> String {
    let str_balance = amount.to_string();

    if str_balance == "0" || decimals == 0 {
        return str_balance;
    }

    if str_balance.len() <= decimals {
        let padding = decimals - str_balance.len();
        format!("0.{}{}", "0".repeat(padding), str_balance)
    } else {
        let split_point = str_balance.len() - decimals;
        format!(
            "{}.{}",
            &str_balance[..split_point],
            &str_balance[split_point..]
        )
    }
}

/// Format a balance amount based on denomination settings
pub fn format_balance(amount: u128, denominated: bool, decimals: u8) -> String {
    if denominated && decimals > 0 {
        apply_denomination(amount, decimals as usize)
    } else {
        amount.to_string()
    }
}

/// Format the transferable string
pub fn format_transferable(transferable: &str, denominated: bool, decimals: u8) -> String {
    if transferable.starts_with("transferable") {
        transferable.to_string()
    } else if denominated && decimals > 0 {
        if let Ok(amount) = transferable.parse::<u128>() {
            apply_denomination(amount, decimals as usize)
        } else {
            transferable.to_string()
        }
    } else {
        transferable.to_string()
    }
}

/// Format frozen fields based on runtime version
pub fn format_frozen_fields(
    account_data: &DecodedAccountData,
    denominated: bool,
    decimals: u8,
) -> (String, String, String) {
    if account_data.frozen.is_some() {
        // Newer runtime: has frozen, no miscFrozen/feeFrozen
        (
            "miscFrozen does not exist for this runtime".to_string(),
            "feeFrozen does not exist for this runtime".to_string(),
            format_balance(account_data.frozen.unwrap_or(0), denominated, decimals),
        )
    } else {
        // Older runtime: has miscFrozen/feeFrozen, no frozen
        (
            format_balance(account_data.misc_frozen.unwrap_or(0), denominated, decimals),
            format_balance(account_data.fee_frozen.unwrap_or(0), denominated, decimals),
            "frozen does not exist for this runtime".to_string(),
        )
    }
}

/// Format locks for API response
pub fn format_locks(
    locks: &[DecodedBalanceLock],
    denominated: bool,
    decimals: u8,
) -> Vec<FormattedBalanceLock> {
    locks
        .iter()
        .map(|lock| FormattedBalanceLock {
            id: lock.id.clone(),
            amount: format_balance(lock.amount, denominated, decimals),
            reasons: lock.reasons.clone(),
        })
        .collect()
}
