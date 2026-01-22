use super::types::{
    BlockInfo, EraPayouts, EraPayoutsData, AccountsError, RcStakingPayoutsQueryParams,
    RcStakingPayoutsResponse, ValidatorPayout,
};
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{
    query_staking_payouts as query_staking_payouts_shared, RawEraPayouts, RawStakingPayouts,
    StakingPayoutsParams,
};
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

/// Handler for GET /rc/accounts/{accountId}/staking-payouts
///
/// Returns staking payout information for a given account address on the relay chain.
/// This endpoint always queries the relay chain directly.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `depth` (optional): Number of eras to query (default: 1)
/// - `era` (optional): The era to query at (default: active_era - 1)
/// - `unclaimedOnly` (optional): Only show unclaimed rewards (default: true)
pub async fn get_staking_payouts(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<RcStakingPayoutsQueryParams>,
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

    let resolved_block = utils::resolve_block_with_rpc(rc_rpc_client, rc_rpc.as_ref(), block_id).await?;

    println!(
        "Fetching RC staking payouts for account {:?} at block {}",
        account, resolved_block.number
    );

    let staking_params = StakingPayoutsParams {
        depth: params.depth,
        era: params.era,
        unclaimed_only: params.unclaimed_only,
    };

    let raw_payouts = query_staking_payouts_shared(rc_client, &account, &resolved_block, &staking_params).await?;

    let response = format_response(&raw_payouts);

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

fn format_response(raw: &RawStakingPayouts) -> RcStakingPayoutsResponse {
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

    RcStakingPayoutsResponse {
        at: BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        eras_payouts,
    }
}
