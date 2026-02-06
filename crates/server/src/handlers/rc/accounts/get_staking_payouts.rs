use super::types::{
    AccountsError, BlockInfo, EraPayouts, EraPayoutsData, RcStakingPayoutsQueryParams,
    RcStakingPayoutsResponse, RelayChainAccess, ValidatorPayout,
};
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{
    RawEraPayouts, RawStakingPayouts, StakingPayoutsParams, query_staking_payouts,
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
#[utoipa::path(
    get,
    path = "/v1/rc/accounts/{accountId}/staking-payouts",
    tag = "rc",
    summary = "RC get staking payouts",
    description = "Returns staking payout information for a given account on the relay chain.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address"),
        ("at" = Option<String>, Query, description = "Block identifier (number or hash)"),
        ("depth" = Option<u32>, Query, description = "Number of eras to query (default: 1)"),
        ("era" = Option<u32>, Query, description = "The era to query at (default: active_era - 1)"),
        ("unclaimedOnly" = Option<bool>, Query, description = "Only show unclaimed rewards (default: true)")
    ),
    responses(
        (status = 200, description = "Staking payouts", body = RcStakingPayoutsResponse),
        (status = 400, description = "Invalid account address"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_staking_payouts(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<RcStakingPayoutsQueryParams>,
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

    let staking_params = StakingPayoutsParams {
        depth: params.depth,
        era: params.era,
        unclaimed_only: params.unclaimed_only,
    };

    let raw_payouts =
        query_staking_payouts(&client_at_block, &account, &resolved_block, &staking_params).await?;

    let response = format_response(&raw_payouts);

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
