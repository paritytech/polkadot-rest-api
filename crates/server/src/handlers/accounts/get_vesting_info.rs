use super::types::{BlockInfo, VestingInfoError, VestingInfoQueryParams, VestingInfoResponse, VestingSchedule};
use super::utils::validate_and_parse_address;
use crate::handlers::accounts::utils::fetch_timestamp;
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use scale_value::{Composite, Value, ValueDef};
use serde_json::json;
use sp_core::crypto::AccountId32;

/// Vesting lock ID: "vesting " padded to 8 bytes (0x76657374696e6720)
const VESTING_LOCK_ID: &str = "vesting ";

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/vesting-info
///
/// Returns vesting information for a given account.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `includeClaimable` (optional): When true, calculate vested amounts
pub async fn get_vesting_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<VestingInfoQueryParams>,
) -> Result<Response, VestingInfoError> {
    let account = validate_and_parse_address(&account_id)
        .map_err(|_| VestingInfoError::InvalidAddress(account_id.clone()))?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    println!(
        "Fetching vesting info for account {:?} at block {}",
        account, resolved_block.number
    );

    let response =
        query_vesting_info(&state, &account, &resolved_block, params.include_claimable, None)
            .await?;

    Ok(Json(response).into_response())
}

// ================================================================================================
// Vesting Info Query
// ================================================================================================

async fn query_vesting_info(
    state: &AppState,
    account: &AccountId32,
    block: &utils::ResolvedBlock,
    include_claimable: bool,
    rc_block_number: Option<u64>,
) -> Result<VestingInfoResponse, VestingInfoError> {
    let client_at_block = state.client.at(block.number).await?;

    // Check if Vesting pallet exists
    let vesting_exists = client_at_block
        .storage()
        .entry("Vesting", "Vesting")
        .is_ok();

    if !vesting_exists {
        return Err(VestingInfoError::VestingPalletNotAvailable);
    }

    // Query vesting schedules
    let vesting_schedules = query_vesting_schedules(state, block.number, account).await?;

    // If no vesting schedules, return empty response
    if vesting_schedules.is_empty() {
        return Ok(VestingInfoResponse {
            at: BlockInfo {
                hash: block.hash.clone(),
                height: block.number.to_string(),
            },
            vesting: Vec::new(),
            vested_balance: None,
            vesting_total: None,
            vested_claimable: None,
            block_number_for_calculation: None,
            block_number_source: None,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        });
    }

    // If includeClaimable is not requested, return raw vesting data
    if !include_claimable {
        let schedules: Vec<VestingSchedule> = vesting_schedules
            .iter()
            .map(|s| VestingSchedule {
                locked: s.locked.to_string(),
                per_block: s.per_block.to_string(),
                starting_block: s.starting_block.to_string(),
                vested: None,
            })
            .collect();

        return Ok(VestingInfoResponse {
            at: BlockInfo {
                hash: block.hash.clone(),
                height: block.number.to_string(),
            },
            vesting: schedules,
            vested_balance: None,
            vesting_total: None,
            vested_claimable: None,
            block_number_for_calculation: None,
            block_number_source: None,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        });
    }

    // Get the on-chain vesting lock amount from balances.locks
    let vesting_locked = query_vesting_lock(state, block.number, account).await?;

    // Determine which block number to use for calculations
    let (calculation_block, block_source) = if let Some(rc_block) = rc_block_number {
        // When using relay chain block mapping, use the RC block number for calculations
        (rc_block, "relay")
    } else {
        // Use the chain's own block number
        (block.number, "self")
    };

    // Calculate vesting amounts
    let calc_result =
        calculate_vesting_amounts(&vesting_schedules, vesting_locked, calculation_block);

    Ok(VestingInfoResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        vesting: calc_result.schedules,
        vested_balance: Some(calc_result.vested_balance),
        vesting_total: Some(calc_result.vesting_total),
        vested_claimable: Some(calc_result.vested_claimable),
        block_number_for_calculation: Some(calculation_block.to_string()),
        block_number_source: Some(block_source.to_string()),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ================================================================================================
// Storage Queries
// ================================================================================================

/// Raw vesting schedule from storage
#[derive(Debug, Clone)]
struct RawVestingSchedule {
    locked: u128,
    per_block: u128,
    starting_block: u64,
}

async fn query_vesting_schedules(
    state: &AppState,
    block_number: u64,
    account: &AccountId32,
) -> Result<Vec<RawVestingSchedule>, VestingInfoError> {
    let client_at_block = state.client.at(block_number).await?;
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
) -> Result<Vec<RawVestingSchedule>, VestingInfoError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        VestingInfoError::DecodeFailed(parity_scale_codec::Error::from(
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
    state: &AppState,
    block_number: u64,
    account: &AccountId32,
) -> Result<u128, VestingInfoError> {
    let client_at_block = state.client.at(block_number).await?;

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
) -> Result<u128, VestingInfoError> {
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        VestingInfoError::DecodeFailed(parity_scale_codec::Error::from(
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

struct VestingCalculationResult {
    schedules: Vec<VestingSchedule>,
    vested_balance: String,
    vesting_total: String,
    vested_claimable: String,
}

fn calculate_vesting_amounts(
    schedules: &[RawVestingSchedule],
    vesting_locked: u128,
    current_block: u64,
) -> VestingCalculationResult {
    let mut total_vested: u128 = 0;
    let mut total_locked: u128 = 0;

    let calculated_schedules: Vec<VestingSchedule> = schedules
        .iter()
        .map(|s| {
            let vested = calculate_vested(current_block, s);
            total_vested = total_vested.saturating_add(vested);
            total_locked = total_locked.saturating_add(s.locked);

            VestingSchedule {
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

// ================================================================================================
// Relay Chain Block Handling
// ================================================================================================

async fn handle_use_rc_block(
    state: AppState,
    account: AccountId32,
    params: VestingInfoQueryParams,
) -> Result<Response, VestingInfoError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(VestingInfoError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(VestingInfoError::RelayChainNotConfigured);
    }

    // Resolve RC block
    let rc_block_id = params
        .at
        .clone()
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
    let rc_block_number_str = rc_resolved.number.to_string();

    // Process each AH block
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };

        // When using RC block, pass the RC block number for vesting calculations
        let mut response = query_vesting_info(
            &state,
            &account,
            &ah_resolved,
            params.include_claimable,
            Some(rc_resolved.number),
        )
        .await?;

        // Add RC block info
        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number_str.clone());

        // Fetch AH timestamp
        if let Ok(timestamp) = fetch_timestamp(&state, ah_block.number).await {
            response.ah_timestamp = Some(timestamp);
        }

        results.push(response);
    }

    Ok(Json(results).into_response())
}
