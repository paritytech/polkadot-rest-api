//! Common staking payouts utilities shared across handler modules.

use crate::utils::ResolvedBlock;
use parity_scale_codec::Decode;
use sp_core::crypto::{AccountId32, Ss58Codec};
use std::collections::HashMap;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// SCALE Decode Types for Staking Payouts storage
// ================================================================================================

/// Active era info
#[derive(Debug, Clone, Decode)]
struct ActiveEraInfo {
    index: u32,
    start: Option<u64>,
}

/// Era reward points
#[derive(Debug, Clone, Decode)]
struct EraRewardPoints {
    total: u32,
    individual: Vec<([u8; 32], u32)>,
}

/// Exposure structure (era stakers)
#[derive(Debug, Clone, Decode)]
struct ExposureStruct {
    #[codec(compact)]
    total: u128,
    #[codec(compact)]
    own: u128,
    others: Vec<IndividualExposureStruct>,
}

/// Individual exposure in stakers
#[derive(Debug, Clone, Decode)]
struct IndividualExposureStruct {
    who: [u8; 32],
    #[codec(compact)]
    value: u128,
}

/// Nominations structure
#[derive(Debug, Clone, Decode)]
struct NominationsStruct {
    targets: Vec<[u8; 32]>,
    submitted_in: u32,
    suppressed: bool,
}

/// Validator preferences
#[derive(Debug, Clone, Decode)]
struct ValidatorPrefs {
    #[codec(compact)]
    commission: u32,
    blocked: bool,
}

/// Staking ledger for claimed rewards check
#[derive(Debug, Clone, Decode)]
struct StakingLedgerForClaimed {
    stash: [u8; 32],
    #[codec(compact)]
    total: u128,
    #[codec(compact)]
    active: u128,
    unlocking: Vec<UnlockChunkStruct>,
    legacy_claimed_rewards: Vec<u32>,
}

/// Unlock chunk
#[derive(Debug, Clone, Decode)]
struct UnlockChunkStruct {
    #[codec(compact)]
    value: u128,
    #[codec(compact)]
    era: u32,
}

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
    // Check if Staking pallet exists
    if client_at_block
        .storage()
        .entry(("Staking", "ActiveEra"))
        .is_err()
    {
        return Err(StakingPayoutsQueryError::StakingPalletNotAvailable);
    }

    // Get active era
    let active_era_addr = subxt::dynamic::storage::<_, ()>("Staking", "ActiveEra");
    let active_era = if let Ok(value) = client_at_block.storage().fetch(active_era_addr, ()).await {
        let raw_bytes = value.into_bytes();
        decode_active_era(&raw_bytes)?
    } else {
        return Err(StakingPayoutsQueryError::NoActiveEra);
    };

    // Get history depth (default to 84 if not found)
    let history_depth_addr = subxt::dynamic::storage::<_, ()>("Staking", "HistoryDepth");
    let history_depth = if let Ok(value) = client_at_block
        .storage()
        .fetch(history_depth_addr, ())
        .await
    {
        let raw_bytes = value.into_bytes();
        u32::decode(&mut &raw_bytes[..]).unwrap_or(84)
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
    let reward_points_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasRewardPoints");
    let reward_points_value = client_at_block
        .storage()
        .fetch(reward_points_addr, (era,))
        .await;

    let (total_era_reward_points, individual_points) = if let Ok(value) = reward_points_value {
        let raw_bytes = value.into_bytes();
        decode_era_reward_points(&raw_bytes).map_err(|e| e.to_string())?
    } else {
        return Ok(RawEraPayouts::Message {
            message: format!("No reward points found for era {}", era),
        });
    };

    // Get total era payout
    let validator_reward_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasValidatorReward");
    let validator_reward_value = client_at_block
        .storage()
        .fetch(validator_reward_addr, (era,))
        .await;

    let total_era_payout = if let Ok(value) = validator_reward_value {
        let raw_bytes = value.into_bytes();
        decode_u128_storage(&raw_bytes).map_err(|e| e.to_string())?
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
    let mut results = Vec::new();

    // Try to get nominators to find which validators this account nominates
    let nominators_addr = subxt::dynamic::storage::<_, ()>("Staking", "Nominators");
    if let Ok(nom_value) = client_at_block
        .storage()
        .fetch(nominators_addr, (*account_bytes,))
        .await
    {
        let raw_bytes = nom_value.into_bytes();
        let targets = decode_nomination_targets(&raw_bytes);

        for validator_bytes in targets {
            // Try ErasStakersClipped
            let stakers_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasStakersClipped");
            if let Ok(exposure_value) = client_at_block
                .storage()
                .fetch(stakers_addr, (era, validator_bytes))
                .await
            {
                let raw_bytes = exposure_value.into_bytes();
                if let Ok((total, own, others)) = decode_exposure(&raw_bytes) {
                    // Check if account is in the others list
                    for (nominator, value) in &others {
                        if nominator == account_bytes {
                            let validator_id = AccountId32::from(validator_bytes).to_ss58check();
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

    // If account is a validator, also check if they have self-stake
    let bonded_addr = subxt::dynamic::storage::<_, ()>("Staking", "Bonded");
    if client_at_block
        .storage()
        .fetch(bonded_addr, (*account_bytes,))
        .await
        .is_ok()
    {
        // Account is a stash, check if they're also validating
        let stakers_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasStakersClipped");
        if let Ok(exposure_value) = client_at_block
            .storage()
            .fetch(stakers_addr, (era, *account_bytes))
            .await
        {
            let raw_bytes = exposure_value.into_bytes();
            if let Ok((total, own, _)) = decode_exposure(&raw_bytes) {
                let validator_id = account.to_ss58check();
                // Only add if not already in results
                if !results.iter().any(|(v, _, _)| v == &validator_id) {
                    results.push((validator_id, own, total));
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
    let prefs_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasValidatorPrefs");
    let prefs_value = client_at_block
        .storage()
        .fetch(prefs_addr, (era, *validator_bytes))
        .await
        .ok()?;
    let raw_bytes = prefs_value.into_bytes();
    decode_validator_commission(&raw_bytes).ok()
}

/// Check if rewards have been claimed for a validator in an era
async fn check_if_claimed(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    validator_bytes: &[u8; 32],
    era: u32,
) -> bool {
    // First get the controller for this validator
    let bonded_addr = subxt::dynamic::storage::<_, ()>("Staking", "Bonded");
    let controller_bytes = if let Ok(value) = client_at_block
        .storage()
        .fetch(bonded_addr, (*validator_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        decode_account_bytes(&raw_bytes).unwrap_or(*validator_bytes)
    } else {
        *validator_bytes
    };

    // Check ledger for claimed rewards
    let ledger_addr = subxt::dynamic::storage::<_, ()>("Staking", "Ledger");
    if let Ok(value) = client_at_block
        .storage()
        .fetch(ledger_addr, (controller_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        return check_claimed_in_ledger(&raw_bytes, era);
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

/// Decode active era from raw SCALE bytes
fn decode_active_era(raw_bytes: &[u8]) -> Result<u32, StakingPayoutsQueryError> {
    if let Ok(era_info) = ActiveEraInfo::decode(&mut &raw_bytes[..]) {
        return Ok(era_info.index);
    }

    Err(StakingPayoutsQueryError::DecodeFailed(
        parity_scale_codec::Error::from("Failed to decode active era"),
    ))
}

/// Decode u128 from raw SCALE bytes
fn decode_u128_storage(raw_bytes: &[u8]) -> Result<u128, String> {
    u128::decode(&mut &raw_bytes[..]).map_err(|e| e.to_string())
}

/// Decode era reward points from raw SCALE bytes
fn decode_era_reward_points(raw_bytes: &[u8]) -> Result<(u32, HashMap<[u8; 32], u32>), String> {
    if let Ok(points) = EraRewardPoints::decode(&mut &raw_bytes[..]) {
        let individual: HashMap<[u8; 32], u32> = points.individual.into_iter().collect();
        return Ok((points.total, individual));
    }

    Err("Failed to decode era reward points".to_string())
}

/// Decode exposure from raw SCALE bytes
fn decode_exposure(raw_bytes: &[u8]) -> Result<ExposureData, String> {
    if let Ok(exposure) = ExposureStruct::decode(&mut &raw_bytes[..]) {
        let others: Vec<([u8; 32], u128)> = exposure
            .others
            .into_iter()
            .map(|ie| (ie.who, ie.value))
            .collect();
        return Ok((exposure.total, exposure.own, others));
    }

    Err("Failed to decode exposure".to_string())
}

/// Decode nomination targets from raw SCALE bytes
fn decode_nomination_targets(raw_bytes: &[u8]) -> Vec<[u8; 32]> {
    if let Ok(nominations) = NominationsStruct::decode(&mut &raw_bytes[..]) {
        return nominations.targets;
    }

    Vec::new()
}

/// Decode validator commission from raw SCALE bytes
fn decode_validator_commission(raw_bytes: &[u8]) -> Result<u32, String> {
    if let Ok(prefs) = ValidatorPrefs::decode(&mut &raw_bytes[..]) {
        return Ok(prefs.commission);
    }

    Err("Failed to decode validator prefs".to_string())
}

/// Decode account bytes from raw SCALE bytes (for Bonded storage)
fn decode_account_bytes(raw_bytes: &[u8]) -> Option<[u8; 32]> {
    <[u8; 32]>::decode(&mut &raw_bytes[..]).ok()
}

/// Check if rewards have been claimed for an era by looking at ledger
fn check_claimed_in_ledger(raw_bytes: &[u8], era: u32) -> bool {
    if let Ok(ledger) = StakingLedgerForClaimed::decode(&mut &raw_bytes[..]) {
        return ledger.legacy_claimed_rewards.contains(&era);
    }

    false
}
