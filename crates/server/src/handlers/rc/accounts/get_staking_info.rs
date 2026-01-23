use super::types::{
    AccountsError, NominationsInfo, RcStakingInfoQueryParams, RcStakingInfoResponse,
    RelayChainAccess, RewardDestination, StakingLedger,
};
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{
    query_staking_info, DecodedRewardDestination, RawStakingInfo,
};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /rc/accounts/{accountId}/staking-info
///
/// Returns staking information for a given stash account address on the relay chain.
/// This endpoint always queries the relay chain directly.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
pub async fn get_staking_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<RcStakingInfoQueryParams>,
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

    let raw_info = query_staking_info(&client_at_block, &account, &resolved_block).await?;

    let response = format_response(&raw_info);

    Ok(Json(response).into_response())
}

// ================================================================================================
// Relay Chain Access
// ================================================================================================

/// Get access to relay chain client and RPC
fn get_relay_chain_access(state: &AppState) -> Result<RelayChainAccess<'_>, AccountsError> {
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

fn format_response(raw: &RawStakingInfo) -> RcStakingInfoResponse {
    let reward_destination = match &raw.reward_destination {
        DecodedRewardDestination::Simple(name) => RewardDestination::Simple(name.clone()),
        DecodedRewardDestination::Account { account } => RewardDestination::Account {
            account: account.clone(),
        },
    };

    let nominations = raw.nominations.as_ref().map(|n| NominationsInfo {
        targets: n.targets.clone(),
        submitted_in: n.submitted_in.clone(),
        suppressed: n.suppressed,
    });

    // Sum all unlocking chunks to get total unlocking amount
    let unlocking_total: u128 = raw
        .staking
        .unlocking
        .iter()
        .filter_map(|c| c.value.parse::<u128>().ok())
        .sum();

    let staking = StakingLedger {
        stash: raw.staking.stash.clone(),
        total: raw.staking.total.clone(),
        active: raw.staking.active.clone(),
        unlocking: unlocking_total.to_string(),
        claimed_rewards: None, // TODO: Implement when include_claimed_rewards is true
    };

    RcStakingInfoResponse {
        at: super::types::BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        controller: raw.controller.clone(),
        reward_destination,
        num_slashing_spans: raw.num_slashing_spans,
        nominations,
        staking,
    }
}
