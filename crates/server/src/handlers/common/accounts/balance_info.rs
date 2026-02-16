// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared utilities for balance info queries.
//!
//! This module provides common functionality for querying account balance information
//! that is shared between the regular accounts endpoint and the RC (relay chain) endpoint.

use crate::utils::ResolvedBlock;
use parity_scale_codec::Decode;
use serde::Serialize;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// SCALE Decode Types for System::Account storage
// ================================================================================================

/// Account data for modern runtimes (with frozen field)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)] // Fields needed for SCALE decoding
struct AccountDataModern {
    free: u128,
    reserved: u128,
    frozen: u128,
    flags: u128,
}

/// Account data for legacy runtimes (with misc_frozen/fee_frozen fields)
#[derive(Debug, Clone, Decode)]
struct AccountDataLegacy {
    free: u128,
    reserved: u128,
    misc_frozen: u128,
    fee_frozen: u128,
}

/// Account info structure (modern runtime)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)] // Fields needed for SCALE decoding
struct AccountInfoModern {
    nonce: u32,
    consumers: u32,
    providers: u32,
    sufficients: u32,
    data: AccountDataModern,
}

/// Account info structure (legacy runtime)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)] // Fields needed for SCALE decoding
struct AccountInfoLegacy {
    nonce: u32,
    consumers: u32,
    providers: u32,
    sufficients: u32,
    data: AccountDataLegacy,
}

// ================================================================================================
// SCALE Decode Types for Balances::Locks storage
// ================================================================================================

/// Lock reasons enum
#[derive(Debug, Clone, Decode)]
enum LockReasons {
    Fee,
    Misc,
    All,
}

impl LockReasons {
    fn as_str(&self) -> &'static str {
        match self {
            LockReasons::Fee => "Fee",
            LockReasons::Misc => "Misc",
            LockReasons::All => "All",
        }
    }
}

/// Balance lock structure
#[derive(Debug, Clone, Decode)]
struct BalanceLock {
    id: [u8; 8],
    amount: u128,
    reasons: LockReasons,
}

// ================================================================================================
// Shared Types
// ================================================================================================

/// Decoded account data from storage
#[derive(Debug, Clone)]
pub struct DecodedAccountData {
    pub nonce: u32,
    pub free: u128,
    pub reserved: u128,
    pub misc_frozen: Option<u128>,
    pub fee_frozen: Option<u128>,
    pub frozen: Option<u128>,
}

/// Decoded balance lock from storage
#[derive(Debug, Clone)]
pub struct DecodedBalanceLock {
    pub id: String,
    pub amount: u128,
    pub reasons: String,
}

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

    // Query System::Account and Balances::Locks concurrently
    let (account_data_result, locks_result) = tokio::join!(
        query_account_data(client_at_block, account),
        query_balance_locks(client_at_block, account)
    );

    let account_data = account_data_result?;
    let locks = locks_result?;

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
// Account Data Query
// ================================================================================================

async fn query_account_data(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Result<DecodedAccountData, BalanceQueryError> {
    // Build the storage address for System::Account(account_id)
    let storage_addr = subxt::dynamic::storage::<_, ()>("System", "Account");
    let account_bytes: [u8; 32] = *account.as_ref();

    let storage_value = client_at_block
        .storage()
        .fetch(storage_addr, (account_bytes,))
        .await;

    if let Ok(value) = storage_value {
        let raw_bytes = value.into_bytes();
        decode_account_info(&raw_bytes)
    } else {
        // Return empty account data if account doesn't exist
        Ok(DecodedAccountData {
            nonce: 0,
            free: 0,
            reserved: 0,
            misc_frozen: None,
            fee_frozen: None,
            frozen: Some(0),
        })
    }
}

fn decode_account_info(raw_bytes: &[u8]) -> Result<DecodedAccountData, BalanceQueryError> {
    // Try modern format first (with frozen field)
    if let Ok(account_info) = AccountInfoModern::decode(&mut &raw_bytes[..]) {
        return Ok(DecodedAccountData {
            nonce: account_info.nonce,
            free: account_info.data.free,
            reserved: account_info.data.reserved,
            misc_frozen: None,
            fee_frozen: None,
            frozen: Some(account_info.data.frozen),
        });
    }

    // Fall back to legacy format (with misc_frozen/fee_frozen fields)
    if let Ok(account_info) = AccountInfoLegacy::decode(&mut &raw_bytes[..]) {
        return Ok(DecodedAccountData {
            nonce: account_info.nonce,
            free: account_info.data.free,
            reserved: account_info.data.reserved,
            misc_frozen: Some(account_info.data.misc_frozen),
            fee_frozen: Some(account_info.data.fee_frozen),
            frozen: None,
        });
    }

    // If neither format works, return an error
    Err(BalanceQueryError::DecodeFailed(
        parity_scale_codec::Error::from("Failed to decode account info: unknown format"),
    ))
}

// ================================================================================================
// Balance Locks Query
// ================================================================================================

async fn query_balance_locks(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Result<Vec<DecodedBalanceLock>, BalanceQueryError> {
    // Check if Balances::Locks exists
    if client_at_block
        .storage()
        .entry(("Balances", "Locks"))
        .is_err()
    {
        return Ok(Vec::new());
    }

    // Build the storage address for Balances::Locks(account_id)
    let storage_addr = subxt::dynamic::storage::<_, ()>("Balances", "Locks");
    let account_bytes: [u8; 32] = *account.as_ref();

    let storage_value = client_at_block
        .storage()
        .fetch(storage_addr, (account_bytes,))
        .await;

    if let Ok(value) = storage_value {
        let raw_bytes = value.into_bytes();
        decode_balance_locks(&raw_bytes)
    } else {
        Ok(Vec::new())
    }
}

fn decode_balance_locks(raw_bytes: &[u8]) -> Result<Vec<DecodedBalanceLock>, BalanceQueryError> {
    // Decode as Vec<BalanceLock>
    if let Ok(locks) = Vec::<BalanceLock>::decode(&mut &raw_bytes[..]) {
        return Ok(locks
            .into_iter()
            .map(|lock| {
                let id = String::from_utf8_lossy(&lock.id)
                    .trim_end_matches('\0')
                    .to_string();
                DecodedBalanceLock {
                    id,
                    amount: lock.amount,
                    reasons: lock.reasons.as_str().to_string(),
                }
            })
            .collect());
    }

    // If decoding fails, return empty (locks may just not exist)
    Ok(Vec::new())
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
