use super::types::{RcProxyInfoError, RcProxyInfoQueryParams, RcProxyInfoResponse};
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{query_proxy_info as query_proxy_info_shared, RawProxyInfo};
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

/// Handler for GET /rc/accounts/{accountId}/proxy-info
///
/// Returns proxy information for a given account on the relay chain.
/// This endpoint always queries the relay chain directly.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
pub async fn get_proxy_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<RcProxyInfoQueryParams>,
) -> Result<Response, RcProxyInfoError> {
    let account = validate_and_parse_address(&account_id)
        .map_err(|_| RcProxyInfoError::InvalidAddress(account_id.clone()))?;

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
        "Fetching RC proxy info for account {:?} at block {}",
        account, resolved_block.number
    );

    let raw_info = query_proxy_info_shared(rc_client, &account, &resolved_block).await?;

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
    RcProxyInfoError,
> {
    // If we're connected directly to a relay chain, use the primary client
    if state.chain_info.chain_type == ChainType::Relay {
        return Ok((&state.client, &state.rpc_client, &state.legacy_rpc));
    }

    // Otherwise, we need the relay chain client (for Asset Hub or parachain)
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(RcProxyInfoError::RelayChainNotAvailable)?;

    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(RcProxyInfoError::RelayChainNotAvailable)?;

    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(RcProxyInfoError::RelayChainNotAvailable)?;

    Ok((relay_client, relay_rpc_client, relay_rpc))
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(raw: &RawProxyInfo) -> RcProxyInfoResponse {
    let delegated_accounts = raw
        .delegated_accounts
        .iter()
        .map(|def| super::types::ProxyDefinition {
            delegate: def.delegate.clone(),
            proxy_type: def.proxy_type.clone(),
            delay: def.delay.clone(),
        })
        .collect();

    RcProxyInfoResponse {
        at: super::types::BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        delegated_accounts,
        deposit_held: raw.deposit_held.clone(),
    }
}
