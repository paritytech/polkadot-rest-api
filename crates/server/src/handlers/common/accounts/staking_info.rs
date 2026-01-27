//! Common staking info utilities shared across handler modules.

use crate::utils::ResolvedBlock;
use scale_value::{Composite, Value, ValueDef};
use sp_core::crypto::{AccountId32, Ss58Codec};
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

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
    let bonded_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Bonded");

    // Check if Staking pallet exists
    let staking_exists = client_at_block
        .storage()
        .entry(bonded_query.clone())
        .is_ok();

    if !staking_exists {
        return Err(StakingQueryError::StakingPalletNotAvailable);
    }

    let account_bytes: [u8; 32] = *account.as_ref();

    // Query Staking.Bonded to get controller from stash
    let bonded_entry = client_at_block.storage().entry(bonded_query)?;
    let key = vec![Value::from_bytes(account_bytes)];
    let bonded_value = bonded_entry.try_fetch(key).await?;

    let controller = if let Some(value) = bonded_value {
        decode_account_id(&value).await?
    } else {
        // Address is not a stash account
        return Err(StakingQueryError::NotAStashAccount);
    };

    let controller_account = AccountId32::from_ss58check(&controller)
        .map_err(|_| StakingQueryError::InvalidAddress(controller.clone()))?;
    let controller_bytes: [u8; 32] = *controller_account.as_ref();

    // Query Staking.Ledger to get staking ledger
    let ledger_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Ledger");
    let ledger_entry = client_at_block.storage().entry(ledger_query)?;
    let key = vec![Value::from_bytes(controller_bytes)];
    let ledger_value = ledger_entry.try_fetch(key).await?;

    let staking = if let Some(value) = ledger_value {
        decode_staking_ledger(&value).await?
    } else {
        return Err(StakingQueryError::LedgerNotFound);
    };

    // Query Staking.Payee to get reward destination
    let payee_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Payee");
    let payee_entry = client_at_block.storage().entry(payee_query)?;
    let key = vec![Value::from_bytes(account_bytes)];
    let payee_value = payee_entry.try_fetch(key).await?;

    let reward_destination = if let Some(value) = payee_value {
        decode_reward_destination(&value).await?
    } else {
        DecodedRewardDestination::Simple("Staked".to_string())
    };

    // Query Staking.Nominators to get nominations
    let nominators_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "Nominators",
    );
    let nominators_entry = client_at_block.storage().entry(nominators_query)?;
    let key = vec![Value::from_bytes(account_bytes)];
    let nominators_value = nominators_entry.try_fetch(key).await?;

    let nominations = if let Some(value) = nominators_value {
        decode_nominations(&value).await?
    } else {
        None
    };

    // Query Staking.SlashingSpans to get number of slashing spans
    let slashing_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "SlashingSpans",
    );
    let num_slashing_spans =
        if let Ok(slashing_entry) = client_at_block.storage().entry(slashing_query) {
            let key = vec![Value::from_bytes(account_bytes)];
            if let Ok(Some(value)) = slashing_entry.try_fetch(key).await {
                decode_slashing_spans(&value).await.unwrap_or(0)
            } else {
                0
            }
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

/// Decode an AccountId from a storage value
async fn decode_account_id(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<String, StakingQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode account id",
        ))
    })?;

    extract_account_id_from_value(&decoded).ok_or_else(|| {
        StakingQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to extract account id",
        ))
    })
}

/// Decode staking ledger from storage value
async fn decode_staking_ledger(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<DecodedStakingLedger, StakingQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode staking ledger",
        ))
    })?;

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            let stash =
                extract_account_id_field(fields, "stash").unwrap_or_else(|| "unknown".to_string());

            let total = extract_u128_field(fields, "total")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "0".to_string());

            let active = extract_u128_field(fields, "active")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "0".to_string());

            let unlocking = extract_unlocking_chunks(fields);

            Ok(DecodedStakingLedger {
                stash,
                total,
                active,
                unlocking,
                claimed_rewards: None, // Will be populated later if requested
            })
        }
        _ => Err(StakingQueryError::DecodeFailed(
            parity_scale_codec::Error::from("Invalid staking ledger format"),
        )),
    }
}

/// Extract unlocking chunks from ledger fields
fn extract_unlocking_chunks(fields: &[(String, Value<()>)]) -> Vec<DecodedUnlockingChunk> {
    let mut chunks = Vec::new();

    if let Some((_, unlocking_value)) = fields.iter().find(|(name, _)| name == "unlocking")
        && let ValueDef::Composite(Composite::Unnamed(items)) = &unlocking_value.value
    {
        for item in items {
            if let ValueDef::Composite(Composite::Named(chunk_fields)) = &item.value {
                let value = extract_u128_field(chunk_fields, "value")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "0".to_string());

                let era = extract_u128_field(chunk_fields, "era")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "0".to_string());

                chunks.push(DecodedUnlockingChunk { value, era });
            }
        }
    }

    chunks
}

/// Decode reward destination from storage value
async fn decode_reward_destination(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<DecodedRewardDestination, StakingQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode reward destination",
        ))
    })?;

    match &decoded.value {
        ValueDef::Variant(variant) => {
            let name = &variant.name;
            match name.as_str() {
                "Staked" | "Stash" | "Controller" | "None" => {
                    Ok(DecodedRewardDestination::Simple(name.clone()))
                }
                "Account" => {
                    // Extract account from variant values
                    if let Composite::Unnamed(values) = &variant.values
                        && let Some(account_value) = values.first()
                        && let Some(account) = extract_account_id_from_value(account_value)
                    {
                        return Ok(DecodedRewardDestination::Account { account });
                    }
                    Ok(DecodedRewardDestination::Simple("Account".to_string()))
                }
                _ => Ok(DecodedRewardDestination::Simple(name.clone())),
            }
        }
        _ => Ok(DecodedRewardDestination::Simple("Staked".to_string())),
    }
}

/// Decode nominations from storage value
async fn decode_nominations(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<Option<DecodedNominationsInfo>, StakingQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode nominations",
        ))
    })?;

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            let targets = extract_targets_field(fields);

            let submitted_in = extract_u128_field(fields, "submittedIn")
                .or_else(|| extract_u128_field(fields, "submitted_in"))
                .map(|v| v.to_string())
                .unwrap_or_else(|| "0".to_string());

            let suppressed = extract_bool_field(fields, "suppressed").unwrap_or(false);

            Ok(Some(DecodedNominationsInfo {
                targets,
                submitted_in,
                suppressed,
            }))
        }
        _ => Ok(None),
    }
}

/// Extract targets (nominated validators) from nominations
fn extract_targets_field(fields: &[(String, Value<()>)]) -> Vec<String> {
    let mut targets = Vec::new();

    if let Some((_, targets_value)) = fields.iter().find(|(name, _)| name == "targets")
        && let ValueDef::Composite(Composite::Unnamed(items)) = &targets_value.value
    {
        for item in items {
            if let Some(account) = extract_account_id_from_value(item) {
                targets.push(account);
            }
        }
    }

    targets
}

/// Decode slashing spans count
async fn decode_slashing_spans(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<u32, StakingQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode slashing spans",
        ))
    })?;

    match &decoded.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            // Count is prior.length + 1
            if let Some((_, prior_value)) = fields.iter().find(|(name, _)| name == "prior")
                && let ValueDef::Composite(Composite::Unnamed(items)) = &prior_value.value
            {
                return Ok(items.len() as u32 + 1);
            }
            Ok(1)
        }
        _ => Ok(0),
    }
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
    let query = subxt::storage::dynamic::<(), scale_value::Value>("Staking", "CurrentEra");

    if let Ok(entry) = client_at_block.storage().entry(query) {
        if let Ok(Some(value)) = entry.try_fetch(()).await {
            let decoded: Value<()> = value.decode_as().map_err(|_| {
                StakingQueryError::DecodeFailed(parity_scale_codec::Error::from(
                    "Failed to decode CurrentEra",
                ))
            })?;

            if let ValueDef::Primitive(scale_value::Primitive::U128(era)) = &decoded.value {
                return Ok(*era as u32);
            }
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
    // Try storage query (older runtimes stored it as a storage item)
    let query = subxt::storage::dynamic::<(), scale_value::Value>("Staking", "HistoryDepth");
    if let Ok(entry) = client_at_block.storage().entry(query) {
        if let Ok(Some(value)) = entry.try_fetch(()).await {
            if let Ok(decoded) = value.decode_as::<Value<()>>() {
                if let ValueDef::Primitive(scale_value::Primitive::U128(depth)) = &decoded.value {
                    return Some(*depth as u32);
                }
            }
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
    let query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "Validators",
    );

    if let Ok(entry) = client_at_block.storage().entry(query) {
        let key = vec![Value::from_bytes(stash_bytes)];
        if let Ok(Some(_)) = entry.try_fetch(key).await {
            return true;
        }
    }

    false
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
    let query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "ClaimedRewards",
    );

    if let Ok(entry) = client_at_block.storage().entry(query) {
        let key = vec![Value::u128(era as u128), Value::from_bytes(*stash_bytes)];

        if let Ok(Some(value)) = entry.try_fetch(key).await {
            if let Ok(decoded) = value.decode_as::<Value<()>>() {
                return extract_u32_vec(&decoded);
            }
        }
    }

    // Try Staking.ErasClaimedRewards (Asset Hub)
    let query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "ErasClaimedRewards",
    );

    if let Ok(entry) = client_at_block.storage().entry(query) {
        let key = vec![Value::u128(era as u128), Value::from_bytes(*stash_bytes)];

        if let Ok(Some(value)) = entry.try_fetch(key).await {
            if let Ok(decoded) = value.decode_as::<Value<()>>() {
                return extract_u32_vec(&decoded);
            }
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
    let query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "ErasStakersOverview",
    );

    if let Ok(entry) = client_at_block.storage().entry(query) {
        let key = vec![Value::u128(era as u128), Value::from_bytes(*stash_bytes)];

        if let Ok(Some(value)) = entry.try_fetch(key).await {
            if let Ok(decoded) = value.decode_as::<Value<()>>() {
                if let ValueDef::Composite(Composite::Named(fields)) = &decoded.value {
                    // Look for pageCount field
                    for (name, val) in fields {
                        if name == "pageCount" || name == "page_count" {
                            if let ValueDef::Primitive(scale_value::Primitive::U128(count)) =
                                &val.value
                            {
                                return Some(*count as u32);
                            }
                        }
                    }
                }
            }
        }
    }

    // If ErasStakersOverview doesn't exist, check ErasStakers (older format, always 1 page)
    let query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>(
        "Staking",
        "ErasStakers",
    );

    if let Ok(entry) = client_at_block.storage().entry(query) {
        let key = vec![Value::u128(era as u128), Value::from_bytes(*stash_bytes)];

        if let Ok(Some(value)) = entry.try_fetch(key).await {
            if let Ok(decoded) = value.decode_as::<Value<()>>() {
                // Check if total > 0
                if let ValueDef::Composite(Composite::Named(fields)) = &decoded.value {
                    for (name, val) in fields {
                        if name == "total" {
                            if let ValueDef::Primitive(scale_value::Primitive::U128(total)) =
                                &val.value
                            {
                                if *total > 0 {
                                    return Some(1); // Old format always has 1 page
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Extract a Vec<u32> from a Value (for claimed page indices)
fn extract_u32_vec(value: &Value<()>) -> Option<Vec<u32>> {
    match &value.value {
        ValueDef::Composite(Composite::Unnamed(items)) => {
            let vec: Vec<u32> = items
                .iter()
                .filter_map(|v| match &v.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(n)) => Some(*n as u32),
                    _ => None,
                })
                .collect();
            Some(vec)
        }
        _ => None,
    }
}

// ================================================================================================
// Helper Functions
// ================================================================================================

/// Extract an AccountId field from named fields and convert to SS58
fn extract_account_id_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<String> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| extract_account_id_from_value(value))
}

/// Extract an AccountId from a Value and convert to SS58
///
/// Handles multiple encoding formats:
/// - `Composite::Unnamed` with byte values (older format)
/// - `Composite::Named` with an "Id" or similar field
/// - Direct byte array representation
fn extract_account_id_from_value(value: &Value<()>) -> Option<String> {
    match &value.value {
        ValueDef::Composite(Composite::Unnamed(bytes)) => {
            // Try to extract as a raw byte array
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
                let account_id = AccountId32::from(arr);
                return Some(account_id.to_ss58check());
            }

            // If we got a single element, it might be a nested account ID
            if bytes.len() == 1 {
                return extract_account_id_from_value(&bytes[0]);
            }

            None
        }
        ValueDef::Composite(Composite::Named(fields)) => {
            // Try common field names for account IDs
            for field_name in ["Id", "id", "account", "stash", "who"] {
                if let Some(account) = extract_account_id_field(fields, field_name) {
                    return Some(account);
                }
            }
            // If there's only one field, try to extract from it
            if fields.len() == 1 {
                return extract_account_id_from_value(&fields[0].1);
            }
            None
        }
        ValueDef::Variant(variant) => {
            // Handle Option<AccountId> or similar variants
            match variant.name.as_str() {
                "Some" | "Id" => {
                    if let Composite::Unnamed(values) = &variant.values {
                        if let Some(inner) = values.first() {
                            return extract_account_id_from_value(inner);
                        }
                    }
                    if let Composite::Named(fields) = &variant.values {
                        if let Some((_, inner)) = fields.first() {
                            return extract_account_id_from_value(inner);
                        }
                    }
                }
                _ => {}
            }
            None
        }
        _ => None,
    }
}

/// Extract u128 field from named fields
fn extract_u128_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<u128> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| match &value.value {
            ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val),
            _ => None,
        })
}

/// Extract bool field from named fields
fn extract_bool_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<bool> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| match &value.value {
            ValueDef::Primitive(scale_value::Primitive::Bool(val)) => Some(*val),
            _ => None,
        })
}
