use super::types::{RcVestingInfoError, RcVestingInfoQueryParams, RcVestingInfoResponse, VestingSchedule};
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{query_vesting_info as query_vesting_info_shared, RawVestingInfo};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use std::sync::Arc;
use subxt_historic::{OnlineClient, SubstrateConfig};
use subxt_rpcs::{LegacyRpcMethods, RpcClient};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /rc/accounts/{accountId}/vesting-info
///
/// Returns vesting information for a given account on the relay chain.
/// This endpoint always queries the relay chain directly.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `includeClaimable` (optional): When true, calculate vested amounts
pub async fn get_vesting_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<RcVestingInfoQueryParams>,
) -> Result<Response, RcVestingInfoError> {
    let account = validate_and_parse_address(&account_id)
        .map_err(|_| RcVestingInfoError::InvalidAddress(account_id.clone()))?;

    // Get the relay chain client and info
    let (rc_client, rc_rpc_client, rc_rpc) = get_relay_chain_access(&state)?;

    // Resolve block on relay chain
    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;

    let resolved_block = utils::resolve_block_with_rpc(rc_rpc_client, rc_rpc, block_id).await?;

    println!(
        "Fetching RC vesting info for account {:?} at block {}",
        account, resolved_block.number
    );

    let raw_info = query_vesting_info_shared(
        rc_client,
        &account,
        &resolved_block,
        params.include_claimable,
        None, // No RC block mapping needed - we're already on RC
    )
    .await?;

    let response = format_response(&raw_info);

    Ok(Json(response).into_response())
}

// ================================================================================================
// Relay Chain Access
// ================================================================================================

/// Get access to relay chain client and RPC
/// Returns (client, rpc_client, legacy_rpc)
fn get_relay_chain_access(
    state: &AppState,
) -> Result<
    (
        &Arc<OnlineClient<SubstrateConfig>>,
        &Arc<RpcClient>,
        &Arc<LegacyRpcMethods<SubstrateConfig>>,
    ),
    RcVestingInfoError,
> {
    // If we're connected directly to a relay chain, use the primary client
    if state.chain_info.chain_type == ChainType::Relay {
        return Ok((&state.client, &state.rpc_client, &state.legacy_rpc));
    }

    // Otherwise, we need the relay chain client (for Asset Hub or parachain)
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(RcVestingInfoError::RelayChainNotAvailable)?;

    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(RcVestingInfoError::RelayChainNotAvailable)?;

    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(RcVestingInfoError::RelayChainNotAvailable)?;

    Ok((relay_client, relay_rpc_client, relay_rpc))
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(raw: &RawVestingInfo) -> RcVestingInfoResponse {
    let schedules = raw
        .schedules
        .iter()
        .map(|s| VestingSchedule {
            locked: s.locked.clone(),
            per_block: s.per_block.clone(),
            starting_block: s.starting_block.clone(),
            vested: s.vested.clone(),
        })
        .collect();

    RcVestingInfoResponse {
        at: super::types::BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        vesting: schedules,
        vested_balance: raw.vested_balance.clone(),
        vesting_total: raw.vesting_total.clone(),
        vested_claimable: raw.vested_claimable.clone(),
        block_number_for_calculation: raw.block_number_for_calculation.clone(),
        block_number_source: raw.block_number_source.clone(),
    }
}
