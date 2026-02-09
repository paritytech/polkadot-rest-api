//! Staking pallet storage query functions.
//!
//! This module provides standalone functions for querying staking-related storage items.
//! Each function handles SCALE decoding and Option wrapper detection automatically.

use parity_scale_codec::Decode;
use sp_core::crypto::{AccountId32, Ss58Codec};
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// SCALE Decode Types
// ================================================================================================

/// Unlocking chunk in staking ledger (compact encoded)
#[derive(Debug, Clone, Decode)]
struct UnlockChunkCompact {
    #[codec(compact)]
    value: u128,
    #[codec(compact)]
    era: u32,
}

/// Unlocking chunk in staking ledger (non-compact, older runtimes)
#[derive(Debug, Clone, Decode)]
struct UnlockChunkNonCompact {
    value: u128,
    era: u32,
}

/// Staking ledger structure (modern runtime with legacy_claimed_rewards)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct StakingLedger {
    stash: [u8; 32],
    #[codec(compact)]
    total: u128,
    #[codec(compact)]
    active: u128,
    unlocking: Vec<UnlockChunkCompact>,
}

/// Staking ledger structure (legacy runtime with claimed_rewards, compact chunks)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct StakingLedgerLegacyCompact {
    stash: [u8; 32],
    #[codec(compact)]
    total: u128,
    #[codec(compact)]
    active: u128,
    unlocking: Vec<UnlockChunkCompact>,
    legacy_claimed_rewards: Vec<u32>,
}

/// Staking ledger structure (very old runtime, non-compact chunks)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct StakingLedgerOld {
    stash: [u8; 32],
    #[codec(compact)]
    total: u128,
    #[codec(compact)]
    active: u128,
    unlocking: Vec<UnlockChunkNonCompact>,
    claimed_rewards: Vec<u32>,
}

/// Minimal staking ledger - manual decoding as last resort
struct StakingLedgerMinimal {
    stash: [u8; 32],
    total: u128,
    active: u128,
}

impl StakingLedgerMinimal {
    fn decode_from_bytes(raw_bytes: &[u8]) -> Option<Self> {
        if raw_bytes.len() < 32 {
            return None;
        }
        let mut stash = [0u8; 32];
        stash.copy_from_slice(&raw_bytes[0..32]);
        let mut cursor = &raw_bytes[32..];
        let total = parity_scale_codec::Compact::<u128>::decode(&mut cursor)
            .ok()?
            .0;
        let active = parity_scale_codec::Compact::<u128>::decode(&mut cursor)
            .ok()?
            .0;
        Some(Self {
            stash,
            total,
            active,
        })
    }
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
#[allow(dead_code)]
struct SlashingSpans {
    span_index: u32,
    last_start: u32,
    last_nonzero_slash: u32,
    prior: Vec<u32>,
}

/// Era stakers overview (for paged staking)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
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
#[allow(dead_code)]
struct Exposure {
    #[codec(compact)]
    total: u128,
    #[codec(compact)]
    own: u128,
    others: Vec<ExposureIndividual>,
}

/// Individual exposure in era stakers
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct ExposureIndividual {
    who: [u8; 32],
    #[codec(compact)]
    value: u128,
}

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum StakingStorageError {
    #[error("The address is not a stash account")]
    NotAStashAccount,

    #[error("Staking ledger not found")]
    LedgerNotFound,

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),
}

// ================================================================================================
// Public Data Types
// ================================================================================================

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
#[derive(Debug, Clone, Decode)]
pub struct DecodedStakingLedger {
    /// Stash account address
    pub stash: String,
    /// Total locked balance (active + unlocking)
    pub total: String,
    /// Active staked balance
    pub active: String,
    /// Unlocking chunks
    pub unlocking: Vec<DecodedUnlockingChunk>,
}

/// Decoded unlocking chunk
#[derive(Debug, Clone, Decode)]
pub struct DecodedUnlockingChunk {
    /// Amount being unlocked
    pub value: String,
    /// Era when funds become available
    pub era: String,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================
/// Get the controller account for a stash from `Staking.Bonded` storage.
///
/// Returns `Ok(controller_ss58)` if the stash is bonded, `Err(NotAStashAccount)` otherwise.
pub async fn get_bonded_controller(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash: &AccountId32,
    ss58_prefix: u16,
) -> Result<String, StakingStorageError> {
    let stash_bytes: [u8; 32] = *stash.as_ref();
    let storage_addr = subxt::dynamic::storage::<_, [u8; 32]>("Staking", "Bonded");

    let value = client_at_block
        .storage()
        .fetch(storage_addr, (stash_bytes,))
        .await
        .map_err(|_| StakingStorageError::NotAStashAccount)?;
    let controller_raw = value.decode();

    if let Ok(controller_address) = controller_raw {
        if let Ok(decoded) = decode_account_id(&controller_address, ss58_prefix) {
            Ok(decoded)
        } else {
            Err(StakingStorageError::NotAStashAccount)
        }
    } else {
        Err(StakingStorageError::DecodeFailed(
            "Failed to decode controller address".into(),
        ))
    }
}

/// Get the staking ledger for an account.
///
/// The `lookup_key` is the account to use as the storage key (controller in legacy, stash in modern).
/// The `expected_stash` is used to validate the decoded ledger contains the correct stash.
///
/// Returns `Ok(DecodedStakingLedger)` if found, `Err(LedgerNotFound)` otherwise.
pub async fn get_staking_ledger(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    lookup_key: &AccountId32,
    ss58_prefix: u16,
) -> Result<DecodedStakingLedger, StakingStorageError> {
    let ledger_addr = subxt::dynamic::storage::<_, ()>("Staking", "Ledger");

    let lookup_bytes: [u8; 32] = *lookup_key.as_ref();

    if let Ok(value) = client_at_block
        .storage()
        .fetch(ledger_addr, (lookup_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(ledger) = StakingLedger::decode(&mut &raw_bytes[..]) {
            return Ok(DecodedStakingLedger {
                stash: AccountId32::from(ledger.stash)
                    .to_ss58check_with_version(ss58_prefix.into()),
                total: ledger.total.to_string(),
                active: ledger.active.to_string(),
                unlocking: ledger
                    .unlocking
                    .into_iter()
                    .map(|chunk| DecodedUnlockingChunk {
                        value: chunk.value.to_string(),
                        era: chunk.era.to_string(),
                    })
                    .collect(),
            });
        } else if let Ok(ledger) = StakingLedgerLegacyCompact::decode(&mut &raw_bytes[..]) {
            // TODO: Handle legacy_claimed_rewards if needed
        } else if let Ok(ledger) = StakingLedgerOld::decode(&mut &raw_bytes[..]) {
            // TODO: Handle claimed_rewards if needed
        }

        return Err(StakingStorageError::DecodeFailed(
            "Failed to decode staking ledger: unknown type".into()
        ));
    }

    Err(StakingStorageError::LedgerNotFound)
}

/// Get the reward destination (Payee) for a stash account.
///
/// Returns the reward destination, defaulting to "Staked" if not found.
pub async fn get_reward_destination(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash: &AccountId32,
    ss58_prefix: u16,
) -> DecodedRewardDestination {
    let stash_bytes: [u8; 32] = *stash.as_ref();
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "Payee");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (stash_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(dest) = decode_reward_destination(&raw_bytes, ss58_prefix) {
            return dest;
        }
    }

    DecodedRewardDestination::Simple("Staked".to_string())
}

/// Get nominations info for a stash account.
///
/// Returns `Some(DecodedNominationsInfo)` if the account is nominating, `None` otherwise.
pub async fn get_nominations(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash: &AccountId32,
    ss58_prefix: u16,
) -> Option<DecodedNominationsInfo> {
    let stash_bytes: [u8; 32] = *stash.as_ref();
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "Nominators");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (stash_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(Some(nominations)) = decode_nominations(&raw_bytes, ss58_prefix) {
            return Some(nominations);
        }
    }

    None
}

/// Get the number of slashing spans for a stash account.
///
/// Returns the count of slashing spans, or 0 if none found.
pub async fn get_slashing_spans_count(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash: &AccountId32,
) -> u32 {
    let stash_bytes: [u8; 32] = *stash.as_ref();
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "SlashingSpans");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (stash_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(count) = decode_slashing_spans(&raw_bytes) {
            return count;
        }
    }

    0
}

/// Check if an account is a validator.
///
/// Returns `true` if the account has validator preferences set.
pub async fn is_validator(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash: &AccountId32,
) -> bool {
    let stash_bytes: [u8; 32] = *stash.as_ref();
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "Validators");

    match client_at_block
        .storage()
        .fetch(storage_addr, (stash_bytes,))
        .await
    {
        Ok(value) => {
            // Check if the value is non-empty (account has validator prefs set)
            let raw_bytes = value.into_bytes();
            // ValidatorPrefs has at least commission (Perbill = u32), so non-empty means validator
            // Empty or default bytes mean not a validator
            !raw_bytes.is_empty() && raw_bytes != [0u8; 0]
        }
        Err(_) => false,
    }
}

/// Get the current era from `Staking.CurrentEra` storage.
///
/// Returns `Some(era)` if found, `None` otherwise.
pub async fn get_current_era(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<u32> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "CurrentEra");

    if let Ok(value) = client_at_block.storage().fetch(storage_addr, ()).await {
        let raw_bytes = value.into_bytes();
        // CurrentEra is Option<u32>, handle the Option wrapper
        if !raw_bytes.is_empty()
            && raw_bytes[0] == 1
            && raw_bytes.len() >= 5
            && let Ok(era) = u32::decode(&mut &raw_bytes[1..])
        {
            return Some(era);
        }
        // Try direct u32 decode (some runtimes may not wrap in Option)
        if let Ok(era) = u32::decode(&mut &raw_bytes[..]) {
            return Some(era);
        }
    }

    None
}

/// Get history depth from `Staking.HistoryDepth` storage.
///
/// Returns the history depth, defaulting to 84 if not found.
pub async fn get_history_depth(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> u32 {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "HistoryDepth");

    if let Ok(value) = client_at_block.storage().fetch(storage_addr, ()).await {
        let raw_bytes = value.into_bytes();
        if let Ok(depth) = u32::decode(&mut &raw_bytes[..]) {
            return depth;
        }
    }

    84
}

/// Get claimed page indices for a validator at a specific era.
///
/// Returns `Some(Vec<u32>)` with claimed page indices, or `None` if status cannot be determined.
/// An empty vec means unclaimed, non-empty means some pages claimed.
pub async fn get_claimed_pages(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator: &AccountId32,
) -> Option<Vec<u32>> {
    let stash_bytes: [u8; 32] = *validator.as_ref();

    // Try Staking.ClaimedRewards (modern paged staking)
    let claimed_rewards_storage = subxt::dynamic::storage::<_, ()>("Staking", "ClaimedRewards");
    let claimed_rewards_result = client_at_block
        .storage()
        .fetch(claimed_rewards_storage, (era, stash_bytes))
        .await;

    match claimed_rewards_result {
        Ok(value) => {
            let raw_bytes = value.into_bytes();
            if let Ok(pages) = Vec::<u32>::decode(&mut &raw_bytes[..]) {
                return Some(pages);
            }
            if let Ok(pages) = Vec::<parity_scale_codec::Compact<u32>>::decode(&mut &raw_bytes[..])
            {
                return Some(pages.into_iter().map(|c| c.0).collect());
            }
            if !raw_bytes.is_empty() {
                return Some(vec![0]);
            }
            return Some(vec![]);
        }
        Err(e) => {
            let err_str = format!("{:?}", e);
            if !err_str.contains("Metadata") && !err_str.contains("Pallet") {
                return Some(vec![]);
            }
        }
    }

    // Try Staking.ErasClaimedRewards (alternative name)
    let eras_claimed_storage = subxt::dynamic::storage::<_, ()>("Staking", "ErasClaimedRewards");
    let eras_claimed_result = client_at_block
        .storage()
        .fetch(eras_claimed_storage, (era, stash_bytes))
        .await;

    match eras_claimed_result {
        Ok(value) => {
            let raw_bytes = value.into_bytes();
            if let Ok(pages) = Vec::<u32>::decode(&mut &raw_bytes[..]) {
                return Some(pages);
            }
            if let Ok(pages) = Vec::<parity_scale_codec::Compact<u32>>::decode(&mut &raw_bytes[..])
            {
                return Some(pages.into_iter().map(|c| c.0).collect());
            }
            if !raw_bytes.is_empty() {
                return Some(vec![0]);
            }
            return Some(vec![]);
        }
        Err(e) => {
            let err_str = format!("{:?}", e);
            if !err_str.contains("Metadata") && !err_str.contains("Pallet") {
                return Some(vec![]);
            }
        }
    }

    // Fall back to ledger's legacy_claimed_rewards
    if let Some(claimed) = check_ledger_claimed_rewards(client_at_block, &stash_bytes, era).await {
        return Some(if claimed { vec![0] } else { vec![] });
    }

    None
}

/// Get page count for a validator at a specific era.
///
/// Returns `Some(page_count)` if the validator was active in that era, `None` otherwise.
pub async fn get_era_stakers_page_count(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator: &AccountId32,
) -> Option<u32> {
    let stash_bytes: [u8; 32] = *validator.as_ref();

    // Try ErasStakersOverview (paged staking)
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasStakersOverview");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, stash_bytes))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(overview) = PagedExposureMetadata::decode(&mut &raw_bytes[..]) {
            return Some(overview.page_count);
        }
    }

    // Try ErasStakers (older format, always 1 page)
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasStakers");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, stash_bytes))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(exposure) = Exposure::decode(&mut &raw_bytes[..])
            && exposure.total > 0
        {
            return Some(1);
        }
    }

    None
}
// ================================================================================================
// Internal Helper Functions
// ================================================================================================

/// Check if an era is claimed in the ledger's legacy_claimed_rewards field
async fn check_ledger_claimed_rewards(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash_bytes: &[u8; 32],
    era: u32,
) -> Option<bool> {
    let bonded_addr = subxt::dynamic::storage::<_, ()>("Staking", "Bonded");
    let controller_bytes = if let Ok(value) = client_at_block
        .storage()
        .fetch(bonded_addr, (*stash_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        <[u8; 32]>::decode(&mut &raw_bytes[..]).ok()?
    } else {
        *stash_bytes
    };

    let ledger_addr = subxt::dynamic::storage::<_, ()>("Staking", "Ledger");

    // Try stash first
    if let Ok(value) = client_at_block
        .storage()
        .fetch(ledger_addr.clone(), (*stash_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Some(is_claimed) = decode_ledger_claimed_eras(&raw_bytes, era) {
            return Some(is_claimed);
        }
    }

    // Try controller
    if let Ok(value) = client_at_block
        .storage()
        .fetch(ledger_addr, (controller_bytes,))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Some(is_claimed) = decode_ledger_claimed_eras(&raw_bytes, era) {
            return Some(is_claimed);
        }
    }

    None
}

// ================================================================================================
// Decoding Functions
// ================================================================================================

fn decode_account_id(raw_bytes: &[u8], ss58_prefix: u16) -> Result<String, StakingStorageError> {
    if let Ok(account_bytes) = <[u8; 32]>::decode(&mut &raw_bytes[..]) {
        let account_id = AccountId32::from(account_bytes);
        return Ok(account_id.to_ss58check_with_version(ss58_prefix.into()));
    }

    Err(StakingStorageError::DecodeFailed(
        parity_scale_codec::Error::from("Failed to decode account id"),
    ))
}

fn decode_reward_destination(
    raw_bytes: &[u8],
    ss58_prefix: u16,
) -> Result<DecodedRewardDestination, StakingStorageError> {
    if let Ok(dest) = RewardDestinationType::decode(&mut &raw_bytes[..]) {
        return Ok(match dest {
            RewardDestinationType::Staked => DecodedRewardDestination::Simple("Staked".to_string()),
            RewardDestinationType::Stash => DecodedRewardDestination::Simple("Stash".to_string()),
            RewardDestinationType::Controller => {
                DecodedRewardDestination::Simple("Controller".to_string())
            }
            RewardDestinationType::None => DecodedRewardDestination::Simple("None".to_string()),
            RewardDestinationType::Account(account_bytes) => {
                let account =
                    AccountId32::from(account_bytes).to_ss58check_with_version(ss58_prefix.into());
                DecodedRewardDestination::Account { account }
            }
        });
    }

    Ok(DecodedRewardDestination::Simple("Staked".to_string()))
}

fn decode_nominations(
    raw_bytes: &[u8],
    ss58_prefix: u16,
) -> Result<Option<DecodedNominationsInfo>, StakingStorageError> {
    if let Ok(nominations) = Nominations::decode(&mut &raw_bytes[..]) {
        let targets = nominations
            .targets
            .into_iter()
            .map(|bytes| AccountId32::from(bytes).to_ss58check_with_version(ss58_prefix.into()))
            .collect();

        return Ok(Some(DecodedNominationsInfo {
            targets,
            submitted_in: nominations.submitted_in.to_string(),
            suppressed: nominations.suppressed,
        }));
    }

    Ok(None)
}

fn decode_slashing_spans(raw_bytes: &[u8]) -> Result<u32, StakingStorageError> {
    if let Ok(spans) = SlashingSpans::decode(&mut &raw_bytes[..]) {
        return Ok(spans.prior.len() as u32 + 1);
    }

    Ok(0)
}

fn decode_ledger_claimed_eras(raw_bytes: &[u8], era: u32) -> Option<bool> {
    // Try decoding directly (no Option wrapper)
    // if let Ok(ledger) = StakingLedgerModern::decode(&mut &raw_bytes[..]) {
    //     return Some(ledger.legacy_claimed_rewards.contains(&era));
    // }
    // if let Ok(ledger) = StakingLedgerLegacyCompact::decode(&mut &raw_bytes[..]) {
    //     return Some(ledger.claimed_rewards.contains(&era));
    // }
    if let Ok(ledger) = StakingLedgerOld::decode(&mut &raw_bytes[..]) {
        return Some(ledger.claimed_rewards.contains(&era));
    }

    // Try with Option<T> wrapper stripped (Some = 0x01 prefix)
    if raw_bytes.len() > 1 && raw_bytes[0] == 1 {
        let bytes_without_option = &raw_bytes[1..];

        // if let Ok(ledger) = StakingLedgerModern::decode(&mut &bytes_without_option[..]) {
        //     return Some(ledger.legacy_claimed_rewards.contains(&era));
        // }
        // if let Ok(ledger) = StakingLedgerLegacyCompact::decode(&mut &bytes_without_option[..]) {
        //     return Some(ledger.claimed_rewards.contains(&era));
        // }
        if let Ok(ledger) = StakingLedgerOld::decode(&mut &bytes_without_option[..]) {
            return Some(ledger.claimed_rewards.contains(&era));
        }
    }

    None
}
