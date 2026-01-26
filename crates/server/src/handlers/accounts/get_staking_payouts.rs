use super::types::{
    AccountsError, BlockInfo, EraPayouts, EraPayoutsData, StakingPayoutsQueryParams,
    StakingPayoutsResponse, ValidatorPayout,
};
use super::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{
    RawEraPayouts, RawStakingPayouts, StakingPayoutsParams, query_staking_payouts,
};
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block, fetch_block_timestamp};
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

/// Handler for GET /accounts/{accountId}/staking-payouts
///
/// Returns staking payout information for a given account address.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `depth` (optional): Number of eras to query (default: 1)
/// - `era` (optional): The era to query at (default: active_era - 1)
/// - `unclaimedOnly` (optional): Only show unclaimed rewards (default: true)
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
pub async fn get_staking_payouts(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<StakingPayoutsQueryParams>,
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id)?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let block_id = params
        .at
        .clone()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
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

    let staking_params = StakingPayoutsParams {
        depth: params.depth,
        era: params.era,
        unclaimed_only: params.unclaimed_only,
    };

    let raw_payouts =
        query_staking_payouts(&client_at_block, &account, &resolved_block, &staking_params).await?;

    let response = format_response(&raw_payouts, None, None, None);

    Ok(Json(response).into_response())
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(
    raw: &RawStakingPayouts,
    rc_block_hash: Option<String>,
    rc_block_number: Option<String>,
    ah_timestamp: Option<String>,
) -> StakingPayoutsResponse {
    let eras_payouts = raw
        .eras_payouts
        .iter()
        .map(|era_payout| match era_payout {
            RawEraPayouts::Payouts(data) => EraPayouts::Payouts(EraPayoutsData {
                era: data.era,
                total_era_reward_points: data.total_era_reward_points.to_string(),
                total_era_payout: data.total_era_payout.to_string(),
                payouts: data
                    .payouts
                    .iter()
                    .map(|p| ValidatorPayout {
                        validator_id: p.validator_id.clone(),
                        nominator_staking_payout: p.nominator_staking_payout.to_string(),
                        claimed: p.claimed,
                        total_validator_reward_points: p.total_validator_reward_points.to_string(),
                        validator_commission: p.validator_commission.to_string(),
                        total_validator_exposure: p.total_validator_exposure.to_string(),
                        nominator_exposure: p.nominator_exposure.to_string(),
                    })
                    .collect(),
            }),
            RawEraPayouts::Message { message } => EraPayouts::Message {
                message: message.clone(),
            },
        })
        .collect();

    StakingPayoutsResponse {
        at: BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        eras_payouts,
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
    params: StakingPayoutsQueryParams,
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
        .clone()
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

    let staking_params = StakingPayoutsParams {
        depth: params.depth,
        era: params.era,
        unclaimed_only: params.unclaimed_only,
    };

    // Process each AH block
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };
        let client_at_block = state.client.at_block(ah_resolved.number).await?;
        let raw_payouts =
            query_staking_payouts(&client_at_block, &account, &ah_resolved, &staking_params)
                .await?;

        let response = format_response(
            &raw_payouts,
            Some(rc_block_hash.clone()),
            Some(rc_block_number.clone()),
            fetch_block_timestamp(&client_at_block).await,
        );

        results.push(response);
    }

    Ok(Json(results).into_response())
}
