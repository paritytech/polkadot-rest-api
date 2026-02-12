use super::types::{
    AccountsError, BlockInfo, ClaimedReward, NominationsInfo, RewardDestination,
    StakingInfoQueryParams, StakingInfoResponse, StakingLedger,
};
use super::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{
    DecodedRewardDestination, RawStakingInfo, query_staking_info,
};
use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block};
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
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Staking information", body = StakingInfoResponse),
        (status = 400, description = "Invalid parameters"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_staking_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<StakingInfoQueryParams>,
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
    let client_at_block = state.client.at_block(resolved_block.number).await?;

    let raw_info = query_staking_info(
        &client_at_block,
        &account,
        &resolved_block,
        params.include_claimed_rewards,
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
        unlocking: unlocking_total.to_string(),
        claimed_rewards,
    };

    StakingInfoResponse {
        at: BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        controller: raw.controller.clone(),
        reward_destination,
        num_slashing_spans: raw.num_slashing_spans,
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

    let rc_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(AccountsError::RelayChainNotConfigured)?;
    let rc_rpc = state
        .get_relay_chain_rpc()
        .ok_or(AccountsError::RelayChainNotConfigured)?;

    // Resolve RC block
    let rc_block_id = params
        .at
        .unwrap_or_else(|| "head".to_string())
        .parse::<utils::BlockId>()?;
    let rc_resolved =
        utils::resolve_block_with_rpc(rc_rpc_client, rc_rpc, Some(rc_block_id)).await?;

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
        let client_at_block = state.client.at_block(ah_resolved.number).await?;
        let raw_info = query_staking_info(
            &client_at_block,
            &account,
            &ah_resolved,
            params.include_claimed_rewards,
        )
        .await?;

        let response = format_response(
            &raw_info,
            Some(rc_block_hash.clone()),
            Some(rc_block_number.clone()),
            fetch_block_timestamp(&client_at_block).await,
        );

        results.push(response);
    }

    Ok(Json(results).into_response())
}
