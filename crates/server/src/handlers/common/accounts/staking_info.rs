//! Common staking info utilities shared across handler modules.

use crate::utils::ResolvedBlock;
use parity_scale_codec::Decode;
use sp_core::crypto::{AccountId32, Ss58Codec};
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// SCALE Decode Types for Staking storage
// ================================================================================================

/// Unlocking chunk in staking ledger
#[derive(Debug, Clone, Decode)]
struct UnlockChunk {
    value: u128,
    era: u32,
}

/// Staking ledger structure
#[derive(Debug, Clone, Decode)]
struct StakingLedger {
    stash: [u8; 32],
    #[codec(compact)]
    total: u128,
    #[codec(compact)]
    active: u128,
    unlocking: Vec<UnlockChunk>,
    // Legacy field - may not exist in newer runtimes
    // claimed_rewards: Vec<u32>,
}

/// Reward destination enum
#[derive(Debug, Clone, Decode)]
enum RewardDestinationType {
    Staked,
    Stash,
    Controller,
    Account([u8; 32]),
    None,
}

/// Nominations structure
#[derive(Debug, Clone, Decode)]
struct Nominations {
    targets: Vec<[u8; 32]>,
    submitted_in: u32,
    suppressed: bool,
}

/// Slashing spans structure
#[derive(Debug, Clone, Decode)]
struct SlashingSpans {
    span_index: u32,
    last_start: u32,
    last_nonzero_slash: u32,
    prior: Vec<u32>,
}

/// Era stakers overview (for paged staking)
#[derive(Debug, Clone, Decode)]
struct PagedExposureMetadata {
    #[codec(compact)]
    total: u128,
    #[codec(compact)]
    own: u128,
    nominator_count: u32,
    page_count: u32,
}

/// Legacy era stakers (non-paged)
#[derive(Debug, Clone, Decode)]
struct Exposure {
    #[codec(compact)]
    total: u128,
    #[codec(compact)]
    own: u128,
    others: Vec<IndividualExposure>,
}

/// Individual exposure in legacy stakers
#[derive(Debug, Clone, Decode)]
struct IndividualExposure {
    who: [u8; 32],
    #[codec(compact)]
    value: u128,
}

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum StakingQueryError {
    #[error("The runtime does not include the staking pallet at this block")]
    StakingPalletNotAvailable,

    #[error("The address is not a stash account")]
    NotAStashAccount,

    #[error("Staking ledger not found for controller")]
    LedgerNotFound,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(Box<subxt::error::OnlineClientAtBlockError>),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(Box<subxt::error::StorageError>),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),
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

// ================================================================================================
// Data Types
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
    /// Staking ledger
    pub staking: DecodedStakingLedger,
}

/// Block information for response
#[derive(Debug, Clone)]
pub struct FormattedBlockInfo {
    pub hash: String,
    pub number: u64,
}

/// Decoded reward destination
#[derive(Debug, Clone)]
pub enum DecodedRewardDestination {
    /// Simple variant without account (Staked, Stash, Controller, None)
    Simple(String),
    /// Account variant with specific address
    Account { account: String },
}

/// Decoded nominations info
#[derive(Debug, Clone)]
pub struct DecodedNominationsInfo {
    /// List of validator addresses being nominated
    pub targets: Vec<String>,
    /// Era in which nomination was submitted
    pub submitted_in: String,
    /// Whether nominations are suppressed
    pub suppressed: bool,
}

/// Decoded staking ledger
#[derive(Debug, Clone)]
pub struct DecodedStakingLedger {
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

/// Decoded unlocking chunk
#[derive(Debug, Clone)]
pub struct DecodedUnlockingChunk {
    /// Amount being unlocked
    pub value: String,
    /// Era when funds become available
    pub era: String,
}

// ================================================================================================
// Core Query Function
// ================================================================================================

/// Query staking info from storage
///
/// This is the shared function used by both `/accounts/:accountId/staking-info`
/// and `/rc/accounts/:accountId/staking-info` endpoints.
///
/// # Arguments
/// * `client_at_block` - The client at the specific block
/// * `account` - The stash account to query
/// * `block` - The resolved block information
/// * `include_claimed_rewards` - Whether to fetch claimed rewards status per era
pub async fn query_staking_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &ResolvedBlock,
    include_claimed_rewards: bool,
) -> Result<RawStakingInfo, StakingQueryError> {
    // Check if Staking pallet exists
    if client_at_block
        .storage()
        .entry(("Staking", "Bonded"))
        .is_err()
    {
        return Err(StakingQueryError::StakingPalletNotAvailable);
    }

    let account_bytes: [u8; 32] = *account.as_ref();

    // Query Staking.Bonded to get controller from stash
    let bonded_addr = subxt::dynamic::storage::<_, ()>("Staking", "Bonded");
    let bonded_value = client_at_block
        .storage()
        .fetch(bonded_addr, (account_bytes,))
        .await;

    let controller = if let Ok(value) = bonded_value {
        let raw_bytes = value.into_bytes();
        decode_account_id(&raw_bytes)?
    } else {
        // Address is not a stash account
        return Err(StakingQueryError::NotAStashAccount);
    };

    let controller_account = AccountId32::from_ss58check(&controller)
        .map_err(|_| StakingQueryError::InvalidAddress(controller.clone()))?;
    let controller_bytes: [u8; 32] = *controller_account.as_ref();

    // Query Staking.Ledger to get staking ledger
    let ledger_addr = subxt::dynamic::storage::<_, ()>("Staking", "Ledger");
    let ledger_value = client_at_block
        .storage()
        .fetch(ledger_addr, (controller_bytes,))
        .await;

    let staking = if let Ok(value) = ledger_value {
        let raw_bytes = value.into_bytes();
        decode_staking_ledger(&raw_bytes)?
    } else {
        return Err(StakingQueryError::LedgerNotFound);
    };

    // Query Staking.Payee to get reward destination
    let payee_addr = subxt::dynamic::storage::<_, ()>("Staking", "Payee");
    let payee_value = client_at_block
        .storage()
        .fetch(payee_addr, (account_bytes,))
        .await;

    let reward_destination = if let Ok(value) = payee_value {
        let raw_bytes = value.into_bytes();
        decode_reward_destination(&raw_bytes)?
    } else {
        DecodedRewardDestination::Simple("Staked".to_string())
    };

    // Query Staking.Nominators to get nominations
    let nominators_addr = subxt::dynamic::storage::<_, ()>("Staking", "Nominators");
    let nominators_value = client_at_block
        .storage()
        .fetch(nominators_addr, (account_bytes,))
        .await;

    let nominations = if let Ok(value) = nominators_value {
        let raw_bytes = value.into_bytes();
        decode_nominations(&raw_bytes)?
    } else {
        None
    };

    // Query Staking.SlashingSpans to get number of slashing spans
    let slashing_addr = subxt::dynamic::storage::<_, ()>("Staking", "SlashingSpans");
    let num_slashing_spans = if let Ok(value) = client_at_block
        .storage()
        .fetch(slashing_addr, (account_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        decode_slashing_spans(&raw_bytes).unwrap_or(0)
    } else {
        0
    };

    // Query claimed rewards if requested
    let claimed_rewards = if include_claimed_rewards {
        query_claimed_rewards(client_at_block, account, &nominations)
            .await
            .ok()
    } else {
        None
    };

    // Update staking with claimed rewards
    let staking = DecodedStakingLedger {
        claimed_rewards,
        ..staking
    };

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
// Decoding Functions
// ================================================================================================

/// Decode an AccountId from raw SCALE bytes
fn decode_account_id(raw_bytes: &[u8]) -> Result<String, StakingQueryError> {
    if let Ok(account_bytes) = <[u8; 32]>::decode(&mut &raw_bytes[..]) {
        let account_id = AccountId32::from(account_bytes);
        return Ok(account_id.to_ss58check());
    }

    Err(StakingQueryError::DecodeFailed(
        parity_scale_codec::Error::from("Failed to decode account id"),
    ))
}

/// Decode staking ledger from raw SCALE bytes
fn decode_staking_ledger(raw_bytes: &[u8]) -> Result<DecodedStakingLedger, StakingQueryError> {
    if let Ok(ledger) = StakingLedger::decode(&mut &raw_bytes[..]) {
        let stash = AccountId32::from(ledger.stash).to_ss58check();

        let unlocking = ledger
            .unlocking
            .into_iter()
            .map(|chunk| DecodedUnlockingChunk {
                value: chunk.value.to_string(),
                era: chunk.era.to_string(),
            })
            .collect();

        return Ok(DecodedStakingLedger {
            stash,
            total: ledger.total.to_string(),
            active: ledger.active.to_string(),
            unlocking,
            claimed_rewards: None, // Will be populated later if requested
        });
    }

    Err(StakingQueryError::DecodeFailed(
        parity_scale_codec::Error::from("Failed to decode staking ledger"),
    ))
}

/// Decode reward destination from raw SCALE bytes
fn decode_reward_destination(
    raw_bytes: &[u8],
) -> Result<DecodedRewardDestination, StakingQueryError> {
    if let Ok(dest) = RewardDestinationType::decode(&mut &raw_bytes[..]) {
        return Ok(match dest {
            RewardDestinationType::Staked => DecodedRewardDestination::Simple("Staked".to_string()),
            RewardDestinationType::Stash => DecodedRewardDestination::Simple("Stash".to_string()),
            RewardDestinationType::Controller => {
                DecodedRewardDestination::Simple("Controller".to_string())
            }
            RewardDestinationType::None => DecodedRewardDestination::Simple("None".to_string()),
            RewardDestinationType::Account(account_bytes) => {
                let account = AccountId32::from(account_bytes).to_ss58check();
                DecodedRewardDestination::Account { account }
            }
        });
    }

    // Default to Staked if decoding fails
    Ok(DecodedRewardDestination::Simple("Staked".to_string()))
}

/// Decode nominations from raw SCALE bytes
fn decode_nominations(
    raw_bytes: &[u8],
) -> Result<Option<DecodedNominationsInfo>, StakingQueryError> {
    if let Ok(nominations) = Nominations::decode(&mut &raw_bytes[..]) {
        let targets = nominations
            .targets
            .into_iter()
            .map(|bytes| AccountId32::from(bytes).to_ss58check())
            .collect();

        return Ok(Some(DecodedNominationsInfo {
            targets,
            submitted_in: nominations.submitted_in.to_string(),
            suppressed: nominations.suppressed,
        }));
    }

    Ok(None)
}

/// Decode slashing spans count from raw SCALE bytes
fn decode_slashing_spans(raw_bytes: &[u8]) -> Result<u32, StakingQueryError> {
    if let Ok(spans) = SlashingSpans::decode(&mut &raw_bytes[..]) {
        // Count is prior.length + 1
        return Ok(spans.prior.len() as u32 + 1);
    }

    Ok(0)
}

// ================================================================================================
// Claimed Rewards Query
// ================================================================================================

/// Query claimed rewards status for each era within the history depth.
///
/// This function checks the claim status for each era by querying:
/// - `Staking.CurrentEra` - to get the current era
/// - `Staking.HistoryDepth` - to get how many eras to check
/// - `Staking.ClaimedRewards(era, validator)` - to get claimed page indices
/// - `Staking.ErasStakersOverview(era, validator)` - to get page count for validators
async fn query_claimed_rewards(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash: &AccountId32,
    nominations: &Option<DecodedNominationsInfo>,
) -> Result<Vec<EraClaimStatus>, StakingQueryError> {
    let stash_bytes: [u8; 32] = *stash.as_ref();

    // Get current era
    let current_era = get_current_era(client_at_block).await?;

    // Get history depth (defaults to 84 if not found)
    let history_depth = get_history_depth(client_at_block).await.unwrap_or(84);

    // Calculate era range to check
    let era_start = current_era.saturating_sub(history_depth);

    // Check if account is a validator
    let is_validator = is_validator(client_at_block, stash).await;

    let mut claimed_rewards = Vec::new();

    for era in era_start..current_era {
        let status = if is_validator {
            query_validator_claim_status(client_at_block, era, &stash_bytes).await
        } else if let Some(noms) = nominations {
            // For nominators, check claim status via their nominated validators
            query_nominator_claim_status(client_at_block, era, &noms.targets, stash).await
        } else {
            ClaimStatus::Undefined
        };

        claimed_rewards.push(EraClaimStatus { era, status });
    }

    Ok(claimed_rewards)
}

/// Get the current era from storage
async fn get_current_era(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, StakingQueryError> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "CurrentEra");

    if let Ok(value) = client_at_block.storage().fetch(storage_addr, ()).await {
        let raw_bytes = value.into_bytes();
        // CurrentEra is Option<u32>, so we need to handle the Option wrapper
        // In SCALE, Some(value) is encoded as 0x01 + value, None is 0x00
        if !raw_bytes.is_empty()
            && raw_bytes[0] == 1
            && raw_bytes.len() >= 5
            && let Ok(era) = u32::decode(&mut &raw_bytes[1..])
        {
            return Ok(era);
        }
        // Try direct u32 decode (some runtimes may not wrap in Option)
        if let Ok(era) = u32::decode(&mut &raw_bytes[..]) {
            return Ok(era);
        }
    }

    Err(StakingQueryError::DecodeFailed(
        parity_scale_codec::Error::from("CurrentEra not found"),
    ))
}

/// Get history depth constant from storage
///
/// The history depth determines how many eras we check for claimed rewards.
/// Default is 84 eras for most Substrate chains.
async fn get_history_depth(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> Option<u32> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "HistoryDepth");

    if let Ok(value) = client_at_block.storage().fetch(storage_addr, ()).await {
        let raw_bytes = value.into_bytes();
        if let Ok(depth) = u32::decode(&mut &raw_bytes[..]) {
            return Some(depth);
        }
    }

    // Default history depth for most chains
    Some(84)
}

/// Check if an account is a validator
async fn is_validator(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash: &AccountId32,
) -> bool {
    let stash_bytes: [u8; 32] = *stash.as_ref();

    // Query Staking.Validators to check if account is a validator
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "Validators");

    client_at_block
        .storage()
        .fetch(storage_addr, (stash_bytes,))
        .await
        .is_ok()
}

/// Query claim status for a validator at a specific era
async fn query_validator_claim_status(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    stash_bytes: &[u8; 32],
) -> ClaimStatus {
    // Try new storage: Staking.ClaimedRewards(era, validator) -> Vec<u32> (page indices)
    let claimed_pages = get_claimed_pages(client_at_block, era, stash_bytes).await;

    // Get page count from ErasStakersOverview
    let page_count = get_era_stakers_page_count(client_at_block, era, stash_bytes).await;

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
            // Have claimed pages but can't determine total
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
async fn query_nominator_claim_status(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator_targets: &[String],
    _nominator_stash: &AccountId32,
) -> ClaimStatus {
    // For nominators, we check the claim status of their nominated validators
    // If any validator has claimed, the nominator's rewards for that era are also claimed
    for validator_ss58 in validator_targets {
        if let Ok(validator_account) = AccountId32::from_ss58check(validator_ss58) {
            let validator_bytes: [u8; 32] = *validator_account.as_ref();
            let status = query_validator_claim_status(client_at_block, era, &validator_bytes).await;

            // Return the first definitive status found
            match status {
                ClaimStatus::Claimed | ClaimStatus::PartiallyClaimed => return status,
                ClaimStatus::Unclaimed => return status,
                ClaimStatus::Undefined => continue,
            }
        }
    }

    ClaimStatus::Undefined
}

/// Get claimed page indices for a validator at a specific era
async fn get_claimed_pages(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    stash_bytes: &[u8; 32],
) -> Option<Vec<u32>> {
    // Try Staking.ClaimedRewards (newer runtimes)
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ClaimedRewards");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, *stash_bytes))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(pages) = Vec::<u32>::decode(&mut &raw_bytes[..]) {
            return Some(pages);
        }
    }

    // Try Staking.ErasClaimedRewards (Asset Hub)
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasClaimedRewards");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, *stash_bytes))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(pages) = Vec::<u32>::decode(&mut &raw_bytes[..]) {
            return Some(pages);
        }
    }

    None
}

/// Get page count for a validator at a specific era from ErasStakersOverview
async fn get_era_stakers_page_count(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    stash_bytes: &[u8; 32],
) -> Option<u32> {
    // Try ErasStakersOverview (paged staking)
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasStakersOverview");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, *stash_bytes))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(overview) = PagedExposureMetadata::decode(&mut &raw_bytes[..]) {
            return Some(overview.page_count);
        }
    }

    // If ErasStakersOverview doesn't exist, check ErasStakers (older format, always 1 page)
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasStakers");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, *stash_bytes))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(exposure) = Exposure::decode(&mut &raw_bytes[..])
            && exposure.total > 0
        {
            return Some(1); // Old format always has 1 page
        }
    }

    None
}
