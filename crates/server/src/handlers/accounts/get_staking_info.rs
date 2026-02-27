// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{
    AccountsError, BlockInfo, ClaimedReward, NominationsInfo, RewardDestination,
    StakingInfoQueryParams, StakingInfoResponse, StakingLedger, UnlockingChunk,
};
use super::utils::validate_and_parse_address;
use crate::extractors::JsonQuery;
use crate::handlers::common::accounts::{
    DecodedRewardDestination, RawStakingInfo, query_staking_info,
};
use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use polkadot_rest_api_config::ChainType;
use serde_json::json;
use sp_core::crypto::AccountId32;

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
#[utoipa::path(
    get,
    path = "/v1/accounts/{accountId}/staking-info",
    tag = "accounts",
    summary = "Account staking info",
    description = "Returns staking information for a given stash account including bonded amount, controller, and nominations.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded stash account address"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier"),
        ("includeClaimedRewards" = Option<bool>, description = "When true, include claimed rewards in the response")
    ),
    responses(
        (status = 200, description = "Staking information", body = StakingInfoResponse),
        (status = 400, description = "Invalid parameters"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_staking_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    JsonQuery(params): JsonQuery<StakingInfoQueryParams>,
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id, state.chain_info.ss58_prefix)?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let client_at_block = utils::resolve_client_at_block(&state.client, params.at.as_ref()).await?;
    let resolved_block = utils::ResolvedBlock {
        hash: format!("{:#x}", client_at_block.block_hash()),
        number: client_at_block.block_number(),
    };

    let raw_info = query_staking_info(
        &client_at_block,
        &account,
        &resolved_block,
        params.include_claimed_rewards,
        state.chain_info.ss58_prefix,
        &state.chain_info.spec_name,
    )
    .await?;

    let response = format_response(&raw_info, None, None, None);

    Ok(Json(response).into_response())
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(
    raw: &RawStakingInfo,
    rc_block_hash: Option<String>,
    rc_block_number: Option<String>,
    ah_timestamp: Option<String>,
) -> StakingInfoResponse {
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

    StakingInfoResponse {
        at: BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        controller: raw.controller.clone(),
        reward_destination,
        num_slashing_spans: raw.num_slashing_spans.to_string(),
        nominations,
        staking,
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
    params: StakingInfoQueryParams,
) -> Result<Response, AccountsError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(AccountsError::UseRcBlockNotSupported);
    }

    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    let rc_rpc = state.get_relay_chain_rpc().await?;

    // Resolve RC block
    let rc_block_id = params
        .at
        .unwrap_or_else(|| "head".to_string())
        .parse::<utils::BlockId>()?;
    let rc_resolved =
        utils::resolve_block_with_rpc(&rc_rpc_client, &rc_rpc, Some(rc_block_id)).await?;

    // Find AH blocks
    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved).await?;

    if ah_blocks.is_empty() {
        return Ok(Json(json!([])).into_response());
    }

    let rc_block_hash = rc_resolved.hash.clone();
    let rc_block_number = rc_resolved.number.to_string();

    // Process all AH blocks concurrently
    let include_claimed_rewards = params.include_claimed_rewards;
    let results = futures::future::try_join_all(ah_blocks.into_iter().map(|ah_block| {
        let state = &state;
        let account = &account;
        let rc_block_hash = &rc_block_hash;
        let rc_block_number = &rc_block_number;
        async move {
            let ah_resolved = utils::ResolvedBlock {
                hash: ah_block.hash.clone(),
                number: ah_block.number,
            };
            let client_at_block = state.client.at_block(ah_resolved.number).await?;
            let raw_info = query_staking_info(
                &client_at_block,
                account,
                &ah_resolved,
                include_claimed_rewards,
                state.chain_info.ss58_prefix,
                &state.chain_info.spec_name,
            )
            .await?;

            let response = format_response(
                &raw_info,
                Some(rc_block_hash.clone()),
                Some(rc_block_number.clone()),
                fetch_block_timestamp(&client_at_block).await,
            );

            Ok::<_, AccountsError>(response)
        }
    }))
    .await?;

    Ok(Json(results).into_response())
}
