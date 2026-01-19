use super::types::{
    BlockInfo, EraPayouts, EraPayoutsData, StakingPayoutsError, StakingPayoutsQueryParams,
    StakingPayoutsResponse, ValidatorPayout,
};
use super::utils::validate_and_parse_address;
use crate::handlers::accounts::utils::fetch_timestamp;
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use config::ChainType;
use scale_value::{Composite, Value, ValueDef};
use serde_json::json;
use sp_core::crypto::{AccountId32, Ss58Codec};
use std::collections::HashMap;

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/staking-payouts
///
/// Returns staking payout information for a given account address.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `depth` (optional): Number of eras to query (default: 1)
/// - `era` (optional): The era to query at (default: active_era - 1)
/// - `unclaimedOnly` (optional): Only show unclaimed rewards (default: true)
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
pub async fn get_staking_payouts(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<StakingPayoutsQueryParams>,
) -> Result<Response, StakingPayoutsError> {
    let account = validate_and_parse_address(&account_id)
        .map_err(|_| StakingPayoutsError::InvalidAddress(account_id.clone()))?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let block_id = params
        .at
        .clone()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    println!(
        "Fetching staking payouts for account {:?} at block {}",
        account, resolved_block.number
    );

    let response = query_staking_payouts(&state, &account, &resolved_block, &params).await?;

    Ok(Json(response).into_response())
}

async fn query_staking_payouts(
    state: &AppState,
    account: &AccountId32,
    block: &utils::ResolvedBlock,
    params: &StakingPayoutsQueryParams,
) -> Result<StakingPayoutsResponse, StakingPayoutsError> {
    let client_at_block = state.client.at(block.number).await?;

    // Check if Staking pallet exists
    let staking_exists = client_at_block
        .storage()
        .entry("Staking", "ActiveEra")
        .is_ok();

    if !staking_exists {
        return Err(StakingPayoutsError::StakingPalletNotAvailable);
    }

    // Get active era - it's a value storage, use fetch with unit key
    let active_era_entry = client_at_block.storage().entry("Staking", "ActiveEra")?;
    let active_era_value = active_era_entry.fetch(&()).await?;
    let active_era = if let Some(value) = active_era_value {
        decode_active_era(&value)?
    } else {
        return Err(StakingPayoutsError::NoActiveEra);
    };

    // Get history depth (default to 84 if not found)
    let history_depth = if let Ok(history_entry) =
        client_at_block.storage().entry("Staking", "HistoryDepth")
    {
        if let Ok(Some(value)) = history_entry.fetch(&()).await {
            decode_u32(&value).unwrap_or(84)
        } else {
            84
        }
    } else {
        84
    };

    // Validate depth parameter
    if params.depth == 0 || params.depth > history_depth {
        return Err(StakingPayoutsError::InvalidDepth);
    }

    // Determine target era (default to active_era - 1, which is the last completed era)
    let target_era = params.era.unwrap_or_else(|| active_era.saturating_sub(1));

    // Validate era parameter
    if target_era >= active_era {
        return Err(StakingPayoutsError::InvalidEra(target_era));
    }

    let min_era = active_era.saturating_sub(history_depth);
    if target_era < min_era {
        return Err(StakingPayoutsError::InvalidEra(target_era));
    }

    // Calculate start era based on depth
    let start_era = target_era.saturating_sub(params.depth - 1).max(min_era);

    // Process each era
    let mut eras_payouts = Vec::new();
    for era in start_era..=target_era {
        let era_payout =
            process_era(state, block.number, account, era, params.unclaimed_only).await;
        eras_payouts.push(era_payout);
    }

    Ok(StakingPayoutsResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        eras_payouts,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ================================================================================================
// Era Processing
// ================================================================================================

async fn process_era(
    state: &AppState,
    block_number: u64,
    account: &AccountId32,
    era: u32,
    unclaimed_only: bool,
) -> EraPayouts {
    // Try to get era data
    match fetch_era_data(state, block_number, account, era, unclaimed_only).await {
        Ok(data) => data,
        Err(e) => EraPayouts::Message {
            message: format!("Era {}: {}", era, e),
        },
    }
}

async fn fetch_era_data(
    state: &AppState,
    block_number: u64,
    account: &AccountId32,
    era: u32,
    unclaimed_only: bool,
) -> Result<EraPayouts, String> {
    let client_at_block = state
        .client
        .at(block_number)
        .await
        .map_err(|e| e.to_string())?;

    let account_bytes: [u8; 32] = *account.as_ref();

    // Get total era reward points
    let reward_points_entry = client_at_block
        .storage()
        .entry("Staking", "ErasRewardPoints")
        .map_err(|e| e.to_string())?;
    let reward_points_value = reward_points_entry
        .fetch(&(era,))
        .await
        .map_err(|e| e.to_string())?;

    let (total_era_reward_points, individual_points) = if let Some(value) = reward_points_value {
        decode_era_reward_points(&value).map_err(|e| e.to_string())?
    } else {
        return Ok(EraPayouts::Message {
            message: format!("No reward points found for era {}", era),
        });
    };

    // Get total era payout
    let validator_reward_entry = client_at_block
        .storage()
        .entry("Staking", "ErasValidatorReward")
        .map_err(|e| e.to_string())?;
    let validator_reward_value = validator_reward_entry
        .fetch(&(era,))
        .await
        .map_err(|e| e.to_string())?;

    let total_era_payout = if let Some(value) = validator_reward_value {
        decode_u128_storage(&value).map_err(|e| e.to_string())?
    } else {
        return Ok(EraPayouts::Message {
            message: format!("No validator reward found for era {}", era),
        });
    };

    // Get exposure data - try ErasStakersClipped first
    let exposure_data =
        fetch_exposure_data(state, block_number, account, era, &account_bytes).await?;

    if exposure_data.is_empty() {
        return Ok(EraPayouts::Message {
            message: format!("Account has no nominations in era {}", era),
        });
    }

    // Calculate payouts for each validator the account nominates
    let mut payouts = Vec::new();
    for (validator_id, nominator_exposure, total_exposure) in exposure_data {
        // Get validator's reward points
        let validator_bytes = AccountId32::from_ss58check(&validator_id)
            .map_err(|_| format!("Invalid validator address: {}", validator_id))?;
        let validator_bytes_arr: [u8; 32] = *validator_bytes.as_ref();

        let validator_points = individual_points
            .get(&validator_bytes_arr)
            .copied()
            .unwrap_or(0);

        if validator_points == 0 {
            continue; // Skip validators with no points
        }

        // Get validator commission
        let commission =
            fetch_validator_commission(state, block_number, era, &validator_bytes_arr)
                .await
                .unwrap_or(0);

        // Check if claimed
        let claimed = check_if_claimed(state, block_number, &validator_bytes_arr, era).await;

        // Skip if unclaimed_only is true and this is already claimed
        if unclaimed_only && claimed {
            continue;
        }

        // Calculate payout
        let account_bytes_ref: &[u8; 32] = account.as_ref();
        let is_validator = account_bytes_ref == &validator_bytes_arr;
        let nominator_payout = calculate_payout(
            total_era_reward_points,
            total_era_payout,
            validator_points,
            commission,
            nominator_exposure,
            total_exposure,
            is_validator,
        );

        payouts.push(ValidatorPayout {
            validator_id,
            nominator_staking_payout: nominator_payout.to_string(),
            claimed,
            total_validator_reward_points: validator_points.to_string(),
            validator_commission: commission.to_string(),
            total_validator_exposure: total_exposure.to_string(),
            nominator_exposure: nominator_exposure.to_string(),
        });
    }

    Ok(EraPayouts::Payouts(EraPayoutsData {
        era,
        total_era_reward_points: total_era_reward_points.to_string(),
        total_era_payout: total_era_payout.to_string(),
        payouts,
    }))
}

// ================================================================================================
// Storage Fetching Functions
// ================================================================================================

/// Fetch exposure data for an account in an era
/// Returns Vec<(validator_id, nominator_exposure, total_exposure)>
async fn fetch_exposure_data(
    state: &AppState,
    block_number: u64,
    account: &AccountId32,
    era: u32,
    account_bytes: &[u8; 32],
) -> Result<Vec<(String, u128, u128)>, String> {
    let client_at_block = state
        .client
        .at(block_number)
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();

    // Try ErasStakersClipped first (standard storage)
    if let Ok(stakers_entry) = client_at_block.storage().entry("Staking", "ErasStakersClipped") {
        // Try to get nominators entry to find which validators this account nominates
        if let Ok(nominators_entry) = client_at_block.storage().entry("Staking", "Nominators") {
            if let Ok(Some(nom_value)) = nominators_entry.fetch(&(account_bytes,)).await {
                let targets = decode_nomination_targets(&nom_value);

                for validator_bytes in targets {
                    if let Ok(Some(exposure_value)) =
                        stakers_entry.fetch(&(era, &validator_bytes)).await
                    {
                        if let Ok((total, own, others)) = decode_exposure(&exposure_value) {
                            // Check if account is in the others list
                            for (nominator, value) in &others {
                                if nominator == account_bytes {
                                    let validator_id =
                                        AccountId32::from(validator_bytes).to_ss58check();
                                    results.push((validator_id, *value, total));
                                    break;
                                }
                            }
                            // Check if account is the validator itself
                            if account_bytes == &validator_bytes {
                                let validator_id =
                                    AccountId32::from(validator_bytes).to_ss58check();
                                results.push((validator_id, own, total));
                            }
                        }
                    }
                }
            }
        }
    }

    // If account is a validator, also check if they have self-stake
    if let Ok(bonded_entry) = client_at_block.storage().entry("Staking", "Bonded") {
        if let Ok(Some(_)) = bonded_entry.fetch(&(account_bytes,)).await {
            // Account is a stash, check if they're also validating
            if let Ok(stakers_entry) = client_at_block.storage().entry("Staking", "ErasStakersClipped")
            {
                if let Ok(Some(exposure_value)) = stakers_entry.fetch(&(era, account_bytes)).await {
                    if let Ok((total, own, _)) = decode_exposure(&exposure_value) {
                        let validator_id = account.to_ss58check();
                        // Only add if not already in results
                        if !results.iter().any(|(v, _, _)| v == &validator_id) {
                            results.push((validator_id, own, total));
                        }
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Fetch validator commission for a specific era
async fn fetch_validator_commission(
    state: &AppState,
    block_number: u64,
    era: u32,
    validator_bytes: &[u8; 32],
) -> Option<u32> {
    let client_at_block = state.client.at(block_number).await.ok()?;
    let prefs_entry = client_at_block
        .storage()
        .entry("Staking", "ErasValidatorPrefs")
        .ok()?;
    let prefs_value = prefs_entry.fetch(&(era, validator_bytes)).await.ok()??;
    decode_validator_commission(&prefs_value).ok()
}

/// Check if rewards have been claimed for a validator in an era
async fn check_if_claimed(
    state: &AppState,
    block_number: u64,
    validator_bytes: &[u8; 32],
    era: u32,
) -> bool {
    let client_at_block = match state.client.at(block_number).await {
        Ok(c) => c,
        Err(_) => return false,
    };

    // First get the controller for this validator
    let controller_bytes =
        if let Ok(bonded_entry) = client_at_block.storage().entry("Staking", "Bonded") {
            if let Ok(Some(value)) = bonded_entry.fetch(&(validator_bytes,)).await {
                decode_account_bytes(&value).unwrap_or(*validator_bytes)
            } else {
                *validator_bytes
            }
        } else {
            *validator_bytes
        };

    // Check ledger for claimed rewards
    if let Ok(ledger_entry) = client_at_block.storage().entry("Staking", "Ledger") {
        if let Ok(Some(value)) = ledger_entry.fetch(&(&controller_bytes,)).await {
            return check_claimed_in_ledger(&value, era);
        }
    }

    false
}

// ================================================================================================
// Payout Calculation
// ================================================================================================

/// Calculate the payout for a nominator
///
/// Formula:
/// 1. validator_era_payout = (validator_points / total_era_points) * total_era_payout
/// 2. commission_payout = validator_era_payout * (commission / 1_000_000_000)
/// 3. stakers_payout = validator_era_payout - commission_payout
/// 4. nominator_payout = stakers_payout * (nominator_exposure / total_exposure)
/// 5. If is_validator: add commission_payout to their share
fn calculate_payout(
    total_era_reward_points: u32,
    total_era_payout: u128,
    validator_points: u32,
    commission: u32, // Parts per billion (0 - 1_000_000_000)
    nominator_exposure: u128,
    total_exposure: u128,
    is_validator: bool,
) -> u128 {
    if total_era_reward_points == 0 || total_exposure == 0 {
        return 0;
    }

    // Calculate validator's share of era payout
    let validator_era_payout = (total_era_payout)
        .saturating_mul(validator_points as u128)
        / (total_era_reward_points as u128);

    // Calculate commission (commission is in parts per billion)
    let commission_payout =
        validator_era_payout.saturating_mul(commission as u128) / 1_000_000_000;

    // Remaining goes to stakers proportionally
    let stakers_payout = validator_era_payout.saturating_sub(commission_payout);

    // Calculate nominator's proportional share
    let nominator_payout = stakers_payout.saturating_mul(nominator_exposure) / total_exposure;

    if is_validator {
        // Validator gets their commission plus their proportional share
        nominator_payout.saturating_add(commission_payout)
    } else {
        nominator_payout
    }
}

// ================================================================================================
// Decoding Functions
// ================================================================================================

fn decode_active_era(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<u32, StakingPayoutsError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingPayoutsError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode active era",
        ))
    })?;

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            for (name, val) in fields {
                if name == "index" {
                    if let ValueDef::Primitive(scale_value::Primitive::U128(v)) = &val.value {
                        return Ok(*v as u32);
                    }
                }
            }
            Err(StakingPayoutsError::DecodeFailed(
                parity_scale_codec::Error::from("Active era index not found"),
            ))
        }
        _ => Err(StakingPayoutsError::DecodeFailed(
            parity_scale_codec::Error::from("Invalid active era format"),
        )),
    }
}

fn decode_u32(value: &subxt_historic::storage::StorageValue<'_>) -> Option<u32> {
    let decoded: Value<()> = value.decode_as().ok()?;
    match &decoded.value {
        ValueDef::Primitive(scale_value::Primitive::U128(v)) => Some(*v as u32),
        _ => None,
    }
}

fn decode_u128_storage(value: &subxt_historic::storage::StorageValue<'_>) -> Result<u128, String> {
    let decoded: Value<()> = value.decode_as().map_err(|e| e.to_string())?;
    match &decoded.value {
        ValueDef::Primitive(scale_value::Primitive::U128(v)) => Ok(*v),
        _ => Err("Invalid u128 format".to_string()),
    }
}

fn decode_era_reward_points(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<(u32, HashMap<[u8; 32], u32>), String> {
    let decoded: Value<()> = value.decode_as().map_err(|e| e.to_string())?;

    let mut total = 0u32;
    let mut individual = HashMap::new();

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            for (name, val) in fields {
                if name == "total" {
                    if let ValueDef::Primitive(scale_value::Primitive::U128(v)) = &val.value {
                        total = *v as u32;
                    }
                } else if name == "individual" {
                    if let ValueDef::Composite(Composite::Unnamed(items)) = &val.value {
                        for item in items {
                            if let ValueDef::Composite(Composite::Unnamed(pair)) = &item.value {
                                if pair.len() == 2 {
                                    if let Some(account) = extract_account_bytes_from_value(&pair[0])
                                    {
                                        if let ValueDef::Primitive(scale_value::Primitive::U128(
                                            points,
                                        )) = &pair[1].value
                                        {
                                            individual.insert(account, *points as u32);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => return Err("Invalid era reward points format".to_string()),
    }

    Ok((total, individual))
}

fn decode_exposure(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<(u128, u128, Vec<([u8; 32], u128)>), String> {
    let decoded: Value<()> = value.decode_as().map_err(|e| e.to_string())?;

    let mut total = 0u128;
    let mut own = 0u128;
    let mut others = Vec::new();

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            for (name, val) in fields {
                if name == "total" {
                    if let ValueDef::Primitive(scale_value::Primitive::U128(v)) = &val.value {
                        total = *v;
                    }
                } else if name == "own" {
                    if let ValueDef::Primitive(scale_value::Primitive::U128(v)) = &val.value {
                        own = *v;
                    }
                } else if name == "others" {
                    if let ValueDef::Composite(Composite::Unnamed(items)) = &val.value {
                        for item in items {
                            if let ValueDef::Composite(Composite::Named(other_fields)) = &item.value
                            {
                                let mut who: Option<[u8; 32]> = None;
                                let mut value_amount = 0u128;
                                for (field_name, field_val) in other_fields {
                                    if field_name == "who" {
                                        who = extract_account_bytes_from_value(field_val);
                                    } else if field_name == "value" {
                                        if let ValueDef::Primitive(scale_value::Primitive::U128(v)) =
                                            &field_val.value
                                        {
                                            value_amount = *v;
                                        }
                                    }
                                }
                                if let Some(account) = who {
                                    others.push((account, value_amount));
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => return Err("Invalid exposure format".to_string()),
    }

    Ok((total, own, others))
}

fn decode_nomination_targets(value: &subxt_historic::storage::StorageValue<'_>) -> Vec<[u8; 32]> {
    let mut targets = Vec::new();

    if let Ok(decoded) = value.decode_as::<Value<()>>() {
        if let ValueDef::Composite(Composite::Named(fields)) = &decoded.value {
            for (name, val) in fields {
                if name == "targets" {
                    if let ValueDef::Composite(Composite::Unnamed(items)) = &val.value {
                        for item in items {
                            if let Some(account) = extract_account_bytes_from_value(item) {
                                targets.push(account);
                            }
                        }
                    }
                }
            }
        }
    }

    targets
}

fn decode_validator_commission(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<u32, String> {
    let decoded: Value<()> = value.decode_as().map_err(|e| e.to_string())?;

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            for (name, val) in fields {
                if name == "commission" {
                    if let ValueDef::Primitive(scale_value::Primitive::U128(v)) = &val.value {
                        return Ok(*v as u32);
                    }
                }
            }
            Ok(0)
        }
        _ => Ok(0),
    }
}

fn decode_account_bytes(value: &subxt_historic::storage::StorageValue<'_>) -> Option<[u8; 32]> {
    let decoded: Value<()> = value.decode_as().ok()?;
    extract_account_bytes_from_value(&decoded)
}

fn extract_account_bytes_from_value(value: &Value<()>) -> Option<[u8; 32]> {
    match &value.value {
        ValueDef::Composite(Composite::Unnamed(bytes)) => {
            let byte_vec: Vec<u8> = bytes
                .iter()
                .filter_map(|v| match &v.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(b)) => Some(*b as u8),
                    _ => None,
                })
                .collect();

            if byte_vec.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&byte_vec);
                Some(arr)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn check_claimed_in_ledger(value: &subxt_historic::storage::StorageValue<'_>, era: u32) -> bool {
    if let Ok(decoded) = value.decode_as::<Value<()>>() {
        if let ValueDef::Composite(Composite::Named(fields)) = &decoded.value {
            // Check claimedRewards field (newer format)
            for (name, val) in fields {
                if name == "claimedRewards"
                    || name == "claimed_rewards"
                    || name == "legacyClaimedRewards"
                {
                    if let ValueDef::Composite(Composite::Unnamed(eras)) = &val.value {
                        for era_val in eras {
                            if let ValueDef::Primitive(scale_value::Primitive::U128(e)) =
                                &era_val.value
                            {
                                if *e as u32 == era {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

// ================================================================================================
// Relay Chain Block Handling
// ================================================================================================

async fn handle_use_rc_block(
    state: AppState,
    account: AccountId32,
    params: StakingPayoutsQueryParams,
) -> Result<Response, StakingPayoutsError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(StakingPayoutsError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(StakingPayoutsError::RelayChainNotConfigured);
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

        let mut response = query_staking_payouts(&state, &account, &ah_resolved, &params).await?;

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
