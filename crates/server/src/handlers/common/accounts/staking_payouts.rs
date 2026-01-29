//! Common staking payouts utilities shared across handler modules.

use crate::utils::ResolvedBlock;
use scale_value::{Composite, Value, ValueDef};
use sp_core::crypto::{AccountId32, Ss58Codec};
use std::collections::HashMap;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum StakingPayoutsQueryError {
    #[error("The runtime does not include the staking pallet at this block")]
    StakingPalletNotAvailable,

    #[error("No active era found")]
    NoActiveEra,

    #[error("Invalid era: requested era {0} is beyond history depth")]
    InvalidEra(u32),

    #[error("Depth must be greater than 0 and less than history depth")]
    InvalidDepth,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(Box<subxt::error::OnlineClientAtBlockError>),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(Box<subxt::error::StorageError>),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),
}

impl From<subxt::error::OnlineClientAtBlockError> for StakingPayoutsQueryError {
    fn from(err: subxt::error::OnlineClientAtBlockError) -> Self {
        StakingPayoutsQueryError::ClientAtBlockFailed(Box::new(err))
    }
}

impl From<subxt::error::StorageError> for StakingPayoutsQueryError {
    fn from(err: subxt::error::StorageError) -> Self {
        StakingPayoutsQueryError::StorageQueryFailed(Box::new(err))
    }
}

// ================================================================================================
// Data Types
// ================================================================================================

/// Decoded exposure data: (total, own, others as Vec<(account_bytes, value)>)
pub type ExposureData = (u128, u128, Vec<([u8; 32], u128)>);

/// Query parameters for staking payouts
#[derive(Debug, Clone)]
pub struct StakingPayoutsParams {
    /// Number of eras to query. Must be less than HISTORY_DEPTH. Defaults to 1.
    pub depth: u32,
    /// The era to query at. Defaults to active_era - 1.
    pub era: Option<u32>,
    /// Only show unclaimed rewards. Defaults to true.
    pub unclaimed_only: bool,
}

impl Default for StakingPayoutsParams {
    fn default() -> Self {
        Self {
            depth: 1,
            era: None,
            unclaimed_only: true,
        }
    }
}

/// Raw staking payouts data returned from query
#[derive(Debug)]
pub struct RawStakingPayouts {
    /// Block information
    pub block: FormattedBlockInfo,
    /// Era payouts data
    pub eras_payouts: Vec<RawEraPayouts>,
}

/// Block information for response
#[derive(Debug, Clone)]
pub struct FormattedBlockInfo {
    pub hash: String,
    pub number: u64,
}

/// Payouts for a single era - can be either actual payouts or an error message
#[derive(Debug)]
pub enum RawEraPayouts {
    /// Successful payout data for an era
    Payouts(RawEraPayoutsData),
    /// Error message when payouts cannot be calculated
    Message { message: String },
}

/// Actual payout data for an era
#[derive(Debug)]
pub struct RawEraPayoutsData {
    /// Era index
    pub era: u32,
    /// Total reward points for the era
    pub total_era_reward_points: u32,
    /// Total payout for the era
    pub total_era_payout: u128,
    /// Individual payouts for validators nominated
    pub payouts: Vec<RawValidatorPayout>,
}

/// Payout information for a single validator
#[derive(Debug)]
pub struct RawValidatorPayout {
    /// Validator stash account ID
    pub validator_id: String,
    /// Calculated payout amount for the nominator
    pub nominator_staking_payout: u128,
    /// Whether the reward has been claimed
    pub claimed: bool,
    /// Validator's reward points for this era
    pub total_validator_reward_points: u32,
    /// Validator's commission (as parts per billion, 0-1000000000)
    pub validator_commission: u32,
    /// Total stake behind this validator
    pub total_validator_exposure: u128,
    /// Nominator's stake behind this validator
    pub nominator_exposure: u128,
}

// ================================================================================================
// Core Query Function
// ================================================================================================

/// Query staking payouts from storage
///
/// This is the shared function used by both `/accounts/:accountId/staking-payouts`
/// and `/rc/accounts/:accountId/staking-payouts` endpoints.
pub async fn query_staking_payouts(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &ResolvedBlock,
    params: &StakingPayoutsParams,
) -> Result<RawStakingPayouts, StakingPayoutsQueryError> {
    let active_era_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "ActiveEra",
    );

    // Check if Staking pallet exists
    let staking_exists = client_at_block
        .storage()
        .entry(active_era_query.clone())
        .is_ok();

    if !staking_exists {
        return Err(StakingPayoutsQueryError::StakingPalletNotAvailable);
    }

    // Get active era - it's a value storage, use fetch with empty key
    let active_era_entry = client_at_block.storage().entry(active_era_query)?;
    let active_era_value = active_era_entry
        .try_fetch(Vec::<scale_value::Value>::new())
        .await?;
    let active_era = if let Some(value) = active_era_value {
        decode_active_era(&value)?
    } else {
        return Err(StakingPayoutsQueryError::NoActiveEra);
    };

    // Get history depth (default to 84 if not found)
    let history_depth_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "HistoryDepth",
    );
    let history_depth =
        if let Ok(history_entry) = client_at_block.storage().entry(history_depth_query) {
            if let Ok(Some(value)) = history_entry
                .try_fetch(Vec::<scale_value::Value>::new())
                .await
            {
                decode_u32(&value).unwrap_or(84)
            } else {
                84
            }
        } else {
            84
        };

    // Validate depth parameter
    if params.depth == 0 || params.depth > history_depth {
        return Err(StakingPayoutsQueryError::InvalidDepth);
    }

    // Determine target era (default to active_era - 1, which is the last completed era)
    let target_era = params.era.unwrap_or_else(|| active_era.saturating_sub(1));

    // Validate era parameter
    if target_era >= active_era {
        return Err(StakingPayoutsQueryError::InvalidEra(target_era));
    }

    let min_era = active_era.saturating_sub(history_depth);
    if target_era < min_era {
        return Err(StakingPayoutsQueryError::InvalidEra(target_era));
    }

    // Calculate start era based on depth
    let start_era = target_era.saturating_sub(params.depth - 1).max(min_era);

    // Process each era
    let mut eras_payouts = Vec::new();
    for era in start_era..=target_era {
        let era_payout = process_era(client_at_block, account, era, params.unclaimed_only).await;
        eras_payouts.push(era_payout);
    }

    Ok(RawStakingPayouts {
        block: FormattedBlockInfo {
            hash: block.hash.clone(),
            number: block.number,
        },
        eras_payouts,
    })
}

// ================================================================================================
// Era Processing
// ================================================================================================

async fn process_era(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    era: u32,
    unclaimed_only: bool,
) -> RawEraPayouts {
    // Try to get era data
    match fetch_era_data(client_at_block, account, era, unclaimed_only).await {
        Ok(data) => data,
        Err(e) => RawEraPayouts::Message {
            message: format!("Era {}: {}", era, e),
        },
    }
}

async fn fetch_era_data(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    era: u32,
    unclaimed_only: bool,
) -> Result<RawEraPayouts, String> {
    let account_bytes: [u8; 32] = *account.as_ref();

    // Get total era reward points
    let reward_points_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "ErasRewardPoints",
    );
    let reward_points_entry = client_at_block
        .storage()
        .entry(reward_points_query)
        .map_err(|e| e.to_string())?;
    let key = vec![Value::u128(era as u128)];
    let reward_points_value = reward_points_entry
        .try_fetch(key)
        .await
        .map_err(|e| e.to_string())?;

    let (total_era_reward_points, individual_points) = if let Some(value) = reward_points_value {
        decode_era_reward_points(&value).map_err(|e| e.to_string())?
    } else {
        return Ok(RawEraPayouts::Message {
            message: format!("No reward points found for era {}", era),
        });
    };

    // Get total era payout
    let validator_reward_query = subxt::storage::dynamic::<
        Vec<scale_value::Value>,
        scale_value::Value,
    >("Staking", "ErasValidatorReward");
    let validator_reward_entry = client_at_block
        .storage()
        .entry(validator_reward_query)
        .map_err(|e| e.to_string())?;
    let key = vec![Value::u128(era as u128)];
    let validator_reward_value = validator_reward_entry
        .try_fetch(key)
        .await
        .map_err(|e| e.to_string())?;

    let total_era_payout = if let Some(value) = validator_reward_value {
        decode_u128_storage(&value).map_err(|e| e.to_string())?
    } else {
        return Ok(RawEraPayouts::Message {
            message: format!("No validator reward found for era {}", era),
        });
    };

    // Get exposure data - try ErasStakersClipped first
    let exposure_data = fetch_exposure_data(client_at_block, account, era, &account_bytes).await?;

    if exposure_data.is_empty() {
        return Ok(RawEraPayouts::Message {
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
        let commission = fetch_validator_commission(client_at_block, era, &validator_bytes_arr)
            .await
            .unwrap_or(0);

        // Check if claimed
        let claimed = check_if_claimed(client_at_block, &validator_bytes_arr, era).await;

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

        payouts.push(RawValidatorPayout {
            validator_id,
            nominator_staking_payout: nominator_payout,
            claimed,
            total_validator_reward_points: validator_points,
            validator_commission: commission,
            total_validator_exposure: total_exposure,
            nominator_exposure,
        });
    }

    Ok(RawEraPayouts::Payouts(RawEraPayoutsData {
        era,
        total_era_reward_points,
        total_era_payout,
        payouts,
    }))
}

// ================================================================================================
// Storage Fetching Functions
// ================================================================================================

/// Fetch exposure data for an account in an era
/// Returns Vec<(validator_id, nominator_exposure, total_exposure)>
async fn fetch_exposure_data(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    era: u32,
    account_bytes: &[u8; 32],
) -> Result<Vec<(String, u128, u128)>, String> {
    let stakers_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "ErasStakersClipped",
    );
    let nominators_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "Nominators",
    );
    let bonded_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Bonded");

    let mut results = Vec::new();

    // Try ErasStakersClipped first (standard storage)
    if let Ok(stakers_entry) = client_at_block.storage().entry(stakers_query.clone()) {
        // Try to get nominators entry to find which validators this account nominates
        if let Ok(nominators_entry) = client_at_block.storage().entry(nominators_query) {
            let key = vec![Value::from_bytes(account_bytes)];
            if let Ok(Some(nom_value)) = nominators_entry.try_fetch(key).await {
                let targets = decode_nomination_targets(&nom_value);

                for validator_bytes in targets {
                    let key = vec![Value::u128(era as u128), Value::from_bytes(validator_bytes)];
                    if let Ok(Some(exposure_value)) = stakers_entry.try_fetch(key).await
                        && let Ok((total, own, others)) = decode_exposure(&exposure_value)
                    {
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
                            let validator_id = AccountId32::from(validator_bytes).to_ss58check();
                            results.push((validator_id, own, total));
                        }
                    }
                }
            }
        }
    }

    // If account is a validator, also check if they have self-stake
    if let Ok(bonded_entry) = client_at_block.storage().entry(bonded_query) {
        let key = vec![Value::from_bytes(account_bytes)];
        if let Ok(Some(_)) = bonded_entry.try_fetch(key).await {
            // Account is a stash, check if they're also validating
            if let Ok(stakers_entry) = client_at_block.storage().entry(stakers_query) {
                let key = vec![Value::u128(era as u128), Value::from_bytes(account_bytes)];
                if let Ok(Some(exposure_value)) = stakers_entry.try_fetch(key).await
                    && let Ok((total, own, _)) = decode_exposure(&exposure_value)
                {
                    let validator_id = account.to_ss58check();
                    // Only add if not already in results
                    if !results.iter().any(|(v, _, _)| v == &validator_id) {
                        results.push((validator_id, own, total));
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Fetch validator commission for a specific era
async fn fetch_validator_commission(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator_bytes: &[u8; 32],
) -> Option<u32> {
    let prefs_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "ErasValidatorPrefs",
    );
    let prefs_entry = client_at_block.storage().entry(prefs_query).ok()?;
    let key = vec![Value::u128(era as u128), Value::from_bytes(validator_bytes)];
    let prefs_value = prefs_entry.try_fetch(key).await.ok()??;
    decode_validator_commission(&prefs_value).ok()
}

/// Check if rewards have been claimed for a validator in an era
async fn check_if_claimed(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    validator_bytes: &[u8; 32],
    era: u32,
) -> bool {
    let bonded_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Bonded");
    let ledger_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Ledger");

    // First get the controller for this validator
    let controller_bytes = if let Ok(bonded_entry) = client_at_block.storage().entry(bonded_query) {
        let key = vec![Value::from_bytes(validator_bytes)];
        if let Ok(Some(value)) = bonded_entry.try_fetch(key).await {
            decode_account_bytes(&value).unwrap_or(*validator_bytes)
        } else {
            *validator_bytes
        }
    } else {
        *validator_bytes
    };

    // Check ledger for claimed rewards
    if let Ok(ledger_entry) = client_at_block.storage().entry(ledger_query) {
        let key = vec![Value::from_bytes(controller_bytes)];
        if let Ok(Some(value)) = ledger_entry.try_fetch(key).await {
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
    let validator_era_payout = (total_era_payout).saturating_mul(validator_points as u128)
        / (total_era_reward_points as u128);

    // Calculate commission (commission is in parts per billion)
    let commission_payout = validator_era_payout.saturating_mul(commission as u128) / 1_000_000_000;

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
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<u32, StakingPayoutsQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingPayoutsQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode active era",
        ))
    })?;

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            for (name, val) in fields {
                if name == "index"
                    && let ValueDef::Primitive(scale_value::Primitive::U128(v)) = &val.value
                {
                    return Ok(*v as u32);
                }
            }
            Err(StakingPayoutsQueryError::DecodeFailed(
                parity_scale_codec::Error::from("Active era index not found"),
            ))
        }
        _ => Err(StakingPayoutsQueryError::DecodeFailed(
            parity_scale_codec::Error::from("Invalid active era format"),
        )),
    }
}

fn decode_u32(value: &subxt::storage::StorageValue<'_, scale_value::Value>) -> Option<u32> {
    let decoded: Value<()> = value.decode_as().ok()?;
    match &decoded.value {
        ValueDef::Primitive(scale_value::Primitive::U128(v)) => Some(*v as u32),
        _ => None,
    }
}

fn decode_u128_storage(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<u128, String> {
    let decoded: Value<()> = value.decode_as().map_err(|e| e.to_string())?;
    match &decoded.value {
        ValueDef::Primitive(scale_value::Primitive::U128(v)) => Ok(*v),
        _ => Err("Invalid u128 format".to_string()),
    }
}

fn decode_era_reward_points(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
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
                } else if name == "individual"
                    && let ValueDef::Composite(Composite::Unnamed(items)) = &val.value
                {
                    for item in items {
                        if let ValueDef::Composite(Composite::Unnamed(pair)) = &item.value
                            && pair.len() == 2
                            && let Some(account) = extract_account_bytes_from_value(&pair[0])
                            && let ValueDef::Primitive(scale_value::Primitive::U128(points)) =
                                &pair[1].value
                        {
                            individual.insert(account, *points as u32);
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
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<ExposureData, String> {
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
                } else if name == "others"
                    && let ValueDef::Composite(Composite::Unnamed(items)) = &val.value
                {
                    for item in items {
                        if let ValueDef::Composite(Composite::Named(other_fields)) = &item.value {
                            let mut who: Option<[u8; 32]> = None;
                            let mut value_amount = 0u128;
                            for (field_name, field_val) in other_fields {
                                if field_name == "who" {
                                    who = extract_account_bytes_from_value(field_val);
                                } else if field_name == "value"
                                    && let ValueDef::Primitive(scale_value::Primitive::U128(v)) =
                                        &field_val.value
                                {
                                    value_amount = *v;
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
        _ => return Err("Invalid exposure format".to_string()),
    }

    Ok((total, own, others))
}

fn decode_nomination_targets(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Vec<[u8; 32]> {
    let mut targets = Vec::new();

    if let Ok(decoded) = value.decode_as::<Value<()>>()
        && let ValueDef::Composite(Composite::Named(fields)) = &decoded.value
    {
        for (name, val) in fields {
            if name == "targets"
                && let ValueDef::Composite(Composite::Unnamed(items)) = &val.value
            {
                for item in items {
                    if let Some(account) = extract_account_bytes_from_value(item) {
                        targets.push(account);
                    }
                }
            }
        }
    }

    targets
}

fn decode_validator_commission(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<u32, String> {
    let decoded: Value<()> = value.decode_as().map_err(|e| e.to_string())?;

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            for (name, val) in fields {
                if name == "commission"
                    && let ValueDef::Primitive(scale_value::Primitive::U128(v)) = &val.value
                {
                    return Ok(*v as u32);
                }
            }
            Ok(0)
        }
        _ => Ok(0),
    }
}

fn decode_account_bytes(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Option<[u8; 32]> {
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

fn check_claimed_in_ledger(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
    era: u32,
) -> bool {
    if let Ok(decoded) = value.decode_as::<Value<()>>()
        && let ValueDef::Composite(Composite::Named(fields)) = &decoded.value
    {
        // Check claimedRewards field (newer format)
        for (name, val) in fields {
            if (name == "claimedRewards"
                || name == "claimed_rewards"
                || name == "legacyClaimedRewards")
                && let ValueDef::Composite(Composite::Unnamed(eras)) = &val.value
            {
                for era_val in eras {
                    if let ValueDef::Primitive(scale_value::Primitive::U128(e)) = &era_val.value
                        && *e as u32 == era
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}
