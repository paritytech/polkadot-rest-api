// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{
    AccountsError, RcBalanceInfoQueryParams, RcBalanceInfoResponse, RelayChainAccessWithSpec,
};
use crate::extractors::JsonQuery;
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{
    RawBalanceInfo, format_balance, format_frozen_fields, format_locks, format_transferable,
    query_balance_info,
};
use crate::state::{AppState, RelayChainError};
use crate::utils;
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use polkadot_rest_api_config::ChainType;

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /rc/accounts/{accountId}/balance-info
///
/// Returns balance information for a given account on the relay chain.
/// This endpoint always queries the relay chain directly.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `token` (optional): Token symbol (defaults to native token)
/// - `denominated` (optional): When true, denominate balances using chain decimals
#[utoipa::path(
    get,
    path = "/v1/rc/accounts/{accountId}/balance-info",
    tag = "rc",
    summary = "RC get balance info",
    description = "Returns balance information for a given account on the relay chain.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address"),
        ("at" = Option<String>, description = "Block identifier (number or hash)"),
        ("token" = Option<String>, description = "Token symbol (defaults to native token)"),
        ("denominated" = Option<bool>, description = "Denominate balances using chain decimals")
    ),
    responses(
        (status = 200, description = "Balance information", body = RcBalanceInfoResponse),
        (status = 400, description = "Invalid account address"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_balance_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    JsonQuery(params): JsonQuery<RcBalanceInfoQueryParams>,
) -> Result<Response, AccountsError> {
    // Get the relay chain ss58_prefix for address validation
    let rc_ss58_prefix = get_relay_chain_ss58_prefix(&state)?;
    let account = validate_and_parse_address(&account_id, rc_ss58_prefix)?;

    // Get the relay chain client and info
    let (rc_client, rc_rpc_client, rc_rpc, rc_spec_name) = get_relay_chain_access(&state).await?;

    // Resolve block on relay chain
    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;

    let resolved_block =
        utils::resolve_block_with_rpc(&rc_rpc_client, rc_rpc.as_ref(), block_id).await?;
    let client_at_block = rc_client.at_block(resolved_block.number).await?;
    let raw_info = query_balance_info(
        &client_at_block,
        &rc_spec_name,
        &account,
        &resolved_block,
        params.token.clone(),
    )
    .await?;

    let response = format_response(&raw_info, params.denominated);

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
        .ok_or(AccountsError::RelayChain(RelayChainError::NotConfigured))
}

/// Get access to relay chain client and RPC
async fn get_relay_chain_access(state: &AppState) -> Result<RelayChainAccessWithSpec<'_>, AccountsError> {
    // If we're connected directly to a relay chain, use the primary client
    if state.chain_info.chain_type == ChainType::Relay {
        return Ok((
            &state.client,
            state.rpc_client.clone(),
            state.legacy_rpc.clone(),
            state.chain_info.spec_name.clone(),
        ));
    }

    // Otherwise, we need the relay chain client (for Asset Hub or parachain)
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(RelayChainError::NotConfigured)?;

    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;

    let relay_rpc = state.get_relay_chain_rpc().await?;

    let relay_spec_name = state
        .relay_chain_info
        .as_ref()
        .map(|info| info.spec_name.clone())
        .ok_or(AccountsError::RelayChain(RelayChainError::NotConfigured))?;

    Ok((relay_client, relay_rpc_client, relay_rpc, relay_spec_name))
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(raw: &RawBalanceInfo, denominated: bool) -> RcBalanceInfoResponse {
    let (misc_frozen, fee_frozen, frozen) =
        format_frozen_fields(&raw.account_data, denominated, raw.token_decimals);

    let formatted_locks = format_locks(&raw.locks, denominated, raw.token_decimals)
        .into_iter()
        .map(|l| super::types::BalanceLock {
            id: l.id,
            amount: l.amount,
            reasons: l.reasons,
        })
        .collect();

    RcBalanceInfoResponse {
        at: super::types::BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        nonce: raw.account_data.nonce.to_string(),
        token_symbol: raw.token_symbol.clone(),
        free: format_balance(raw.account_data.free, denominated, raw.token_decimals),
        reserved: format_balance(raw.account_data.reserved, denominated, raw.token_decimals),
        misc_frozen,
        fee_frozen,
        frozen,
        transferable: format_transferable(&raw.transferable, denominated, raw.token_decimals),
        locks: formatted_locks,
    }
}
