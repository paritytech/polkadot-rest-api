// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{
    AccountsError, RcVestingInfoQueryParams, RcVestingInfoResponse, RelayChainAccess,
    VestingSchedule,
};
use crate::extractors::JsonQuery;
use crate::handlers::accounts::utils::validate_and_parse_address;
use crate::handlers::common::accounts::{RawVestingInfo, query_vesting_info};
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

/// Handler for GET /rc/accounts/{accountId}/vesting-info
///
/// Returns vesting information for a given account on the relay chain.
/// This endpoint always queries the relay chain directly.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `includeClaimable` (optional): When true, calculate vested amounts
#[utoipa::path(
    get,
    path = "/v1/rc/accounts/{accountId}/vesting-info",
    tag = "rc",
    summary = "RC get vesting info",
    description = "Returns vesting information for a given account on the relay chain.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address"),
        ("at" = Option<String>, description = "Block identifier (number or hash)"),
        ("includeClaimable" = Option<bool>, description = "When true, calculate vested amounts")
    ),
    responses(
        (status = 200, description = "Vesting information", body = RcVestingInfoResponse),
        (status = 400, description = "Invalid account address"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_vesting_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    JsonQuery(params): JsonQuery<RcVestingInfoQueryParams>,
) -> Result<Response, AccountsError> {
    // Get the relay chain ss58_prefix for address validation
    let rc_ss58_prefix = get_relay_chain_ss58_prefix(&state)?;
    let account = validate_and_parse_address(&account_id, rc_ss58_prefix)?;

    // Get the relay chain client and info
    let (rc_client, rc_rpc_client, rc_rpc) = get_relay_chain_access(&state).await?;

    // Resolve block on relay chain
    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;

    let resolved_block =
        utils::resolve_block_with_rpc(&rc_rpc_client, rc_rpc.as_ref(), block_id).await?;
    let client_at_block = rc_client.at_block(resolved_block.number).await?;

    let raw_info = query_vesting_info(&client_at_block, &account, &resolved_block).await?;

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
        .ok_or(AccountsError::RelayChain(RelayChainError::NotConfigured))
}

/// Get access to relay chain client and RPC
async fn get_relay_chain_access(state: &AppState) -> Result<RelayChainAccess<'_>, AccountsError> {
    // If we're connected directly to a relay chain, use the primary client
    if state.chain_info.chain_type == ChainType::Relay {
        return Ok((
            &state.client,
            state.rpc_client.clone(),
            state.legacy_rpc.clone(),
        ));
    }

    // Otherwise, we need the relay chain client (for Asset Hub or parachain)
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(RelayChainError::NotConfigured)?;

    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;

    let relay_rpc = state.get_relay_chain_rpc().await?;

    Ok((relay_client, relay_rpc_client, relay_rpc))
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(raw: &RawVestingInfo) -> RcVestingInfoResponse {
    let schedules = raw
        .schedules
        .iter()
        .map(|s| VestingSchedule {
            locked: s.locked.clone(),
            per_block: s.per_block.clone(),
            starting_block: s.starting_block.clone(),
        })
        .collect();

    RcVestingInfoResponse {
        at: super::types::BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        vesting: schedules,
    }
}
