//! Common vesting info utilities shared across handler modules.

use crate::utils::ResolvedBlock;
use parity_scale_codec::Decode;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// SCALE Decode Types for Vesting::Vesting storage
// ================================================================================================

/// Vesting schedule structure with compact fields
#[derive(Debug, Clone, Decode)]
struct VestingScheduleCompact {
    #[codec(compact)]
    locked: u128,
    #[codec(compact)]
    per_block: u128,
    #[codec(compact)]
    starting_block: u32,
}

/// Vesting schedule structure with non-compact fields (older runtimes)
#[derive(Debug, Clone, Decode)]
struct VestingScheduleNonCompact {
    locked: u128,
    per_block: u128,
    starting_block: u64,
}

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum VestingQueryError {
    #[error("The runtime does not include the vesting pallet at this block")]
    VestingPalletNotAvailable,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(Box<subxt::error::OnlineClientAtBlockError>),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(Box<subxt::error::StorageError>),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),
}

impl From<subxt::error::OnlineClientAtBlockError> for VestingQueryError {
    fn from(err: subxt::error::OnlineClientAtBlockError) -> Self {
        VestingQueryError::ClientAtBlockFailed(Box::new(err))
    }
}

impl From<subxt::error::StorageError> for VestingQueryError {
    fn from(err: subxt::error::StorageError) -> Self {
        VestingQueryError::StorageQueryFailed(Box::new(err))
    }
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
    // Check if Vesting pallet exists
    if client_at_block
        .storage()
        .entry(("Vesting", "Vesting"))
        .is_err()
    {
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
    // Build the storage address for Vesting::Vesting(account)
    let storage_addr = subxt::dynamic::storage::<_, Vec<u8>>("Vesting", "Vesting");
    let account_bytes: [u8; 32] = *account.as_ref();

    let storage_value = client_at_block
        .storage()
        .fetch(storage_addr, (account_bytes,))
        .await;

    if let Ok(value) = storage_value {
        let raw_bytes = value.into_bytes();
        decode_vesting_schedules(&raw_bytes)
    } else {
        Ok(Vec::new())
    }
}

/// Decode vesting schedules from raw SCALE bytes
fn decode_vesting_schedules(raw_bytes: &[u8]) -> Result<Vec<RawVestingSchedule>, VestingQueryError> {
    // Try decoding as Vec<VestingScheduleCompact> (modern runtime)
    if let Ok(schedules) = Vec::<VestingScheduleCompact>::decode(&mut &raw_bytes[..]) {
        return Ok(schedules
            .into_iter()
            .map(|s| RawVestingSchedule {
                locked: s.locked,
                per_block: s.per_block,
                starting_block: s.starting_block as u64,
            })
            .collect());
    }

    // Try decoding as Vec<VestingScheduleNonCompact> (older runtime)
    if let Ok(schedules) = Vec::<VestingScheduleNonCompact>::decode(&mut &raw_bytes[..]) {
        return Ok(schedules
            .into_iter()
            .map(|s| RawVestingSchedule {
                locked: s.locked,
                per_block: s.per_block,
                starting_block: s.starting_block,
            })
            .collect());
    }

    // Try decoding as single VestingScheduleCompact
    if let Ok(schedule) = VestingScheduleCompact::decode(&mut &raw_bytes[..]) {
        return Ok(vec![RawVestingSchedule {
            locked: schedule.locked,
            per_block: schedule.per_block,
            starting_block: schedule.starting_block as u64,
        }]);
    }

    // Try decoding as single VestingScheduleNonCompact
    if let Ok(schedule) = VestingScheduleNonCompact::decode(&mut &raw_bytes[..]) {
        return Ok(vec![RawVestingSchedule {
            locked: schedule.locked,
            per_block: schedule.per_block,
            starting_block: schedule.starting_block,
        }]);
    }

    // If all decoding attempts fail, return empty (no vesting)
    Ok(Vec::new())
}
