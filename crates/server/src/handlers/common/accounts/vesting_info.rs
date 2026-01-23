//! Common vesting info utilities shared across handler modules.

use crate::utils::ResolvedBlock;
use scale_value::{Composite, Value, ValueDef};
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum VestingQueryError {
    #[error("The runtime does not include the vesting pallet at this block")]
    VestingPalletNotAvailable,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[from] subxt::error::OnlineClientAtBlockError),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(#[from] subxt::error::StorageError),

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
}

/// Raw vesting schedule from storage (internal)
#[derive(Debug, Clone)]
struct RawVestingSchedule {
    locked: u128,
    per_block: u128,
    starting_block: u64,
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
pub async fn query_vesting_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &ResolvedBlock,
) -> Result<RawVestingInfo, VestingQueryError> {
    let storage_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Vesting", "Vesting");

    // Check if Vesting pallet exists
    let vesting_exists = client_at_block
        .storage()
        .entry(storage_query)
        .is_ok();

    if !vesting_exists {
        return Err(VestingQueryError::VestingPalletNotAvailable);
    }

    // Query vesting schedules
    let vesting_schedules = query_vesting_schedules(client_at_block, account).await?;

    // Convert to decoded schedules
    let schedules: Vec<DecodedVestingSchedule> = vesting_schedules
        .iter()
        .map(|s| DecodedVestingSchedule {
            locked: s.locked.to_string(),
            per_block: s.per_block.to_string(),
            starting_block: s.starting_block.to_string(),
        })
        .collect();

    Ok(RawVestingInfo {
        block: FormattedBlockInfo {
            hash: block.hash.clone(),
            number: block.number,
        },
        schedules,
    })
}

// ================================================================================================
// Storage Queries
// ================================================================================================

async fn query_vesting_schedules(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Result<Vec<RawVestingSchedule>, VestingQueryError> {
    let storage_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Vesting", "Vesting");
    let storage_entry = client_at_block.storage().entry(storage_query)?;

    // Vesting::Vesting takes a single AccountId key
    let account_bytes: [u8; 32] = *account.as_ref();
    let key = vec![Value::from_bytes(&account_bytes)];
    let storage_value = storage_entry.try_fetch(key).await?;

    if let Some(value) = storage_value {
        decode_vesting_schedules(&value).await
    } else {
        Ok(Vec::new())
    }
}

async fn decode_vesting_schedules(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
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
