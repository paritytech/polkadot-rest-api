//! Common vesting info utilities shared across handler modules.

use crate::utils::ResolvedBlock;
use scale_value::{Composite, Value, ValueDef};
use sp_core::crypto::AccountId32;
use std::sync::Arc;
use subxt_historic::{OnlineClient, SubstrateConfig};
use thiserror::Error;

/// Vesting lock ID: "vesting " padded to 8 bytes (0x76657374696e6720)
const VESTING_LOCK_ID: &str = "vesting ";

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum VestingQueryError {
    #[error("The runtime does not include the vesting pallet at this block")]
    VestingPalletNotAvailable,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[from] subxt_historic::error::OnlineClientAtBlockError),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(#[from] subxt_historic::error::StorageError),

    #[error("Failed to fetch storage entry")]
    StorageEntryFailed(#[from] subxt_historic::error::StorageEntryIsNotAPlainValue),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),
}

// ================================================================================================
// Data Types
// ================================================================================================

/// Raw vesting info data returned from storage query
#[derive(Debug)]
pub struct RawVestingInfo {
    /// Block information
    pub block: FormattedBlockInfo,
    /// Vesting schedules
    pub schedules: Vec<DecodedVestingSchedule>,
    /// Total vested balance (only when include_claimable is true)
    pub vested_balance: Option<String>,
    /// Total vesting amount (only when include_claimable is true)
    pub vesting_total: Option<String>,
    /// Claimable amount (only when include_claimable is true)
    pub vested_claimable: Option<String>,
    /// Block number used for calculations (only when include_claimable is true)
    pub block_number_for_calculation: Option<String>,
    /// Source of block number: "relay" or "self" (only when include_claimable is true)
    pub block_number_source: Option<String>,
}

/// Block information for response
#[derive(Debug, Clone)]
pub struct FormattedBlockInfo {
    pub hash: String,
    pub number: u64,
}

/// Decoded vesting schedule from storage
#[derive(Debug, Clone)]
pub struct DecodedVestingSchedule {
    /// Total tokens locked at start of vesting
    pub locked: String,
    /// Tokens unlocked per block
    pub per_block: String,
    /// Block when vesting begins
    pub starting_block: String,
    /// Amount vested (only when include_claimable is true)
    pub vested: Option<String>,
}

/// Raw vesting schedule from storage (internal)
#[derive(Debug, Clone)]
struct RawVestingSchedule {
    locked: u128,
    per_block: u128,
    starting_block: u64,
}

/// Result of vesting calculation (internal)
struct VestingCalculationResult {
    schedules: Vec<DecodedVestingSchedule>,
    vested_balance: String,
    vesting_total: String,
    vested_claimable: String,
}

// ================================================================================================
// Core Query Function
// ================================================================================================

/// Query vesting info from storage
///
/// This is the shared function used by both `/accounts/:accountId/vesting-info`
/// and `/rc/accounts/:accountId/vesting-info` endpoints.
///
/// Parameters:
/// - `client`: The subxt client to query
/// - `account`: The account to query vesting for
/// - `block`: The block to query at
/// - `include_claimable`: Whether to calculate vested amounts
/// - `calculation_block_number`: Optional block number to use for calculations (for RC block mapping)
pub async fn query_vesting_info(
    client: &Arc<OnlineClient<SubstrateConfig>>,
    account: &AccountId32,
    block: &ResolvedBlock,
    include_claimable: bool,
    calculation_block_number: Option<u64>,
) -> Result<RawVestingInfo, VestingQueryError> {
    let client_at_block = client.at(block.number).await?;

    // Check if Vesting pallet exists
    let vesting_exists = client_at_block
        .storage()
        .entry("Vesting", "Vesting")
        .is_ok();

    if !vesting_exists {
        return Err(VestingQueryError::VestingPalletNotAvailable);
    }

    // Query vesting schedules
    let vesting_schedules = query_vesting_schedules(client, block.number, account).await?;

    // If no vesting schedules, return empty response
    if vesting_schedules.is_empty() {
        return Ok(RawVestingInfo {
            block: FormattedBlockInfo {
                hash: block.hash.clone(),
                number: block.number,
            },
            schedules: Vec::new(),
            vested_balance: None,
            vesting_total: None,
            vested_claimable: None,
            block_number_for_calculation: None,
            block_number_source: None,
        });
    }

    // If includeClaimable is not requested, return raw vesting data
    if !include_claimable {
        let schedules: Vec<DecodedVestingSchedule> = vesting_schedules
            .iter()
            .map(|s| DecodedVestingSchedule {
                locked: s.locked.to_string(),
                per_block: s.per_block.to_string(),
                starting_block: s.starting_block.to_string(),
                vested: None,
            })
            .collect();

        return Ok(RawVestingInfo {
            block: FormattedBlockInfo {
                hash: block.hash.clone(),
                number: block.number,
            },
            schedules,
            vested_balance: None,
            vesting_total: None,
            vested_claimable: None,
            block_number_for_calculation: None,
            block_number_source: None,
        });
    }

    // Get the on-chain vesting lock amount from balances.locks
    let vesting_locked = query_vesting_lock(client, block.number, account).await?;

    // Determine which block number to use for calculations
    let (calculation_block, block_source) = if let Some(rc_block) = calculation_block_number {
        // When using relay chain block mapping, use the RC block number for calculations
        (rc_block, "relay")
    } else {
        // Use the chain's own block number
        (block.number, "self")
    };

    // Calculate vesting amounts
    let calc_result =
        calculate_vesting_amounts(&vesting_schedules, vesting_locked, calculation_block);

    Ok(RawVestingInfo {
        block: FormattedBlockInfo {
            hash: block.hash.clone(),
            number: block.number,
        },
        schedules: calc_result.schedules,
        vested_balance: Some(calc_result.vested_balance),
        vesting_total: Some(calc_result.vesting_total),
        vested_claimable: Some(calc_result.vested_claimable),
        block_number_for_calculation: Some(calculation_block.to_string()),
        block_number_source: Some(block_source.to_string()),
    })
}

// ================================================================================================
// Storage Queries
// ================================================================================================

async fn query_vesting_schedules(
    client: &Arc<OnlineClient<SubstrateConfig>>,
    block_number: u64,
    account: &AccountId32,
) -> Result<Vec<RawVestingSchedule>, VestingQueryError> {
    let client_at_block = client.at(block_number).await?;
    let storage_entry = client_at_block.storage().entry("Vesting", "Vesting")?;

    // Vesting::Vesting takes a single AccountId key
    let account_bytes: [u8; 32] = *account.as_ref();
    let storage_value = storage_entry.fetch(&(&account_bytes,)).await?;

    if let Some(value) = storage_value {
        decode_vesting_schedules(&value).await
    } else {
        Ok(Vec::new())
    }
}

async fn decode_vesting_schedules(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<Vec<RawVestingSchedule>, VestingQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        VestingQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode vesting schedules",
        ))
    })?;

    let mut schedules = Vec::new();

    // Vesting schedules can be a single schedule or a BoundedVec of schedules
    match &decoded.value {
        // Vec/BoundedVec of schedules
        ValueDef::Composite(Composite::Unnamed(items)) => {
            for item in items {
                if let Some(schedule) = decode_single_schedule(item) {
                    schedules.push(schedule);
                }
            }
        }
        // Single schedule (for older runtimes)
        ValueDef::Composite(Composite::Named(fields)) => {
            if let Some(schedule) = decode_schedule_from_fields(fields) {
                schedules.push(schedule);
            }
        }
        _ => {}
    }

    Ok(schedules)
}

fn decode_single_schedule(value: &Value<()>) -> Option<RawVestingSchedule> {
    match &value.value {
        ValueDef::Composite(Composite::Named(fields)) => decode_schedule_from_fields(fields),
        _ => None,
    }
}

fn decode_schedule_from_fields(fields: &[(String, Value<()>)]) -> Option<RawVestingSchedule> {
    let locked = extract_u128_field(fields, "locked")?;
    let per_block = extract_u128_field(fields, "perBlock")
        .or_else(|| extract_u128_field(fields, "per_block"))?;
    let starting_block = extract_u128_field(fields, "startingBlock")
        .or_else(|| extract_u128_field(fields, "starting_block"))?
        as u64;

    Some(RawVestingSchedule {
        locked,
        per_block,
        starting_block,
    })
}

fn extract_u128_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<u128> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| match &value.value {
            ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val),
            _ => None,
        })
}

async fn query_vesting_lock(
    client: &Arc<OnlineClient<SubstrateConfig>>,
    block_number: u64,
    account: &AccountId32,
) -> Result<u128, VestingQueryError> {
    let client_at_block = client.at(block_number).await?;

    // Check if Balances::Locks exists
    let locks_exists = client_at_block
        .storage()
        .entry("Balances", "Locks")
        .is_ok();

    if !locks_exists {
        return Ok(0);
    }

    let storage_entry = client_at_block.storage().entry("Balances", "Locks")?;
    let account_bytes: [u8; 32] = *account.as_ref();
    let storage_value = storage_entry.fetch(&(&account_bytes,)).await?;

    if let Some(value) = storage_value {
        decode_vesting_lock(&value).await
    } else {
        Ok(0)
    }
}

async fn decode_vesting_lock(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<u128, VestingQueryError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        VestingQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode balance locks",
        ))
    })?;

    // Locks is a Vec<BalanceLock>
    if let ValueDef::Composite(Composite::Unnamed(items)) = &decoded.value {
        for item in items {
            if let ValueDef::Composite(Composite::Named(fields)) = &item.value {
                // Extract id
                let id = fields
                    .iter()
                    .find(|(name, _)| name == "id")
                    .map(|(_, v)| extract_lock_id(v))
                    .unwrap_or_default();

                // Check if this is the vesting lock
                if id == VESTING_LOCK_ID || id.starts_with("vesting") {
                    if let Some(amount) = extract_u128_field(fields, "amount") {
                        return Ok(amount);
                    }
                }
            }
        }
    }

    Ok(0)
}

fn extract_lock_id(value: &Value<()>) -> String {
    match &value.value {
        ValueDef::Composite(Composite::Unnamed(bytes)) => {
            let byte_vec: Vec<u8> = bytes
                .iter()
                .filter_map(|v| match &v.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(b)) => Some(*b as u8),
                    _ => None,
                })
                .collect();

            String::from_utf8_lossy(&byte_vec)
                .trim_end_matches('\0')
                .to_string()
        }
        _ => String::new(),
    }
}

// ================================================================================================
// Vesting Calculations
// ================================================================================================

fn calculate_vesting_amounts(
    schedules: &[RawVestingSchedule],
    vesting_locked: u128,
    current_block: u64,
) -> VestingCalculationResult {
    let mut total_vested: u128 = 0;
    let mut total_locked: u128 = 0;

    let calculated_schedules: Vec<DecodedVestingSchedule> = schedules
        .iter()
        .map(|s| {
            let vested = calculate_vested(current_block, s);
            total_vested = total_vested.saturating_add(vested);
            total_locked = total_locked.saturating_add(s.locked);

            DecodedVestingSchedule {
                locked: s.locked.to_string(),
                per_block: s.per_block.to_string(),
                starting_block: s.starting_block.to_string(),
                vested: Some(vested.to_string()),
            }
        })
        .collect();

    // Calculate claimable amount
    // vestedClaimable = vestingLocked - (vestingTotal - vestedBalance)
    let still_locked = total_locked.saturating_sub(total_vested);
    let vested_claimable = vesting_locked.saturating_sub(still_locked);

    VestingCalculationResult {
        schedules: calculated_schedules,
        vested_balance: total_vested.to_string(),
        vesting_total: total_locked.to_string(),
        vested_claimable: vested_claimable.to_string(),
    }
}

/// Calculate the amount that has vested for a single vesting schedule.
///
/// The calculation follows the formula used in the vesting pallet:
/// - If currentBlock <= startingBlock: nothing is vested yet
/// - Otherwise: vested = min(blocksPassed * perBlock, locked)
fn calculate_vested(current_block: u64, schedule: &RawVestingSchedule) -> u128 {
    // Vesting hasn't started yet
    if current_block <= schedule.starting_block {
        return 0;
    }

    // Calculate how many blocks have passed since vesting started
    let blocks_passed = current_block - schedule.starting_block;

    // Calculate vested amount: blocksPassed * perBlock
    let vested = (blocks_passed as u128).saturating_mul(schedule.per_block);

    // Return the minimum of vested and locked (can't vest more than was locked)
    std::cmp::min(vested, schedule.locked)
}
