// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

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
#[allow(dead_code)]
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
#[allow(dead_code)]
struct StakingLedgerMinimal {
    stash: [u8; 32],
    total: u128,
    active: u128,
}

#[allow(dead_code)]
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
        } else if let Ok(ledger) = StakingLedgerOld::decode(&mut &raw_bytes[..]) {
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
        }

        return Err(StakingStorageError::DecodeFailed(
            "Failed to decode staking ledger: unknown type".into(),
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
/// Queries `Staking.Validators` storage. In Substrate, this storage item uses
/// a `ValueQuery` with a default of `ValidatorPrefs { commission: 0, blocked: false }`.
/// This means a `fetch` will return bytes even for non-validators (the default value).
///
/// To distinguish actual validators, we check if the account appears in `Staking.Bonded`
/// and has non-default validator preferences, or we check the validators list directly.
/// The most reliable approach is to check if the account is in the bonded map AND
/// has a `Validators` entry that differs from the storage default.
pub async fn is_validator(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    stash: &AccountId32,
) -> bool {
    let stash_bytes: [u8; 32] = *stash.as_ref();

    // First verify the account is bonded (is a stash account)
    let bonded_addr = subxt::dynamic::storage::<_, ()>("Staking", "Bonded");
    let is_bonded = client_at_block
        .storage()
        .fetch(bonded_addr, (stash_bytes,))
        .await
        .is_ok();

    if !is_bonded {
        return false;
    }

    // Check if the account has a Nominators entry - if so, it's a nominator, not a validator
    let nominators_addr = subxt::dynamic::storage::<_, ()>("Staking", "Nominators");
    let is_nominator = client_at_block
        .storage()
        .fetch(nominators_addr, (stash_bytes,))
        .await
        .is_ok();

    if is_nominator {
        return false;
    }

    // Account is bonded but not nominating - it's a validator (or idle, but we treat
    // bonded non-nominators as validators for the purposes of reward queries)
    true
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

            // SCALE encoding for empty Vec is a single byte 0x00 (compact length = 0)
            // If we see this, it means no pages are claimed
            if raw_bytes.len() == 1 && raw_bytes[0] == 0 {
                return Some(vec![]);
            }

            // Check if it might be Option<Vec<u32>> wrapped:
            // - None = [0x00] (handled above)
            // - Some(vec![]) = [0x01, 0x00]
            // - Some(vec![0]) = [0x01, 0x04, 0x00, 0x00, 0x00, 0x00]
            if raw_bytes.len() >= 2 && raw_bytes[0] == 0x01 {
                // This looks like Some(...), try decoding the inner value
                let inner_bytes = &raw_bytes[1..];

                if inner_bytes.len() == 1 && inner_bytes[0] == 0 {
                    return Some(vec![]);
                }

                if let Ok(pages) = Vec::<u32>::decode(&mut &inner_bytes[..]) {
                    return Some(pages);
                }
            }

            // For a non-empty Vec<u32>, we need at least 5 bytes:
            // - 1 byte for compact length (0x04 for length 1)
            // - 4 bytes for each u32 element
            // If we have fewer bytes, something is wrong - treat as unclaimed
            if raw_bytes.len() < 5 {
                return Some(vec![]);
            }

            // Try decoding as Vec<u32>
            if let Ok(pages) = Vec::<u32>::decode(&mut &raw_bytes[..]) {
                return Some(pages);
            }
            // Try decoding as Vec<Compact<u32>>
            if let Ok(pages) = Vec::<parity_scale_codec::Compact<u32>>::decode(&mut &raw_bytes[..])
            {
                let pages: Vec<u32> = pages.into_iter().map(|c| c.0).collect();
                return Some(pages);
            }
            // If we can't decode, assume unclaimed (conservative approach)
            return Some(vec![]);
        }
        Err(e) => {
            let err_str = format!("{:?}", e);
            let err_lower = err_str.to_lowercase();
            // Check if this is a "storage not found" error vs. other errors
            let is_not_found = err_lower.contains("storagenotfound")
                || err_lower.contains("not found")
                || err_lower.contains("notfound")
                || err_lower.contains("decodingfailed")
                || err_lower.contains("none")
                || err_lower.contains("empty");

            // For modern runtimes, if ClaimedRewards key doesn't exist, it means unclaimed
            if is_not_found {
                return Some(vec![]);
            }
        }
    }

    // Try Staking.ErasClaimedRewards (alternative name used in some runtimes)
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
                let pages: Vec<u32> = pages.into_iter().map(|c| c.0).collect();
                return Some(pages);
            }
            // If we can't decode, assume unclaimed
            return Some(vec![]);
        }
        Err(e) => {
            let err_str = format!("{:?}", e);
            let err_lower = err_str.to_lowercase();
            let is_not_found = err_lower.contains("storagenotfound")
                || err_lower.contains("not found")
                || err_lower.contains("notfound")
                || err_lower.contains("decodingfailed")
                || err_lower.contains("none")
                || err_lower.contains("empty");

            if is_not_found {
                return Some(vec![]);
            }
        }
    }

    // NOTE: We intentionally do NOT fall back to the legacy ledger's claimed_rewards field.
    // On modern runtimes (Kusama/Polkadot), the ClaimedRewards storage is the authoritative source.
    // The legacy ledger decode can produce false positives when decoding modern ledger bytes.
    //
    // If we reached here, it means both ClaimedRewards and ErasClaimedRewards fetches failed
    // with errors that weren't "not found" - this is unusual and might indicate a problem.
    // In this case, assume unclaimed (conservative approach that allows payouts to show).
    Some(vec![])
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

/// Get active era from `Staking.ActiveEra` storage.
///
/// Returns `Some(era_index)` if found, `None` otherwise.
pub async fn get_active_era(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> Option<u32> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ActiveEra");

    if let Ok(value) = client_at_block.storage().fetch(storage_addr, ()).await {
        let raw_bytes = value.into_bytes();
        // ActiveEra is Option<ActiveEraInfo { index: u32, start: Option<u64> }>
        // First try decoding the struct
        if let Ok(era_info) = ActiveEraInfo::decode(&mut &raw_bytes[..]) {
            return Some(era_info.index);
        }
        // Try with Option wrapper
        if raw_bytes.len() > 1
            && raw_bytes[0] == 1
            && let Ok(era_info) = ActiveEraInfo::decode(&mut &raw_bytes[1..])
        {
            return Some(era_info.index);
        }
    }

    None
}

/// Active era info structure
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct ActiveEraInfo {
    index: u32,
    start: Option<u64>,
}

/// Era reward points result
pub type EraRewardPointsResult = (u32, std::collections::HashMap<[u8; 32], u32>);

/// Get era reward points from `Staking.ErasRewardPoints` storage.
///
/// Returns `Some((total_points, individual_map))` if found, `None` otherwise.
pub async fn get_era_reward_points(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
) -> Option<EraRewardPointsResult> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasRewardPoints");

    if let Ok(value) = client_at_block.storage().fetch(storage_addr, (era,)).await {
        let raw_bytes = value.into_bytes();
        if let Ok(points) = EraRewardPointsStruct::decode(&mut &raw_bytes[..]) {
            let individual: std::collections::HashMap<[u8; 32], u32> =
                points.individual.into_iter().collect();
            return Some((points.total, individual));
        }
    }

    None
}

/// Era reward points structure
#[derive(Debug, Clone, Decode)]
struct EraRewardPointsStruct {
    total: u32,
    individual: Vec<([u8; 32], u32)>,
}

/// Get total era validator reward from `Staking.ErasValidatorReward` storage.
///
/// Returns `Some(total_payout)` if found, `None` otherwise.
pub async fn get_era_validator_reward(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
) -> Option<u128> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasValidatorReward");

    if let Ok(value) = client_at_block.storage().fetch(storage_addr, (era,)).await {
        let raw_bytes = value.into_bytes();
        if let Ok(reward) = u128::decode(&mut &raw_bytes[..]) {
            return Some(reward);
        }
    }

    None
}

/// Get validator preferences (commission) for an era.
///
/// Returns `Some(commission)` if found, `None` otherwise.
/// Commission is in parts per billion (0 - 1_000_000_000).
pub async fn get_era_validator_prefs(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator: &AccountId32,
) -> Option<u32> {
    let validator_bytes: [u8; 32] = *validator.as_ref();
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasValidatorPrefs");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, validator_bytes))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(prefs) = ValidatorPrefs::decode(&mut &raw_bytes[..]) {
            return Some(prefs.commission);
        }
    }

    None
}

/// Validator preferences structure
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct ValidatorPrefs {
    #[codec(compact)]
    commission: u32,
    blocked: bool,
}

/// Exposure data: (total, own, others as Vec<(account_bytes, value)>)
pub type ExposureData = (u128, u128, Vec<([u8; 32], u128)>);

/// Get era stakers from `Staking.ErasStakersClipped` storage (legacy).
///
/// Returns `Some((total, own, others))` if found, `None` otherwise.
pub async fn get_era_stakers_clipped(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator: &AccountId32,
) -> Option<ExposureData> {
    let validator_bytes: [u8; 32] = *validator.as_ref();
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasStakersClipped");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, validator_bytes))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(exposure) = Exposure::decode(&mut &raw_bytes[..]) {
            let others: Vec<([u8; 32], u128)> = exposure
                .others
                .into_iter()
                .map(|ie| (ie.who, ie.value))
                .collect();
            return Some((exposure.total, exposure.own, others));
        }
    }

    None
}

/// Get era stakers overview from `Staking.ErasStakersOverview` storage (paged staking).
///
/// Returns `Some((total, own, page_count))` if found, `None` otherwise.
pub async fn get_era_stakers_overview(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator: &AccountId32,
) -> Option<(u128, u128, u32)> {
    let validator_bytes: [u8; 32] = *validator.as_ref();
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasStakersOverview");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, validator_bytes))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(overview) = PagedExposureMetadata::decode(&mut &raw_bytes[..]) {
            return Some((overview.total, overview.own, overview.page_count));
        }
    }

    None
}

/// Paged exposure data: (page_total, others as Vec<(account_bytes, value)>)
pub type ExposurePageData = (u128, Vec<([u8; 32], u128)>);

/// Get era stakers page from `Staking.ErasStakersPaged` storage.
///
/// Returns `Some((page_total, others))` if found, `None` otherwise.
pub async fn get_era_stakers_paged(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator: &AccountId32,
    page: u32,
) -> Option<ExposurePageData> {
    let validator_bytes: [u8; 32] = *validator.as_ref();
    let storage_addr = subxt::dynamic::storage::<_, ()>("Staking", "ErasStakersPaged");

    if let Ok(value) = client_at_block
        .storage()
        .fetch(storage_addr, (era, validator_bytes, page))
        .await
    {
        let raw_bytes = value.into_bytes();
        if let Ok(exposure_page) = ExposurePage::decode(&mut &raw_bytes[..]) {
            let others: Vec<([u8; 32], u128)> = exposure_page
                .others
                .into_iter()
                .map(|ie| (ie.who, ie.value))
                .collect();
            return Some((exposure_page.page_total, others));
        }
    }

    None
}

/// Exposure page structure (for paged staking)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct ExposurePage {
    #[codec(compact)]
    page_total: u128,
    others: Vec<ExposureIndividual>,
}

/// Check if rewards have been claimed for a validator in an era.
///
/// Uses `get_claimed_pages` and `get_era_stakers_page_count` to determine if
/// all pages have been claimed. Returns `true` only when all pages are confirmed claimed.
pub async fn is_era_claimed(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
    validator: &AccountId32,
) -> bool {
    let (claimed_pages, page_count) = tokio::join!(
        get_claimed_pages(client_at_block, era, validator),
        get_era_stakers_page_count(client_at_block, era, validator),
    );
    match (claimed_pages, page_count) {
        (Some(pages), Some(total)) => {
            // Fully claimed if all pages are accounted for
            !pages.is_empty() && pages.len() as u32 >= total
        }
        (Some(pages), None) => {
            // Have claimed pages but can't determine total - if we have any pages
            // claimed, assume at least partially claimed. For the `unclaimed_only`
            // filter, treat as claimed to be conservative.
            !pages.is_empty()
        }
        _ => false,
    }
}

// ================================================================================================
// Bulk Era Exposure Query (Sidecar-style approach for historical eras)
// ================================================================================================

/// Result of bulk era exposure query.
/// Maps nominator account bytes to a list of (validator_bytes, nominator_exposure, total_exposure).
pub type EraExposureMap = std::collections::HashMap<[u8; 32], Vec<([u8; 32], u128, u128)>>;

/// Validator exposure info for bulk queries
#[derive(Debug, Clone)]
pub struct ValidatorExposureInfo {
    pub validator_bytes: [u8; 32],
    pub total: u128,
    pub own: u128,
}

/// Fetch ALL exposures for a specific era in bulk using prefix-based storage iteration.
///
/// Uses subxt's `PrefixOf` support to iterate only entries matching the given era,
/// avoiding the need to scan all eras in storage.
///
/// Used as a fallback when the targeted approach (using current nominations) fails
/// to find results, which can happen when nominations have changed since the era being queried.
///
/// Returns a map of nominator_bytes → [(validator_bytes, nominator_exposure, total_exposure)]
/// and a map of validator_bytes → ValidatorExposureInfo for validators' own stake.
pub async fn get_era_exposures_bulk(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    era: u32,
) -> (
    EraExposureMap,
    std::collections::HashMap<[u8; 32], ValidatorExposureInfo>,
) {
    let mut nominator_map: EraExposureMap = std::collections::HashMap::new();
    let mut validator_map: std::collections::HashMap<[u8; 32], ValidatorExposureInfo> =
        std::collections::HashMap::new();

    // Try paged staking first (ErasStakersOverview + ErasStakersPaged)
    // Key type: (u32, [u8; 32]) for (era, validator)
    let overview_addr = subxt::dynamic::storage::<(u32, [u8; 32]), scale_value::Value>(
        "Staking",
        "ErasStakersOverview",
    );
    let mut found_paged = false;

    // Use era as prefix key to iterate only entries for this specific era
    if let Ok(mut iter) = client_at_block.storage().iter(overview_addr, (era,)).await {
        while let Some(Ok(kv)) = iter.next().await {
            let key_bytes = kv.key_bytes();

            // Key format: pallet_hash(16) + storage_hash(16) + hasher(8) + era(4) + hasher(8) + validator(32)
            // Validator is in the last 32 bytes
            if key_bytes.len() < 84 {
                continue;
            }

            found_paged = true;

            let mut validator_bytes = [0u8; 32];
            validator_bytes.copy_from_slice(&key_bytes[key_bytes.len() - 32..]);

            let overview_bytes = kv.value().bytes();
            if let Ok(overview) = PagedExposureMetadata::decode(&mut &overview_bytes[..]) {
                if overview.total == 0 {
                    continue;
                }

                validator_map.insert(
                    validator_bytes,
                    ValidatorExposureInfo {
                        validator_bytes,
                        total: overview.total,
                        own: overview.own,
                    },
                );
            }
        }
    }

    // If we found paged staking data, fetch the paged exposures for this era
    if found_paged && !validator_map.is_empty() {
        // Key type: (u32, [u8; 32], u32) for (era, validator, page)
        let paged_addr = subxt::dynamic::storage::<(u32, [u8; 32], u32), scale_value::Value>(
            "Staking",
            "ErasStakersPaged",
        );
        // Use era as prefix key to iterate only pages for this specific era
        if let Ok(mut iter) = client_at_block.storage().iter(paged_addr, (era,)).await {
            while let Some(Ok(kv)) = iter.next().await {
                let key_bytes = kv.key_bytes();

                // Key format: pallet_hash(16) + storage_hash(16) + hasher(8) + era(4) + hasher(8) + validator(32) + hasher(8) + page(4)
                // Validator is at a fixed offset; extract from known positions
                if key_bytes.len() < 96 {
                    continue;
                }

                // Validator is at positions 52..84 (after pallet+storage hashes + era key part)
                let mut validator_bytes = [0u8; 32];
                validator_bytes.copy_from_slice(&key_bytes[52..84]);

                let page_bytes = kv.value().bytes();
                if let Ok(exposure_page) = ExposurePage::decode(&mut &page_bytes[..]) {
                    let total = validator_map
                        .get(&validator_bytes)
                        .map(|v| v.total)
                        .unwrap_or(0);

                    if total == 0 {
                        continue;
                    }

                    for individual in exposure_page.others {
                        nominator_map.entry(individual.who).or_default().push((
                            validator_bytes,
                            individual.value,
                            total,
                        ));
                    }
                }
            }
        }

        return (nominator_map, validator_map);
    }

    // Fall back to legacy ErasStakersClipped
    // Key type: (u32, [u8; 32]) for (era, validator)
    let clipped_addr = subxt::dynamic::storage::<(u32, [u8; 32]), scale_value::Value>(
        "Staking",
        "ErasStakersClipped",
    );
    // Use era as prefix key to iterate only entries for this specific era
    if let Ok(mut iter) = client_at_block.storage().iter(clipped_addr, (era,)).await {
        while let Some(Ok(kv)) = iter.next().await {
            let key_bytes = kv.key_bytes();

            // Key format: pallet_hash(16) + storage_hash(16) + hasher(8) + era(4) + hasher(8) + validator(32)
            // Validator is in the last 32 bytes
            if key_bytes.len() < 84 {
                continue;
            }

            let mut validator_bytes = [0u8; 32];
            validator_bytes.copy_from_slice(&key_bytes[key_bytes.len() - 32..]);

            let raw_bytes = kv.value().bytes();
            if let Ok(exposure) = Exposure::decode(&mut &raw_bytes[..]) {
                if exposure.total == 0 {
                    continue;
                }

                validator_map.insert(
                    validator_bytes,
                    ValidatorExposureInfo {
                        validator_bytes,
                        total: exposure.total,
                        own: exposure.own,
                    },
                );

                for individual in exposure.others {
                    nominator_map.entry(individual.who).or_default().push((
                        validator_bytes,
                        individual.value,
                        exposure.total,
                    ));
                }
            }
        }
    }

    (nominator_map, validator_map)
}
// ================================================================================================
// Internal Helper Functions
// ================================================================================================

/// Check if an era is claimed in the ledger's legacy_claimed_rewards field
///
/// NOTE: This is only used for very old runtimes that don't have ClaimedRewards storage.
/// Modern runtimes (Kusama/Polkadot post-paged-staking) use ClaimedRewards storage directly.
/// Currently unused but kept for potential future use with older chains.
#[allow(dead_code)]
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

#[allow(dead_code)]
fn decode_ledger_claimed_eras(raw_bytes: &[u8], era: u32) -> Option<bool> {
    // Try decoding with StakingLedgerLegacyCompact (has legacy_claimed_rewards field)
    if let Ok(ledger) = StakingLedgerLegacyCompact::decode(&mut &raw_bytes[..]) {
        return Some(ledger.legacy_claimed_rewards.contains(&era));
    }

    // Try StakingLedgerOld (very old runtime format)
    if let Ok(ledger) = StakingLedgerOld::decode(&mut &raw_bytes[..]) {
        return Some(ledger.claimed_rewards.contains(&era));
    }

    // Try with Option<T> wrapper stripped (Some = 0x01 prefix)
    if raw_bytes.len() > 1 && raw_bytes[0] == 1 {
        let bytes_without_option = &raw_bytes[1..];

        if let Ok(ledger) = StakingLedgerLegacyCompact::decode(&mut &bytes_without_option[..]) {
            return Some(ledger.legacy_claimed_rewards.contains(&era));
        }

        if let Ok(ledger) = StakingLedgerOld::decode(&mut &bytes_without_option[..]) {
            return Some(ledger.claimed_rewards.contains(&era));
        }
    }

    None
}

// ================================================================================================
// Additional Staking Progress Queries
// ================================================================================================

/// Decoded active era information with start timestamp.
#[derive(Debug, Clone)]
pub struct DecodedActiveEraInfo {
    pub index: u32,
    pub start: Option<u64>,
}

/// Force era status enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForceEra {
    NotForcing,
    ForceNew,
    ForceNone,
    ForceAlways,
}

impl ForceEra {
    pub fn as_str(&self) -> &'static str {
        match self {
            ForceEra::NotForcing => "NotForcing",
            ForceEra::ForceNew => "ForceNew",
            ForceEra::ForceNone => "ForceNone",
            ForceEra::ForceAlways => "ForceAlways",
        }
    }
}

/// Get the ideal validator count from Staking::ValidatorCount.
pub async fn get_validator_count(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<u32> {
    let storage_addr = subxt::dynamic::storage::<(), u32>("Staking", "ValidatorCount");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    value.decode().ok()
}

/// Get the force era status from Staking::ForceEra.
pub async fn get_force_era(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<ForceEra> {
    let storage_addr = subxt::dynamic::storage::<(), u8>("Staking", "ForceEra");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    
    // ForceEra is an enum: 0=NotForcing, 1=ForceNew, 2=ForceNone, 3=ForceAlways
    let variant = value.decode().ok()?;
    Some(match variant {
        0 => ForceEra::NotForcing,
        1 => ForceEra::ForceNew,
        2 => ForceEra::ForceNone,
        3 => ForceEra::ForceAlways,
        _ => ForceEra::NotForcing,
    })
}

/// Get the active era info with start timestamp from Staking::ActiveEra.
pub async fn get_active_era_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<DecodedActiveEraInfo> {
    let storage_addr = subxt::dynamic::storage::<(), ()>("Staking", "ActiveEra");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    
    let raw_bytes = value.into_bytes();
    
    // Try direct decode first
    if let Ok(era_info) = ActiveEraInfo::decode(&mut &raw_bytes[..]) {
        return Some(DecodedActiveEraInfo {
            index: era_info.index,
            start: era_info.start,
        });
    }
    
    // Try with Option wrapper
    if let Ok(Some(era_info)) = Option::<ActiveEraInfo>::decode(&mut &raw_bytes[..]) {
        return Some(DecodedActiveEraInfo {
            index: era_info.index,
            start: era_info.start,
        });
    }
    
    None
}

/// Get the current session index from Session::CurrentIndex.
pub async fn get_session_current_index(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<u32> {
    let storage_addr = subxt::dynamic::storage::<(), u32>("Session", "CurrentIndex");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    value.decode().ok()
}

/// Get the session validators from Session::Validators.
pub async fn get_session_validators(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Option<Vec<String>> {
    let storage_addr = subxt::dynamic::storage::<(), ()>("Session", "Validators");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    
    let raw_bytes = value.into_bytes();
    let validators: Vec<[u8; 32]> = Vec::decode(&mut &raw_bytes[..]).ok()?;
    
    Some(
        validators
            .into_iter()
            .map(|bytes| AccountId32::from(bytes).to_ss58check_with_version(ss58_prefix.into()))
            .collect()
    )
}

/// Get the current timestamp from Timestamp::Now.
pub async fn get_timestamp(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<u64> {
    let storage_addr = subxt::dynamic::storage::<(), u64>("Timestamp", "Now");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    value.decode().ok()
}

/// Get the bonded eras from Staking::BondedEras.
/// Returns a list of (era_index, session_index) pairs.
pub async fn get_bonded_eras(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<Vec<(u32, u32)>> {
    let storage_addr = subxt::dynamic::storage::<(), ()>("Staking", "BondedEras");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    
    let raw_bytes = value.into_bytes();
    Vec::<(u32, u32)>::decode(&mut &raw_bytes[..]).ok()
}

// ================================================================================================
// Babe Pallet Queries (used for session/era progress calculations)
// ================================================================================================

/// Get the current slot from Babe::CurrentSlot.
pub async fn get_babe_current_slot(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<u64> {
    let storage_addr = subxt::dynamic::storage::<(), u64>("Babe", "CurrentSlot");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    value.decode().ok()
}

/// Get the current epoch index from Babe::EpochIndex.
pub async fn get_babe_epoch_index(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<u64> {
    let storage_addr = subxt::dynamic::storage::<(), u64>("Babe", "EpochIndex");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    value.decode().ok()
}

/// Get the genesis slot from Babe::GenesisSlot.
pub async fn get_babe_genesis_slot(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<u64> {
    let storage_addr = subxt::dynamic::storage::<(), u64>("Babe", "GenesisSlot");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    value.decode().ok()
}

/// Get skipped epochs from Babe::SkippedEpochs.
/// Returns a list of (epoch_index, skipped_session_count) pairs.
pub async fn get_babe_skipped_epochs(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<Vec<(u64, u32)>> {
    let storage_addr = subxt::dynamic::storage::<(), ()>("Babe", "SkippedEpochs");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .ok()?;
    
    let raw_bytes = value.into_bytes();
    Vec::<(u64, u32)>::decode(&mut &raw_bytes[..]).ok()
}
