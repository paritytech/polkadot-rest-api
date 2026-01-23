use super::types::{
    BlockInfo, NominationsInfo, RewardDestination, AccountsError, StakingInfoQueryParams,
    StakingInfoResponse, StakingLedger,
};
use super::utils::validate_and_parse_address;
use crate::handlers::accounts::utils::fetch_timestamp;
use crate::handlers::common::accounts::{
    query_staking_info, DecodedRewardDestination, RawStakingInfo,
};
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
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
pub async fn get_staking_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<StakingInfoQueryParams>,
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id)?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let block_id = params.at.as_ref().map(|s| s.parse::<utils::BlockId>()).transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let client_at_block = match params.at {
        None => state.client.at_current_block().await?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<utils::BlockId>()?;
            match block_id {
                utils::BlockId::Hash(hash) => state.client.at_block(hash).await?,
                utils::BlockId::Number(number) => state.client.at_block(number).await?,
            }
        }
    };

    let raw_info = query_staking_info(&client_at_block, &account, &resolved_block).await?;

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
        DecodedRewardDestination::Account { account } => {
            RewardDestination::Account { account: account.clone() }
        }
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

    let rc_rpc_client = state.get_relay_chain_rpc_client()
        .ok_or(AccountsError::RelayChainNotConfigured)?;
    let rc_rpc = state.get_relay_chain_rpc()
        .ok_or(AccountsError::RelayChainNotConfigured)?;

    // Resolve RC block
    let rc_block_id = params
        .at
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
    let rc_block_number = rc_resolved.number.to_string();

    // Process each AH block
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };
        let client_at_block = state.client.at_block(ah_resolved.number).await?;
        let raw_info = query_staking_info(&client_at_block, &account, &ah_resolved).await?;

        let response = format_response(
            &raw_info,
            Some(rc_block_hash.clone()),
            Some(rc_block_number.clone()),
            fetch_timestamp(&client_at_block).await,
        );

        results.push(response);
    }

    Ok(Json(results).into_response())
}
