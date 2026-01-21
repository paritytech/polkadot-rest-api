use super::types::{
    NominationsInfo, RcStakingInfoError, RcStakingInfoQueryParams, RcStakingInfoResponse,
    RewardDestination, StakingLedger, UnlockingChunk,
};
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{
    query_staking_info as query_staking_info_shared, DecodedRewardDestination, RawStakingInfo,
};
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
) -> Result<Response, RcStakingInfoError> {
    let account = validate_and_parse_address(&account_id)
        .map_err(|_| RcStakingInfoError::InvalidAddress(account_id.clone()))?;

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
        "Fetching RC staking info for account {:?} at block {}",
        account, resolved_block.number
    );

    let raw_info = query_staking_info_shared(rc_client, &account, &resolved_block).await?;

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
    RcStakingInfoError,
> {
    // If we're connected directly to a relay chain, use the primary client
    if state.chain_info.chain_type == ChainType::Relay {
        return Ok((&state.client, &state.rpc_client, &state.legacy_rpc));
    }

    // Otherwise, we need the relay chain client (for Asset Hub or parachain)
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(RcStakingInfoError::RelayChainNotAvailable)?;

    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(RcStakingInfoError::RelayChainNotAvailable)?;

    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(RcStakingInfoError::RelayChainNotAvailable)?;

    Ok((relay_client, relay_rpc_client, relay_rpc))
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(raw: &RawStakingInfo) -> RcStakingInfoResponse {
    let reward_destination = match &raw.reward_destination {
        DecodedRewardDestination::Simple(name) => RewardDestination::Simple(name.clone()),
        DecodedRewardDestination::Account { account } => {
            RewardDestination::Account { account: account.clone() }
        }
    };

    let nominations = raw.nominations.as_ref().map(|n| NominationsInfo {
        targets: n.targets.clone(),
        submitted_in: n.submitted_in.clone(),
        suppressed: n.suppressed,
    });

    let staking = StakingLedger {
        stash: raw.staking.stash.clone(),
        total: raw.staking.total.clone(),
        active: raw.staking.active.clone(),
        unlocking: raw
            .staking
            .unlocking
            .iter()
            .map(|c| UnlockingChunk {
                value: c.value.clone(),
                era: c.era.clone(),
            })
            .collect(),
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
