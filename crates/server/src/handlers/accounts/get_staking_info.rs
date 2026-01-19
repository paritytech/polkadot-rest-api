use super::types::{
    BlockInfo, NominationsInfo, RewardDestination, StakingInfoError, StakingInfoQueryParams,
    StakingInfoResponse, StakingLedger, UnlockingChunk,
};
use super::utils::validate_and_parse_address;
use crate::handlers::accounts::utils::fetch_timestamp;
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use config::ChainType;
use scale_value::{Composite, Value, ValueDef};
use serde_json::json;
use sp_core::crypto::{AccountId32, Ss58Codec};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/staking-info
///
/// Returns staking information for a given stash account address.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
pub async fn get_staking_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<StakingInfoQueryParams>,
) -> Result<Response, StakingInfoError> {
    let account = validate_and_parse_address(&account_id)
        .map_err(|_| StakingInfoError::InvalidAddress(account_id.clone()))?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let block_id = params.at.map(|s| s.parse::<utils::BlockId>()).transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    println!(
        "Fetching staking info for account {:?} at block {}",
        account, resolved_block.number
    );

    let response = query_staking_info(&state, &account, &resolved_block).await?;

    Ok(Json(response).into_response())
}

async fn query_staking_info(
    state: &AppState,
    account: &AccountId32,
    block: &utils::ResolvedBlock,
) -> Result<StakingInfoResponse, StakingInfoError> {
    let client_at_block = state.client.at(block.number).await?;

    // Check if Staking pallet exists
    let staking_exists = client_at_block
        .storage()
        .entry("Staking", "Bonded")
        .is_ok();

    if !staking_exists {
        return Err(StakingInfoError::StakingPalletNotAvailable);
    }

    let account_bytes: [u8; 32] = *account.as_ref();

    // Query Staking.Bonded to get controller from stash
    let bonded_entry = client_at_block.storage().entry("Staking", "Bonded")?;
    let bonded_value = bonded_entry.fetch(&(&account_bytes,)).await?;

    let controller = if let Some(value) = bonded_value {
        decode_account_id(&value).await?
    } else {
        // Address is not a stash account
        return Err(StakingInfoError::NotAStashAccount);
    };

    let controller_account = AccountId32::from_ss58check(&controller)
        .map_err(|_| StakingInfoError::InvalidAddress(controller.clone()))?;
    let controller_bytes: [u8; 32] = *controller_account.as_ref();

    // Query Staking.Ledger to get staking ledger
    let ledger_entry = client_at_block.storage().entry("Staking", "Ledger")?;
    let ledger_value = ledger_entry.fetch(&(&controller_bytes,)).await?;

    let staking = if let Some(value) = ledger_value {
        decode_staking_ledger(&value).await?
    } else {
        return Err(StakingInfoError::LedgerNotFound);
    };

    // Query Staking.Payee to get reward destination
    let payee_entry = client_at_block.storage().entry("Staking", "Payee")?;
    let payee_value = payee_entry.fetch(&(&account_bytes,)).await?;

    let reward_destination = if let Some(value) = payee_value {
        decode_reward_destination(&value).await?
    } else {
        RewardDestination::Simple("Staked".to_string())
    };

    // Query Staking.Nominators to get nominations
    let nominators_entry = client_at_block.storage().entry("Staking", "Nominators")?;
    let nominators_value = nominators_entry.fetch(&(&account_bytes,)).await?;

    let nominations = if let Some(value) = nominators_value {
        decode_nominations(&value).await?
    } else {
        None
    };

    // Query Staking.SlashingSpans to get number of slashing spans
    let num_slashing_spans =
        if let Ok(slashing_entry) = client_at_block.storage().entry("Staking", "SlashingSpans") {
            if let Ok(Some(value)) = slashing_entry.fetch(&(&account_bytes,)).await {
                decode_slashing_spans(&value).await.unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

    Ok(StakingInfoResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        controller,
        reward_destination,
        num_slashing_spans,
        nominations,
        staking,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ================================================================================================
// Decoding Functions
// ================================================================================================

/// Decode an AccountId from a storage value
async fn decode_account_id(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<String, StakingInfoError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingInfoError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode account id",
        ))
    })?;

    extract_account_id_from_value(&decoded).ok_or_else(|| {
        StakingInfoError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to extract account id",
        ))
    })
}

/// Decode staking ledger from storage value
async fn decode_staking_ledger(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<StakingLedger, StakingInfoError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingInfoError::DecodeFailed(parity_scale_codec::Error::from(
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

            Ok(StakingLedger {
                stash,
                total,
                active,
                unlocking,
            })
        }
        _ => Err(StakingInfoError::DecodeFailed(
            parity_scale_codec::Error::from("Invalid staking ledger format"),
        )),
    }
}

/// Extract unlocking chunks from ledger fields
fn extract_unlocking_chunks(fields: &[(String, Value<()>)]) -> Vec<UnlockingChunk> {
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

                    chunks.push(UnlockingChunk { value, era });
                }
            }
        }
    }

    chunks
}

/// Decode reward destination from storage value
async fn decode_reward_destination(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<RewardDestination, StakingInfoError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingInfoError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode reward destination",
        ))
    })?;

    match &decoded.value {
        ValueDef::Variant(variant) => {
            let name = &variant.name;
            match name.as_str() {
                "Staked" | "Stash" | "Controller" | "None" => {
                    Ok(RewardDestination::Simple(name.clone()))
                }
                "Account" => {
                    // Extract account from variant values
                    if let Composite::Unnamed(values) = &variant.values {
                        if let Some(account_value) = values.first() {
                            if let Some(account) = extract_account_id_from_value(account_value) {
                                return Ok(RewardDestination::Account { account });
                            }
                        }
                    }
                    Ok(RewardDestination::Simple("Account".to_string()))
                }
                _ => Ok(RewardDestination::Simple(name.clone())),
            }
        }
        _ => Ok(RewardDestination::Simple("Staked".to_string())),
    }
}

/// Decode nominations from storage value
async fn decode_nominations(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<Option<NominationsInfo>, StakingInfoError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingInfoError::DecodeFailed(parity_scale_codec::Error::from(
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

            Ok(Some(NominationsInfo {
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
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<u32, StakingInfoError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        StakingInfoError::DecodeFailed(parity_scale_codec::Error::from(
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

// ================================================================================================
// Relay Chain Block Handling
// ================================================================================================

async fn handle_use_rc_block(
    state: AppState,
    account: AccountId32,
    params: StakingInfoQueryParams,
) -> Result<Response, StakingInfoError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(StakingInfoError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(StakingInfoError::RelayChainNotConfigured);
    }

    // Resolve RC block
    let rc_block_id = params
        .at
        .unwrap_or_else(|| "head".to_string())
        .parse::<utils::BlockId>()?;
    let rc_resolved = utils::resolve_block_with_rpc(
        state.get_relay_chain_rpc_client().unwrap(),
        state.get_relay_chain_rpc().unwrap(),
        Some(rc_block_id),
    )
    .await?;

    // Find AH blocks
    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved).await?;

    if ah_blocks.is_empty() {
        return Ok(Json(json!([])).into_response());
    }

    let rc_block_hash = rc_resolved.hash.clone();
    let rc_block_number = rc_resolved.number.to_string();

    // Process each AH block
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };

        let mut response = query_staking_info(&state, &account, &ah_resolved).await?;

        // Add RC block info
        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        // Fetch AH timestamp
        if let Ok(timestamp) = fetch_timestamp(&state, ah_block.number).await {
            response.ah_timestamp = Some(timestamp);
        }

        results.push(response);
    }

    Ok(Json(results).into_response())
}
