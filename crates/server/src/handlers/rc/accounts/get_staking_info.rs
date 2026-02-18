// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{
    AccountsError, ClaimedReward, NominationsInfo, RcStakingInfoQueryParams, RcStakingInfoResponse,
    RelayChainAccess, RewardDestination, StakingLedger, UnlockingChunk,
};
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{
    DecodedRewardDestination, RawStakingInfo, query_staking_info,
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
#[utoipa::path(
    get,
    path = "/v1/rc/accounts/{accountId}/staking-info",
    tag = "rc",
    summary = "RC get staking info",
    description = "Returns staking information for a given stash account on the relay chain.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded stash account address"),
        ("at" = Option<String>, Query, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Staking information", body = RcStakingInfoResponse),
        (status = 400, description = "Invalid account address"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_staking_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<RcStakingInfoQueryParams>,
) -> Result<Response, AccountsError> {
    // Get the relay chain ss58_prefix for address validation
    let rc_ss58_prefix = get_relay_chain_ss58_prefix(&state)?;
    let account = validate_and_parse_address(&account_id, rc_ss58_prefix)?;

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
    let client_at_block = rc_client.at_block(resolved_block.number).await?;

    // For RC endpoints, use relay chain spec_name if available
    let rc_spec_name = state
        .relay_chain_info
        .as_ref()
        .map(|info| info.spec_name.as_str())
        .unwrap_or(&state.chain_info.spec_name);

    let raw_info = query_staking_info(
        &client_at_block,
        &account,
        &resolved_block,
        params.include_claimed_rewards,
        rc_ss58_prefix,
        rc_spec_name,
    )
    .await?;

    let response = format_response(&raw_info);

    Ok(Json(response).into_response())
}

// ================================================================================================
// Relay Chain Access
// ================================================================================================

/// Get the SS58 prefix for the relay chain
fn get_relay_chain_ss58_prefix(state: &AppState) -> Result<u16, AccountsError> {
    if state.chain_info.chain_type == ChainType::Relay {
        return Ok(state.chain_info.ss58_prefix);
    }

    state
        .relay_chain_info
        .as_ref()
        .map(|info| info.ss58_prefix)
        .ok_or(AccountsError::RelayChainNotAvailable)
}

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
        DecodedRewardDestination::Simple(name) => match name.as_str() {
            "Staked" => RewardDestination::Staked(()),
            "Stash" => RewardDestination::Stash(()),
            "Controller" => RewardDestination::Controller(()),
            "None" => RewardDestination::None(()),
            _ => RewardDestination::Staked(()),
        },
        DecodedRewardDestination::Account { account } => {
            RewardDestination::Account(account.clone())
        }
    };

    let nominations = raw.nominations.as_ref().map(|n| NominationsInfo {
        targets: n.targets.clone(),
        submitted_in: n.submitted_in.clone(),
        suppressed: n.suppressed,
    });

    // Convert unlocking chunks to response format
    let unlocking: Vec<UnlockingChunk> = raw
        .staking
        .unlocking
        .iter()
        .map(|c| UnlockingChunk {
            value: c.value.clone(),
            era: c.era.clone(),
        })
        .collect();

    // Convert claimed rewards if present
    let claimed_rewards = raw.staking.claimed_rewards.as_ref().map(|rewards| {
        rewards
            .iter()
            .map(|r| ClaimedReward {
                era: r.era.to_string(),
                status: r.status.as_str().to_string(),
            })
            .collect()
    });

    let staking = StakingLedger {
        stash: raw.staking.stash.clone(),
        total: raw.staking.total.clone(),
        active: raw.staking.active.clone(),
        unlocking,
        claimed_rewards,
    };

    RcStakingInfoResponse {
        at: super::types::BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        controller: raw.controller.clone(),
        reward_destination,
        num_slashing_spans: raw.num_slashing_spans.to_string(),
        nominations,
        staking,
    }
}
