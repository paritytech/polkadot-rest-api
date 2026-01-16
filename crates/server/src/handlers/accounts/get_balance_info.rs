use super::types::{
    BalanceInfoError, BalanceInfoQueryParams, BalanceInfoResponse, BalanceLock, BlockInfo,
    DecodedAccountData, DecodedBalanceLock,
};
use super::utils::validate_and_parse_address;
use crate::handlers::accounts::utils::{extract_u128_field, fetch_timestamp};
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use scale_value::{Composite, Value, ValueDef};
use serde_json::json;
use sp_core::crypto::AccountId32;

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/balance-info
///
/// Returns balance information for a given account.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `token` (optional): Token symbol for chains with multiple tokens (defaults to native)
/// - `denominated` (optional): When true, denominate balances using chain decimals
pub async fn get_balance_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<BalanceInfoQueryParams>,
) -> Result<Response, BalanceInfoError> {
    let account = validate_and_parse_address(&account_id)
        .map_err(|_| BalanceInfoError::InvalidAddress(account_id.clone()))?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    println!(
        "Fetching balance info for account {:?} at block {}",
        account, resolved_block.number
    );

    let response = query_balance_info(&state, &account, &resolved_block, &params).await?;

    Ok(Json(response).into_response())
}

async fn query_balance_info(
    state: &AppState,
    account: &AccountId32,
    block: &utils::ResolvedBlock,
    params: &BalanceInfoQueryParams,
) -> Result<BalanceInfoResponse, BalanceInfoError> {
    let client_at_block = state.client.at(block.number).await?;

    // Check if System and Balances pallets exist
    let system_account_exists = client_at_block
        .storage()
        .entry("System", "Account")
        .is_ok();

    if !system_account_exists {
        return Err(BalanceInfoError::BalancesPalletNotAvailable);
    }

    // Get token symbol (default based on chain type)
    let token_symbol = params.token.clone().unwrap_or_else(|| {
        get_default_token_symbol(&state.chain_info.spec_name)
    });

    // Get token decimals (default based on chain type)
    let token_decimals = get_default_token_decimals(&state.chain_info.spec_name);

    // Query System::Account for account info
    let account_data = query_account_data(state, block.number, account).await?;

    // Query Balances::Locks for balance locks
    let locks = query_balance_locks(state, block.number, account).await?;

    // Calculate transferable balance
    let transferable = calculate_transferable(state, block.number, &account_data).await;

    // Format the response
    let response = format_balance_response(
        block,
        &token_symbol,
        &account_data,
        &locks,
        &transferable,
        params.denominated,
        token_decimals,
    );

    Ok(response)
}

// ================================================================================================
// Token/Decimals Helpers
// ================================================================================================

fn get_default_token_symbol(spec_name: &str) -> String {
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

fn get_default_token_decimals(spec_name: &str) -> u8 {
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

fn get_default_existential_deposit(spec_name: &str) -> u128 {
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
    state: &AppState,
    block_number: u64,
    account: &AccountId32,
) -> Result<DecodedAccountData, BalanceInfoError> {
    let client_at_block = state.client.at(block_number).await?;
    let storage_entry = client_at_block.storage().entry("System", "Account")?;

    // System::Account takes a single AccountId key
    // Pass the account bytes as a fixed-size array reference
    let account_bytes: [u8; 32] = *account.as_ref();
    let storage_value = storage_entry.fetch(&(&account_bytes,)).await?;

    if let Some(value) = storage_value {
        decode_account_info(&value).await
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

async fn decode_account_info(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<DecodedAccountData, BalanceInfoError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        BalanceInfoError::DecodeFailed(parity_scale_codec::Error::from(
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
                        // Sometimes nonce is wrapped
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

// ================================================================================================
// Balance Locks Query
// ================================================================================================

async fn query_balance_locks(
    state: &AppState,
    block_number: u64,
    account: &AccountId32,
) -> Result<Vec<DecodedBalanceLock>, BalanceInfoError> {
    let client_at_block = state.client.at(block_number).await?;

    // Check if Balances::Locks exists
    let locks_exists = client_at_block
        .storage()
        .entry("Balances", "Locks")
        .is_ok();

    if !locks_exists {
        return Ok(Vec::new());
    }

    let storage_entry = client_at_block.storage().entry("Balances", "Locks")?;
    // Balances::Locks takes a single AccountId key
    let account_bytes: [u8; 32] = *account.as_ref();
    let storage_value = storage_entry.fetch(&(&account_bytes,)).await?;

    if let Some(value) = storage_value {
        decode_balance_locks(&value).await
    } else {
        Ok(Vec::new())
    }
}

async fn decode_balance_locks(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<Vec<DecodedBalanceLock>, BalanceInfoError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        BalanceInfoError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode balance locks",
        ))
    })?;

    let mut locks = Vec::new();

    // Locks is a Vec<BalanceLock>
    if let ValueDef::Composite(Composite::Unnamed(items)) = &decoded.value {
        for item in items {
            if let ValueDef::Composite(Composite::Named(fields)) = &item.value {
                // Extract id (it's a [u8; 8] encoded as bytes)
                let id = fields
                    .iter()
                    .find(|(name, _)| name == "id")
                    .map(|(_, v)| extract_lock_id(v))
                    .unwrap_or_else(|| "unknown".to_string());

                let amount = extract_u128_field(fields, "amount").unwrap_or(0);

                // Extract reasons - it's an enum
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
            // Try to convert bytes to string
            let byte_vec: Vec<u8> = bytes
                .iter()
                .filter_map(|v| match &v.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(b)) => Some(*b as u8),
                    _ => None,
                })
                .collect();

            // Convert to string, trimming null bytes
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

async fn calculate_transferable(
    state: &AppState,
    _block_number: u64,
    account_data: &DecodedAccountData,
) -> String {
    // For newer runtimes with frozen field:
    // transferable = free - max(maybeED, frozen - reserved)
    // where maybeED = 0 if frozen == 0 && reserved == 0, else existential_deposit

    if let Some(frozen) = account_data.frozen {
        // Get existential deposit from chain defaults
        let ed = get_default_existential_deposit(&state.chain_info.spec_name);

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

fn format_balance_response(
    block: &utils::ResolvedBlock,
    token_symbol: &str,
    account_data: &DecodedAccountData,
    locks: &[DecodedBalanceLock],
    transferable: &str,
    denominated: bool,
    decimals: u8,
) -> BalanceInfoResponse {
    let format_balance = |amount: u128| -> String {
        if denominated && decimals > 0 {
            apply_denomination(amount, decimals as usize)
        } else {
            amount.to_string()
        }
    };

    // Format frozen fields based on runtime version
    let (misc_frozen, fee_frozen, frozen) = if account_data.frozen.is_some() {
        // Newer runtime: has frozen, no miscFrozen/feeFrozen
        (
            "miscFrozen does not exist for this runtime".to_string(),
            "feeFrozen does not exist for this runtime".to_string(),
            format_balance(account_data.frozen.unwrap_or(0)),
        )
    } else {
        // Older runtime: has miscFrozen/feeFrozen, no frozen
        (
            format_balance(account_data.misc_frozen.unwrap_or(0)),
            format_balance(account_data.fee_frozen.unwrap_or(0)),
            "frozen does not exist for this runtime".to_string(),
        )
    };

    // Format transferable
    let formatted_transferable = if transferable.starts_with("transferable") {
        transferable.to_string()
    } else if denominated && decimals > 0 {
        if let Ok(amount) = transferable.parse::<u128>() {
            apply_denomination(amount, decimals as usize)
        } else {
            transferable.to_string()
        }
    } else {
        transferable.to_string()
    };

    // Format locks
    let formatted_locks: Vec<BalanceLock> = locks
        .iter()
        .map(|lock| BalanceLock {
            id: lock.id.clone(),
            amount: if denominated && decimals > 0 {
                apply_denomination(lock.amount, decimals as usize)
            } else {
                lock.amount.to_string()
            },
            reasons: lock.reasons.clone(),
        })
        .collect();

    BalanceInfoResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        nonce: account_data.nonce.to_string(),
        token_symbol: token_symbol.to_string(),
        free: format_balance(account_data.free),
        reserved: format_balance(account_data.reserved),
        misc_frozen,
        fee_frozen,
        frozen,
        transferable: formatted_transferable,
        locks: formatted_locks,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    }
}

fn apply_denomination(amount: u128, decimals: usize) -> String {
    let str_balance = amount.to_string();

    if str_balance == "0" || decimals == 0 {
        return str_balance;
    }

    if str_balance.len() <= decimals {
        // Pad with leading zeros
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

// ================================================================================================
// Relay Chain Block Handling
// ================================================================================================

async fn handle_use_rc_block(
    state: AppState,
    account: AccountId32,
    params: BalanceInfoQueryParams,
) -> Result<Response, BalanceInfoError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(BalanceInfoError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(BalanceInfoError::RelayChainNotConfigured);
    }

    // Resolve RC block
    let rc_block_id = params
        .at
        .clone()
        .unwrap_or_else(|| "head".to_string())
        .parse::<utils::BlockId>()?;
    let rc_resolved = utils::resolve_block_with_rpc(
        state.get_relay_chain_rpc_client().unwrap(),
        state.get_relay_chain_rpc().unwrap(),
        Some(rc_block_id),
    )
    .await?;

    // Find AH blocks
    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved).await?;

    if ah_blocks.is_empty() {
        return Ok(Json(json!([])).into_response());
    }

    let rc_block_hash = rc_resolved.hash.clone();
    let rc_block_number = rc_resolved.number.to_string();

    // Process each AH block
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };

        let mut response = query_balance_info(&state, &account, &ah_resolved, &params).await?;

        // Add RC block info
        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        // Fetch AH timestamp
        if let Ok(timestamp) = fetch_timestamp(&state, ah_block.number).await {
            response.ah_timestamp = Some(timestamp);
        }

        results.push(response);
    }

    Ok(Json(results).into_response())
}
