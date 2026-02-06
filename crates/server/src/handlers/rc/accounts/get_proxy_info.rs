use super::types::{AccountsError, RcProxyInfoQueryParams, RcProxyInfoResponse, RelayChainAccess};
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{RawProxyInfo, query_proxy_info};
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

/// Handler for GET /rc/accounts/{accountId}/proxy-info
///
/// Returns proxy information for a given account on the relay chain.
/// This endpoint always queries the relay chain directly.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
#[utoipa::path(
    get,
    path = "/v1/rc/accounts/{accountId}/proxy-info",
    tag = "rc",
    summary = "RC get proxy info",
    description = "Returns proxy information for a given account on the relay chain.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address"),
        ("at" = Option<String>, Query, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Proxy information", body = RcProxyInfoResponse),
        (status = 400, description = "Invalid account address"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_proxy_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<RcProxyInfoQueryParams>,
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

    let raw_info = query_proxy_info(&client_at_block, &account, &resolved_block).await?;

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
