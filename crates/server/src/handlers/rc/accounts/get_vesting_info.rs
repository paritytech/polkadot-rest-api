use super::types::{
    AccountsError, RcVestingInfoQueryParams, RcVestingInfoResponse, VestingSchedule,
};
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{RawVestingInfo, query_vesting_info};
use crate::state::{AppState, SubstrateLegacyRpc};
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use std::sync::Arc;
use subxt::{OnlineClient, SubstrateConfig};
use subxt_rpcs::RpcClient;

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
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id)?;

    // Get the relay chain client and info
    let (rc_client, rc_rpc_client, rc_rpc) = get_relay_chain_access(&state)?;

    // Resolve block on relay chain
    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;

    let resolved_block =
        utils::resolve_block_with_rpc(rc_rpc_client, rc_rpc.as_ref(), block_id).await?;

    let client_at_block = match params.at {
        None => rc_client.at_current_block().await?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<utils::BlockId>()?;
            match block_id {
                utils::BlockId::Hash(hash) => rc_client.at_block(hash).await?,
                utils::BlockId::Number(number) => rc_client.at_block(number).await?,
            }
        }
    };

    let raw_info = query_vesting_info(&client_at_block, &account, &resolved_block).await?;

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
        &Arc<SubstrateLegacyRpc>,
    ),
    AccountsError,
> {
    // If we're connected directly to a relay chain, use the primary client
    if state.chain_info.chain_type == ChainType::Relay {
        return Ok((&state.client, &state.rpc_client, &state.legacy_rpc));
    }

    // Otherwise, we need the relay chain client (for Asset Hub or parachain)
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(AccountsError::RelayChainNotAvailable)?;

    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(AccountsError::RelayChainNotAvailable)?;

    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(AccountsError::RelayChainNotAvailable)?;

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
        })
        .collect();

    RcVestingInfoResponse {
        at: super::types::BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        vesting: schedules,
    }
}
