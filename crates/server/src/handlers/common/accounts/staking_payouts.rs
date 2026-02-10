//! Common staking payouts utilities shared across handler modules.

use crate::consts::{get_chain_display_name, get_migration_boundaries, is_bad_staking_block};
use crate::handlers::runtime_queries::staking;
use crate::utils::ResolvedBlock;
use sp_core::crypto::{AccountId32, Ss58Codec};
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// Types are defined in runtime_queries::staking module

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

    #[error("{0}")]
    BadStakingBlock(String),

    #[error("Relay chain connection is required to query pre-migration era data. Configure a relay chain URL via SAS_SUBSTRATE_MULTI_CHAIN_URL.")]
    RelayChainConnectionRequired,
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
    ss58_prefix: u16,
    spec_name: &str,
    relay_client_at_block: Option<&OnlineClientAtBlock<SubstrateConfig>>,
) -> Result<RawStakingPayouts, StakingPayoutsQueryError> {
    // Check for known bad staking blocks
    if is_bad_staking_block(spec_name, block.number) {
        let chain_name = get_chain_display_name(spec_name);
        return Err(StakingPayoutsQueryError::BadStakingBlock(format!(
            "Post migration, there were some interruptions to staking on {chain_name}, \
             Block {} is in the list of known bad staking blocks in {chain_name}",
            block.number
        )));
    }

    // Check if Staking pallet exists
    if client_at_block
        .storage()
        .entry(("Staking", "ActiveEra"))
        .is_err()
    {
        return Err(StakingPayoutsQueryError::StakingPalletNotAvailable);
    }

    // Get active era
    let active_era = staking::get_active_era(client_at_block)
        .await
        .ok_or(StakingPayoutsQueryError::NoActiveEra)?;

    // Get history depth (default to 84 if not found)
    let history_depth = staking::get_history_depth(client_at_block).await;

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

    // Check if migration-aware era splitting is needed
    let migration_boundaries = get_migration_boundaries(spec_name);

    let mut eras_payouts = Vec::new();

    if let Some(boundaries) = migration_boundaries {
        // Split era range at the migration boundary
        let relay_last_era = boundaries.relay_chain_last_era;
        let ah_first_era = boundaries.asset_hub_first_era;

        // Pre-migration eras: [start_era, min(target_era, relay_last_era - 1)]
        let pre_migration_end = target_era.min(relay_last_era.saturating_sub(1));
        let has_pre_migration_eras = start_era <= pre_migration_end && start_era < relay_last_era;

        // Post-migration eras: [max(start_era, ah_first_era), target_era]
        let post_migration_start = start_era.max(ah_first_era);
        let has_post_migration_eras = post_migration_start <= target_era;

        // Process pre-migration eras from relay chain
        if has_pre_migration_eras {
            let rc_client = relay_client_at_block
                .ok_or(StakingPayoutsQueryError::RelayChainConnectionRequired)?;

            for era in start_era..=pre_migration_end {
                let era_payout = process_era(
                    rc_client,
                    account,
                    era,
                    params.unclaimed_only,
                    ss58_prefix,
                )
                .await;
                eras_payouts.push(era_payout);
            }
        }

        // Process post-migration eras from Asset Hub
        if has_post_migration_eras {
            for era in post_migration_start..=target_era {
                let era_payout = process_era(
                    client_at_block,
                    account,
                    era,
                    params.unclaimed_only,
                    ss58_prefix,
                )
                .await;
                eras_payouts.push(era_payout);
            }
        }
    } else {
        // No migration boundaries — process all eras from the connected chain
        for era in start_era..=target_era {
            let era_payout = process_era(
                client_at_block,
                account,
                era,
                params.unclaimed_only,
                ss58_prefix,
            )
            .await;
            eras_payouts.push(era_payout);
        }
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
    ss58_prefix: u16,
) -> RawEraPayouts {
    // Try to get era data
    match fetch_era_data(client_at_block, account, era, unclaimed_only, ss58_prefix).await {
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
    ss58_prefix: u16,
) -> Result<RawEraPayouts, String> {
    let account_bytes: [u8; 32] = *account.as_ref();

    // Get total era reward points
    let (total_era_reward_points, individual_points) =
        match staking::get_era_reward_points(client_at_block, era).await {
            Some(points) => points,
            None => {
                return Ok(RawEraPayouts::Message {
                    message: format!("No reward points found for era {}", era),
                });
            }
        };

    // Get total era payout
    let total_era_payout = match staking::get_era_validator_reward(client_at_block, era).await {
        Some(reward) => reward,
        None => {
            return Ok(RawEraPayouts::Message {
                message: format!("No validator reward found for era {}", era),
            });
        }
    };

    // Get exposure data
    let exposure_data =
        fetch_exposure_data(client_at_block, account, era, &account_bytes, ss58_prefix).await?;

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

/// Fetch exposure data for an account in an era.
/// Returns Vec<(validator_id, nominator_exposure, total_exposure)>
///
/// Strategy:
/// 1. First try targeted approach using current nominations (fast)
/// 2. If no results, fall back to bulk approach (slower but handles historical eras
///    where nominations may have changed)
async fn fetch_exposure_data(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    era: u32,
    account_bytes: &[u8; 32],
    ss58_prefix: u16,
) -> Result<Vec<(String, u128, u128)>, String> {
    // First try the targeted approach using current nominations
    let mut results = Vec::new();

    // Get the account's nominations to find which validators to query
    // Note: This uses current nominations which may differ from historical eras
    let nominations = staking::get_nominations(client_at_block, account, ss58_prefix).await;
    // Also check if account is a validator
    let is_validator = staking::is_validator(client_at_block, account).await;
    // Collect validator addresses to query
    let mut validators_to_query: Vec<AccountId32> = Vec::new();

    if let Some(noms) = &nominations {
        for target_ss58 in &noms.targets {
            if let Ok(validator_account) = AccountId32::from_ss58check(target_ss58) {
                validators_to_query.push(validator_account);
            }
        }
    }

    // If account is a validator, also check their own exposure
    if is_validator {
        validators_to_query.push(account.clone());
    }

    if validators_to_query.is_empty() {
        return Ok(results);
    }

    // Query each validator's exposure to find the account's stake
    for validator in &validators_to_query {
        if let Some(exposure) = find_account_in_validator_exposure(
            client_at_block,
            era,
            validator,
            account_bytes,
            ss58_prefix,
        )
        .await
        {
            let validator_ss58 = validator.to_ss58check_with_version(ss58_prefix.into());
            if !results.iter().any(|(v, _, _)| v == &validator_ss58) {
                results.push(exposure);
            }
        }
    }
    Ok(results)
}

/// Find an account's exposure within a specific validator's stakers.
/// Returns Some((validator_ss58, nominator_exposure, total_exposure)) if found.
async fn find_account_in_validator_exposure(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator: &AccountId32,
    account_bytes: &[u8; 32],
    ss58_prefix: u16,
) -> Option<(String, u128, u128)> {
    let validator_bytes: [u8; 32] = *validator.as_ref();
    let validator_ss58 = validator.to_ss58check_with_version(ss58_prefix.into());

    // Try paged staking first (ErasStakersOverview + ErasStakersPaged)
    if let Some((total, own, page_count)) =
        staking::get_era_stakers_overview(client_at_block, era, validator).await
    {
        // Check if account is the validator itself
        if account_bytes == &validator_bytes {
            return Some((validator_ss58, own, total));
        }

        // Search through paged exposures for the account
        for page in 0..page_count {
            if let Some((_page_total, others)) =
                staking::get_era_stakers_paged(client_at_block, era, validator, page).await
            {
                for (nominator_bytes, nominator_value) in others {
                    if &nominator_bytes == account_bytes {
                        return Some((validator_ss58.clone(), nominator_value, total));
                    }
                }
            }
        }
        return None;
    }

    // Fall back to legacy ErasStakersClipped
    if let Some((total, own, others)) =
        staking::get_era_stakers_clipped(client_at_block, era, validator).await
    {
        // Check if account is the validator itself
        if account_bytes == &validator_bytes {
            return Some((validator_ss58, own, total));
        }

        // Search through nominators for the account
        for (nominator_bytes, nominator_value) in others {
            if &nominator_bytes == account_bytes {
                return Some((validator_ss58.clone(), nominator_value, total));
            }
        }
    }

    None
}

/// Fetch validator commission for a specific era
async fn fetch_validator_commission(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator_bytes: &[u8; 32],
) -> Option<u32> {
    let validator_account = AccountId32::from(*validator_bytes);
    staking::get_era_validator_prefs(client_at_block, era, &validator_account).await
}

/// Check if rewards have been claimed for a validator in an era
async fn check_if_claimed(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    validator_bytes: &[u8; 32],
    era: u32,
) -> bool {
    let validator_account = AccountId32::from(*validator_bytes);
    staking::is_era_claimed(client_at_block, era, &validator_account).await
}

// ================================================================================================
// Payout Calculation
// ================================================================================================

/// Calculate the payout for a nominator
///
/// Matches Substrate's on-chain calculation which uses floor division at each step:
/// 1. validator_total_reward = total_era_payout * validator_points / total_era_points
/// 2. validator_commission = validator_total_reward * commission / BILLION
/// 3. staker_reward_pool = validator_total_reward - validator_commission
/// 4. nominator_payout = staker_reward_pool * nominator_exposure / total_exposure
fn calculate_payout(
    total_era_reward_points: u32,
    total_era_payout: u128,
    validator_points: u32,
    commission: u32, // Parts per billion (0 - 1_000_000_000)
    nominator_exposure: u128,
    total_exposure: u128,
    is_validator: bool,
) -> u128 {
    const BILLION: u128 = 1_000_000_000;

    if total_era_reward_points == 0 || total_exposure == 0 {
        return 0;
    }

    // Step 1: Calculate validator's total reward from the era
    // validator_total_reward = total_era_payout * validator_points / total_era_points
    let validator_total_reward = total_era_payout.saturating_mul(validator_points as u128)
        / (total_era_reward_points as u128);

    // Step 2: Calculate commission taken by validator
    // validator_commission = validator_total_reward * commission / BILLION
    let validator_commission_payout =
        validator_total_reward.saturating_mul(commission as u128) / BILLION;

    // Step 3: Calculate the reward pool for all stakers (validator + nominators)
    // staker_reward_pool = validator_total_reward - validator_commission
    let staker_reward_pool = validator_total_reward.saturating_sub(validator_commission_payout);

    // Step 4: Calculate nominator's share of the staker reward pool
    // nominator_payout = staker_reward_pool * nominator_exposure / total_exposure
    let staker_payout = staker_reward_pool.saturating_mul(nominator_exposure) / total_exposure;

    if is_validator {
        // Validator gets their staker share plus commission
        staker_payout.saturating_add(validator_commission_payout)
    } else {
        staker_payout
    }
}

// Decoding functions have been moved to runtime_queries::staking module
