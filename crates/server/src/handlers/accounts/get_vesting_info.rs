use super::types::{BlockInfo, AccountsError, VestingInfoQueryParams, VestingInfoResponse, VestingSchedule};
use super::utils::validate_and_parse_address;
use crate::handlers::accounts::utils::fetch_timestamp;
use crate::handlers::common::accounts::{query_vesting_info as query_vesting_info_shared, RawVestingInfo};
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use serde_json::json;
use sp_core::crypto::AccountId32;

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
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id)?;

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

    let raw_info = query_vesting_info_shared(&state.client, &account, &resolved_block).await?;

    let response = format_response(&raw_info, None, None, None);

    Ok(Json(response).into_response())
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(
    raw: &RawVestingInfo,
    rc_block_hash: Option<String>,
    rc_block_number: Option<String>,
    ah_timestamp: Option<String>,
) -> VestingInfoResponse {
    let schedules = raw
        .schedules
        .iter()
        .map(|s| VestingSchedule {
            locked: s.locked.clone(),
            per_block: s.per_block.clone(),
            starting_block: s.starting_block.clone(),
        })
        .collect();

    VestingInfoResponse {
        at: BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        vesting: schedules,
        rc_block_hash,
        rc_block_number,
        ah_timestamp,
    }
}

// ================================================================================================
// Relay Chain Block Handling
// ================================================================================================

async fn handle_use_rc_block(
    state: AppState,
    account: AccountId32,
    params: VestingInfoQueryParams,
) -> Result<Response, AccountsError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(AccountsError::UseRcBlockNotSupported);
    }

    let rc_rpc_client = state.get_relay_chain_rpc_client()
        .ok_or(AccountsError::RelayChainNotConfigured)?;
    let rc_rpc = state.get_relay_chain_rpc()
        .ok_or(AccountsError::RelayChainNotConfigured)?;

    // Resolve RC block
    let rc_block_id = params
        .at
        .clone()
        .unwrap_or_else(|| "head".to_string())
        .parse::<utils::BlockId>()?;
    let rc_resolved = utils::resolve_block_with_rpc(
        rc_rpc_client,
        rc_rpc,
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

        let raw_info = query_vesting_info_shared(&state.client, &account, &ah_resolved).await?;

        // Fetch AH timestamp
        let ah_timestamp = fetch_timestamp(&state, ah_block.number).await.ok();

        let response = format_response(
            &raw_info,
            Some(rc_block_hash.clone()),
            Some(rc_block_number_str.clone()),
            ah_timestamp,
        );

        results.push(response);
    }

    Ok(Json(results).into_response())
}
