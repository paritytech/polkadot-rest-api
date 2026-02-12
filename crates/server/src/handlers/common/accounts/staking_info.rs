//! Common staking info utilities shared across handler modules.

use crate::consts::{get_chain_display_name, is_bad_staking_block};
use crate::handlers::runtime_queries::staking::{self, StakingStorageError};
use crate::utils::ResolvedBlock;
use futures::future::join_all;
use sp_core::crypto::{AccountId32, Ss58Codec};
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// Re-export types from runtime_queries::staking for backwards compatibility
pub use crate::handlers::runtime_queries::staking::{
    DecodedNominationsInfo, DecodedRewardDestination, DecodedStakingLedger, DecodedUnlockingChunk,
};

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum StakingQueryError {
    #[error("The runtime does not include the staking pallet at this block")]
    StakingPalletNotAvailable,

    #[error("The address is not a stash account")]
    NotAStashAccount,

    #[error("Staking ledger not found")]
    LedgerNotFound,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(Box<subxt::error::OnlineClientAtBlockError>),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(Box<subxt::error::StorageError>),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("{0}")]
    BadStakingBlock(String),
}

impl From<subxt::error::OnlineClientAtBlockError> for StakingQueryError {
    fn from(err: subxt::error::OnlineClientAtBlockError) -> Self {
        StakingQueryError::ClientAtBlockFailed(Box::new(err))
    }
}

impl From<subxt::error::StorageError> for StakingQueryError {
    fn from(err: subxt::error::StorageError) -> Self {
        StakingQueryError::StorageQueryFailed(Box::new(err))
    }
}

impl From<StakingStorageError> for StakingQueryError {
    fn from(err: StakingStorageError) -> Self {
        match err {
            StakingStorageError::NotAStashAccount => StakingQueryError::NotAStashAccount,
            StakingStorageError::LedgerNotFound => StakingQueryError::LedgerNotFound,
            StakingStorageError::DecodeFailed(e) => StakingQueryError::DecodeFailed(e),
            StakingStorageError::InvalidAddress(addr) => StakingQueryError::InvalidAddress(addr),
        }
    }
}

// ================================================================================================
// Public Data Types
// ================================================================================================

/// Raw staking info data returned from storage query
#[derive(Debug)]
pub struct RawStakingInfo {
    /// Block information
    pub block: FormattedBlockInfo,
    /// Controller address
    pub controller: String,
    /// Reward destination
    pub reward_destination: DecodedRewardDestination,
    /// Number of slashing spans
    pub num_slashing_spans: u32,
    /// Nominations info (None if not a nominator)
    pub nominations: Option<DecodedNominationsInfo>,
    /// Staking ledger with optional claimed rewards
    pub staking: StakingLedgerWithClaims,
}

/// Block information for response
#[derive(Debug, Clone)]
pub struct FormattedBlockInfo {
    pub hash: String,
    pub number: u64,
}

/// Staking ledger with optional claimed rewards
#[derive(Debug, Clone)]
pub struct StakingLedgerWithClaims {
    /// Stash account address
    pub stash: String,
    /// Total locked balance (active + unlocking)
    pub total: String,
    /// Active staked balance
    pub active: String,
    /// Unlocking chunks
    pub unlocking: Vec<DecodedUnlockingChunk>,
    /// Claimed rewards per era (only populated when include_claimed_rewards=true)
    pub claimed_rewards: Option<Vec<EraClaimStatus>>,
}

impl From<DecodedStakingLedger> for StakingLedgerWithClaims {
    fn from(ledger: DecodedStakingLedger) -> Self {
        Self {
            stash: ledger.stash,
            total: ledger.total,
            active: ledger.active,
            unlocking: ledger.unlocking,
            claimed_rewards: None,
        }
    }
}

/// Claim status for a specific era
#[derive(Debug, Clone)]
pub struct EraClaimStatus {
    /// Era index
    pub era: u32,
    /// Claim status
    pub status: ClaimStatus,
}

/// Possible claim statuses
#[derive(Debug, Clone)]
pub enum ClaimStatus {
    /// All rewards for this era have been claimed
    Claimed,
    /// No rewards for this era have been claimed
    Unclaimed,
    /// Some but not all rewards have been claimed (paged staking)
    PartiallyClaimed,
    /// Unable to determine status
    Undefined,
}

impl ClaimStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimStatus::Claimed => "claimed",
            ClaimStatus::Unclaimed => "unclaimed",
            ClaimStatus::PartiallyClaimed => "partially claimed",
            ClaimStatus::Undefined => "undefined",
        }
    }
}

// ================================================================================================
// Core Query Function
// ================================================================================================

/// Query staking info from storage
///
/// This is the shared function used by both `/accounts/:accountId/staking-info`
/// and `/rc/accounts/:accountId/staking-info` endpoints.
pub async fn query_staking_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &ResolvedBlock,
    include_claimed_rewards: bool,
    ss58_prefix: u16,
    spec_name: &str,
) -> Result<RawStakingInfo, StakingQueryError> {
    // Check for known bad staking blocks
    if is_bad_staking_block(spec_name, block.number) {
        let chain_name = get_chain_display_name(spec_name);
        return Err(StakingQueryError::BadStakingBlock(format!(
            "Post migration, there were some interruptions to staking on {chain_name}, \
             Block {} is in the list of known bad staking blocks in {chain_name}",
            block.number
        )));
    }

    // Check if Staking pallet exists
    if client_at_block
        .storage()
        .entry(("Staking", "Bonded"))
        .is_err()
    {
        return Err(StakingQueryError::StakingPalletNotAvailable);
    }

    let controller = staking::get_bonded_controller(client_at_block, account, ss58_prefix).await?;
    let controller_account = AccountId32::from_string(&controller).map_err(|_| {
        StakingStorageError::DecodeFailed("Failed to decode controller account".into())
    })?;

    // Run all independent queries in parallel
    // Pass `account` as the expected stash for ledger validation
    let (ledger_result, reward_destination, nominations, num_slashing_spans) = tokio::join!(
        staking::get_staking_ledger(client_at_block, &controller_account, ss58_prefix),
        staking::get_reward_destination(client_at_block, account, ss58_prefix),
        staking::get_nominations(client_at_block, account, ss58_prefix),
        staking::get_slashing_spans_count(client_at_block, account),
    );

    let ledger = ledger_result?;

    // Query claimed rewards if requested
    let claimed_rewards = if include_claimed_rewards {
        query_claimed_rewards(client_at_block, account, &nominations)
            .await
            .ok()
    } else {
        None
    };

    // Convert ledger and add claimed rewards
    let mut staking: StakingLedgerWithClaims = ledger.into();
    staking.claimed_rewards = claimed_rewards;

    Ok(RawStakingInfo {
        block: FormattedBlockInfo {
            hash: block.hash.clone(),
            number: block.number,
        },
        controller,
        reward_destination,
        num_slashing_spans,
        nominations,
        staking,
    })
}

// ================================================================================================
// Claimed Rewards Query
// ================================================================================================

/// Query claimed rewards status for each era within the history depth
async fn query_claimed_rewards(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash: &AccountId32,
    nominations: &Option<DecodedNominationsInfo>,
) -> Result<Vec<EraClaimStatus>, StakingQueryError> {
    // Fetch era info and validator status in parallel
    let (current_era_opt, history_depth, is_validator) = tokio::join!(
        staking::get_current_era(client_at_block),
        staking::get_history_depth(client_at_block),
        staking::is_validator(client_at_block, stash),
    );

    let current_era = current_era_opt.ok_or_else(|| {
        StakingQueryError::DecodeFailed(parity_scale_codec::Error::from("CurrentEra not found"))
    })?;
    let era_start = current_era.saturating_sub(history_depth);

    // Query all eras in parallel
    // Note: For nominators, this checks current nominations against each era's claimed status.
    // If nominations have changed over time, historical eras may show inaccurate status.
    let era_futures: Vec<_> = (era_start..current_era)
        .map(|era| {
            let nominations = nominations.clone();
            async move {
                let status = if let Some(noms) = &nominations {
                    // Account has nominations, check if any nominated validator has claimed
                    query_nominator_claim_status(client_at_block, era, &noms.targets).await
                } else if is_validator {
                    // No nominations but is a validator, check own claimed rewards
                    query_validator_claim_status(client_at_block, era, stash).await
                } else {
                    // Neither nominator nor validator
                    ClaimStatus::Undefined
                };
                EraClaimStatus { era, status }
            }
        })
        .collect();

    let claimed_rewards = join_all(era_futures).await;

    Ok(claimed_rewards)
}

/// Query claim status for a validator at a specific era
async fn query_validator_claim_status(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator: &AccountId32,
) -> ClaimStatus {
    // Query claimed pages and page count in parallel
    let (claimed_pages, page_count) = tokio::join!(
        staking::get_claimed_pages(client_at_block, era, validator),
        staking::get_era_stakers_page_count(client_at_block, era, validator),
    );

    match (claimed_pages, page_count) {
        (Some(pages), Some(total)) => {
            if pages.is_empty() {
                ClaimStatus::Unclaimed
            } else if pages.len() as u32 >= total {
                ClaimStatus::Claimed
            } else {
                ClaimStatus::PartiallyClaimed
            }
        }
        (Some(pages), None) => {
            if pages.is_empty() {
                ClaimStatus::Unclaimed
            } else {
                ClaimStatus::Claimed
            }
        }
        (None, Some(_)) => ClaimStatus::Unclaimed,
        (None, None) => ClaimStatus::Undefined,
    }
}

/// Query claim status for a nominator at a specific era
///
/// This follows Sidecar's approach: iterate through nominated validators and return
/// the status of the first validator that was active in that era.
///
/// Note: This checks CURRENT nominated validators, not historical ones.
/// If nominations changed over time, this may not accurately reflect historical eras.
async fn query_nominator_claim_status(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator_targets: &[String],
) -> ClaimStatus {
    // Iterate through nominated validators, find the first one that was active in this era
    for (idx, validator_ss58) in validator_targets.iter().enumerate() {
        if let Ok(validator_account) = AccountId32::from_ss58check(validator_ss58) {
            // Query both page count (to check if validator was active) and claimed pages
            let (page_count, claimed_pages) = tokio::join!(
                staking::get_era_stakers_page_count(client_at_block, era, &validator_account),
                staking::get_claimed_pages(client_at_block, era, &validator_account),
            );

            // Check if validator was active in this era (has ErasStakersOverview data)
            if let Some(total_pages) = page_count {
                // Validator was active, determine claim status
                let status = match claimed_pages {
                    Some(pages) => {
                        if pages.is_empty() {
                            ClaimStatus::Unclaimed
                        } else if pages.len() as u32 >= total_pages {
                            ClaimStatus::Claimed
                        } else {
                            ClaimStatus::PartiallyClaimed
                        }
                    }
                    None => ClaimStatus::Unclaimed,
                };
                return status;
            }
            // Validator not active in this era, try next one
            // If this is the last validator and none were active, return Undefined
            if idx == validator_targets.len() - 1 {
                return ClaimStatus::Undefined;
            }
        }
    }

    ClaimStatus::Undefined
}
