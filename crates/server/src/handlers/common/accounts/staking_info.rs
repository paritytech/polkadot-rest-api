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
    ClientAtBlockFailed(#[from] subxt::error::OnlineClientAtBlockError),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(#[from] subxt::error::StorageError),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),
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
pub async fn query_staking_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &ResolvedBlock,
) -> Result<RawStakingInfo, StakingQueryError> {
    let bonded_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Bonded");

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
    let key = vec![Value::from_bytes(&account_bytes)];
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
    let ledger_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Ledger");
    let ledger_entry = client_at_block.storage().entry(ledger_query)?;
    let key = vec![Value::from_bytes(&controller_bytes)];
    let ledger_value = ledger_entry.try_fetch(key).await?;

    let staking = if let Some(value) = ledger_value {
        decode_staking_ledger(&value).await?
    } else {
        return Err(StakingQueryError::LedgerNotFound);
    };

    // Query Staking.Payee to get reward destination
    let payee_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Payee");
    let payee_entry = client_at_block.storage().entry(payee_query)?;
    let key = vec![Value::from_bytes(&account_bytes)];
    let payee_value = payee_entry.try_fetch(key).await?;

    let reward_destination = if let Some(value) = payee_value {
        decode_reward_destination(&value).await?
    } else {
        DecodedRewardDestination::Simple("Staked".to_string())
    };

    // Query Staking.Nominators to get nominations
    let nominators_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "Nominators");
    let nominators_entry = client_at_block.storage().entry(nominators_query)?;
    let key = vec![Value::from_bytes(&account_bytes)];
    let nominators_value = nominators_entry.try_fetch(key).await?;

    let nominations = if let Some(value) = nominators_value {
        decode_nominations(&value).await?
    } else {
        None
    };

    // Query Staking.SlashingSpans to get number of slashing spans
    let slashing_query = subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Staking", "SlashingSpans");
    let num_slashing_spans =
        if let Ok(slashing_entry) = client_at_block.storage().entry(slashing_query) {
            let key = vec![Value::from_bytes(&account_bytes)];
            if let Ok(Some(value)) = slashing_entry.try_fetch(key).await {
                decode_slashing_spans(&value).await.unwrap_or(0)
            } else {
                0
            }
        } else {
            0
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
            let stash = extract_account_id_field(fields, "stash")
                .unwrap_or_else(|| "unknown".to_string());

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

    if let Some((_, unlocking_value)) = fields.iter().find(|(name, _)| name == "unlocking") {
        if let ValueDef::Composite(Composite::Unnamed(items)) = &unlocking_value.value {
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
                    if let Composite::Unnamed(values) = &variant.values {
                        if let Some(account_value) = values.first() {
                            if let Some(account) = extract_account_id_from_value(account_value) {
                                return Ok(DecodedRewardDestination::Account { account });
                            }
                        }
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

    if let Some((_, targets_value)) = fields.iter().find(|(name, _)| name == "targets") {
        if let ValueDef::Composite(Composite::Unnamed(items)) = &targets_value.value {
            for item in items {
                if let Some(account) = extract_account_id_from_value(item) {
                    targets.push(account);
                }
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
            if let Some((_, prior_value)) = fields.iter().find(|(name, _)| name == "prior") {
                if let ValueDef::Composite(Composite::Unnamed(items)) = &prior_value.value {
                    return Ok(items.len() as u32 + 1);
                }
            }
            Ok(1)
        }
        _ => Ok(0),
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
fn extract_account_id_from_value(value: &Value<()>) -> Option<String> {
    match &value.value {
        ValueDef::Composite(Composite::Unnamed(bytes)) => {
            // This might be a raw byte array
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
                // Use generic substrate prefix (42)
                Some(account_id.to_ss58check())
            } else {
                None
            }
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
