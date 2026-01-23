//! Shared utilities for balance info queries.
//!
//! This module provides common functionality for querying account balance information
//! that is shared between the regular accounts endpoint and the RC (relay chain) endpoint.

use crate::utils::ResolvedBlock;
use scale_value::{Composite, Value, ValueDef};
use serde::Serialize;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

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
    ClientAtBlockFailed(#[from] subxt::error::OnlineClientAtBlockError),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(#[from] subxt::error::StorageError),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),
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
    let storage_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("System", "Account");

    // Check if System and Balances pallets exist
    let system_account_exists = client_at_block.storage().entry(storage_query).is_ok();

    if !system_account_exists {
        return Err(BalanceQueryError::BalancesPalletNotAvailable);
    }

    // Get token symbol (default based on chain type)
    let token_symbol = token.unwrap_or_else(|| get_default_token_symbol(spec_name));

    // Get token decimals
    let token_decimals = get_default_token_decimals(spec_name);

    // Query System::Account for account info
    let account_data = query_account_data(client_at_block, account).await?;

    // Query Balances::Locks for balance locks
    let locks = query_balance_locks(client_at_block, account).await?;

    // Calculate transferable balance
    let transferable = calculate_transferable(spec_name, &account_data);

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
    if spec_lower.contains("polkadot") || spec_lower.contains("statemint") {
        "DOT".to_string()
    } else if spec_lower.contains("kusama") || spec_lower.contains("statemine") {
        "KSM".to_string()
    } else if spec_lower.contains("westend") || spec_lower.contains("westmint") {
        "WND".to_string()
    } else if spec_lower.contains("rococo") {
        "ROC".to_string()
    } else if spec_lower.contains("paseo") {
        "PAS".to_string()
    } else {
        "UNIT".to_string()
    }
}

/// Get the default token decimals for a given spec name
pub fn get_default_token_decimals(spec_name: &str) -> u8 {
    let spec_lower = spec_name.to_lowercase();
    if spec_lower.contains("polkadot") || spec_lower.contains("statemint") {
        10 // DOT has 10 decimals
    } else if spec_lower.contains("kusama") || spec_lower.contains("statemine") {
        12 // KSM has 12 decimals
    } else if spec_lower.contains("westend") || spec_lower.contains("westmint") {
        12 // WND has 12 decimals
    } else {
        12 // Default to 12 decimals
    }
}

/// Get the default existential deposit for a given spec name
pub fn get_default_existential_deposit(spec_name: &str) -> u128 {
    let spec_lower = spec_name.to_lowercase();
    if spec_lower.contains("polkadot") || spec_lower.contains("statemint") {
        10_000_000_000 // 1 DOT = 10^10 planck, ED = 1 DOT on Polkadot Asset Hub
    } else if spec_lower.contains("kusama") || spec_lower.contains("statemine") {
        33_333_333 // ~0.000033 KSM on Kusama Asset Hub
    } else if spec_lower.contains("westend") || spec_lower.contains("westmint") {
        1_000_000_000_000 // 1 WND on Westend
    } else {
        1_000_000_000_000 // Default
    }
}

// ================================================================================================
// Account Data Query
// ================================================================================================

async fn query_account_data(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Result<DecodedAccountData, BalanceQueryError> {
    let storage_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("System", "Account");
    let storage_entry = client_at_block.storage().entry(storage_query)?;

    let account_bytes: [u8; 32] = *account.as_ref();
    let key = vec![Value::from_bytes(account_bytes)];
    let storage_value = storage_entry.try_fetch(key).await?;

    if let Some(value) = storage_value {
        decode_account_info(&value)
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

fn decode_account_info(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<DecodedAccountData, BalanceQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        BalanceQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode account info",
        ))
    })?;

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            // Extract nonce
            let nonce = fields
                .iter()
                .find(|(name, _)| name == "nonce")
                .and_then(|(_, value)| match &value.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val as u32),
                    ValueDef::Composite(Composite::Unnamed(vals)) => {
                        vals.first().and_then(|v| match &v.value {
                            ValueDef::Primitive(scale_value::Primitive::U128(val)) => {
                                Some(*val as u32)
                            }
                            _ => None,
                        })
                    }
                    _ => None,
                })
                .unwrap_or(0);

            // Extract data field which contains balance info
            let data_value = fields
                .iter()
                .find(|(name, _)| name == "data")
                .map(|(_, v)| v);

            if let Some(data) = data_value {
                match &data.value {
                    ValueDef::Composite(Composite::Named(data_fields)) => {
                        let free = extract_u128_field(data_fields, "free").unwrap_or(0);
                        let reserved = extract_u128_field(data_fields, "reserved").unwrap_or(0);

                        // Check for new format (frozen) vs old format (miscFrozen, feeFrozen)
                        let frozen = extract_u128_field(data_fields, "frozen");
                        let misc_frozen = extract_u128_field(data_fields, "miscFrozen")
                            .or_else(|| extract_u128_field(data_fields, "misc_frozen"));
                        let fee_frozen = extract_u128_field(data_fields, "feeFrozen")
                            .or_else(|| extract_u128_field(data_fields, "fee_frozen"));

                        Ok(DecodedAccountData {
                            nonce,
                            free,
                            reserved,
                            misc_frozen,
                            fee_frozen,
                            frozen,
                        })
                    }
                    _ => Ok(DecodedAccountData {
                        nonce,
                        free: 0,
                        reserved: 0,
                        misc_frozen: None,
                        fee_frozen: None,
                        frozen: Some(0),
                    }),
                }
            } else {
                Ok(DecodedAccountData {
                    nonce,
                    free: 0,
                    reserved: 0,
                    misc_frozen: None,
                    fee_frozen: None,
                    frozen: Some(0),
                })
            }
        }
        _ => Ok(DecodedAccountData {
            nonce: 0,
            free: 0,
            reserved: 0,
            misc_frozen: None,
            fee_frozen: None,
            frozen: Some(0),
        }),
    }
}

fn extract_u128_field(fields: &[(String, Value<()>)], name: &str) -> Option<u128> {
    fields.iter().find(|(n, _)| n == name).and_then(|(_, v)| {
        match &v.value {
            ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val),
            // Sometimes values are wrapped in a Composite
            ValueDef::Composite(Composite::Unnamed(vals)) => {
                vals.first().and_then(|inner| match &inner.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val),
                    _ => None,
                })
            }
            _ => None,
        }
    })
}

// ================================================================================================
// Balance Locks Query
// ================================================================================================

async fn query_balance_locks(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Result<Vec<DecodedBalanceLock>, BalanceQueryError> {
    let storage_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Balances", "Locks");

    // Check if Balances::Locks exists
    let locks_exists = client_at_block
        .storage()
        .entry(storage_query.clone())
        .is_ok();

    if !locks_exists {
        return Ok(Vec::new());
    }

    let storage_entry = client_at_block.storage().entry(storage_query)?;
    let account_bytes: [u8; 32] = *account.as_ref();
    let key = vec![Value::from_bytes(account_bytes)];
    let storage_value = storage_entry.try_fetch(key).await?;

    if let Some(value) = storage_value {
        decode_balance_locks(&value)
    } else {
        Ok(Vec::new())
    }
}

fn decode_balance_locks(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<Vec<DecodedBalanceLock>, BalanceQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        BalanceQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode balance locks",
        ))
    })?;

    let mut locks = Vec::new();

    if let ValueDef::Composite(Composite::Unnamed(items)) = &decoded.value {
        for item in items {
            if let ValueDef::Composite(Composite::Named(fields)) = &item.value {
                let id = fields
                    .iter()
                    .find(|(name, _)| name == "id")
                    .map(|(_, v)| extract_lock_id(v))
                    .unwrap_or_else(|| "unknown".to_string());

                let amount = extract_u128_field(fields, "amount").unwrap_or(0);

                let reasons = fields
                    .iter()
                    .find(|(name, _)| name == "reasons")
                    .map(|(_, v)| extract_lock_reasons(v))
                    .unwrap_or_else(|| "All".to_string());

                locks.push(DecodedBalanceLock {
                    id,
                    amount,
                    reasons,
                });
            }
        }
    }

    Ok(locks)
}

fn extract_lock_id(value: &Value<()>) -> String {
    match &value.value {
        ValueDef::Composite(Composite::Unnamed(bytes)) => {
            let byte_vec: Vec<u8> = bytes
                .iter()
                .filter_map(|v| match &v.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(b)) => Some(*b as u8),
                    _ => None,
                })
                .collect();

            String::from_utf8_lossy(&byte_vec)
                .trim_end_matches('\0')
                .to_string()
        }
        _ => "unknown".to_string(),
    }
}

fn extract_lock_reasons(value: &Value<()>) -> String {
    match &value.value {
        ValueDef::Variant(variant) => variant.name.clone(),
        _ => "All".to_string(),
    }
}

// ================================================================================================
// Transferable Calculation
// ================================================================================================

/// Calculate the transferable balance based on the account data
pub fn calculate_transferable(spec_name: &str, account_data: &DecodedAccountData) -> String {
    // For newer runtimes with frozen field:
    // transferable = free - max(maybeED, frozen - reserved)
    // where maybeED = 0 if frozen == 0 && reserved == 0, else existential_deposit

    if let Some(frozen) = account_data.frozen {
        let ed = get_default_existential_deposit(spec_name);

        let no_frozen_reserved = frozen == 0 && account_data.reserved == 0;
        let maybe_ed = if no_frozen_reserved { 0 } else { ed };

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
