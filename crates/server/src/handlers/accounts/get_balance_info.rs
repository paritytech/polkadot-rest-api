// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{
    AccountsError, BalanceInfoQueryParams, BalanceInfoResponse, BalanceLock, BlockInfo,
};
use super::utils::validate_and_parse_address;
use crate::extractors::JsonQuery;
use crate::handlers::common::accounts::{
    RawBalanceInfo, format_balance, format_frozen_fields, format_locks, format_transferable,
    query_balance_info,
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

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/balance-info
///
/// Returns balance information for a given account.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `token` (optional): Token symbol for chains with multiple tokens (defaults to native)
/// - `denominated` (optional): When true, denominate balances using chain decimals
#[utoipa::path(
    get,
    path = "/v1/accounts/{accountId}/balance-info",
    tag = "accounts",
    summary = "Account balance info",
    description = "Returns balance information for a given account including free, reserved, and locked balances.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier"),
        ("token" = Option<String>, description = "Token symbol for chains with multiple tokens"),
        ("denominated" = Option<bool>, description = "When true, denominate balances using chain decimals")
    ),
    responses(
        (status = 200, description = "Account balance information", body = BalanceInfoResponse),
        (status = 400, description = "Invalid account or block parameter"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_balance_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    JsonQuery(params): JsonQuery<BalanceInfoQueryParams>,
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id, state.chain_info.ss58_prefix)?;

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

    let raw_info = query_balance_info(
        &client_at_block,
        &state.chain_info.spec_name,
        &account,
        &resolved_block,
        params.token.clone(),
    )
    .await?;

    let denominated = params.denominated.unwrap_or(false);
    let response = format_response(&raw_info, denominated, None, None, None);

    Ok(Json(response).into_response())
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(
    raw: &RawBalanceInfo,
    denominated: bool,
    rc_block_hash: Option<String>,
    rc_block_number: Option<String>,
    ah_timestamp: Option<String>,
) -> BalanceInfoResponse {
    let (misc_frozen, fee_frozen, frozen) =
        format_frozen_fields(&raw.account_data, denominated, raw.token_decimals);

    let formatted_locks: Vec<BalanceLock> =
        format_locks(&raw.locks, denominated, raw.token_decimals)
            .into_iter()
            .map(|l| BalanceLock {
                id: l.id,
                amount: l.amount,
                reasons: l.reasons,
            })
            .collect();

    BalanceInfoResponse {
        at: BlockInfo {
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
    account: sp_core::crypto::AccountId32,
    params: BalanceInfoQueryParams,
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
        .clone()
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

    // Process each AH block
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };
        let client_at_block = state.client.at_block(ah_resolved.number).await?;

        let raw_info = query_balance_info(
            &client_at_block,
            &state.chain_info.spec_name,
            &account,
            &ah_resolved,
            params.token.clone(),
        )
        .await?;

        let denominated = params.denominated.unwrap_or(false);
        let response = format_response(
            &raw_info,
            denominated,
            Some(rc_block_hash.clone()),
            Some(rc_block_number.clone()),
            fetch_block_timestamp(&client_at_block).await,
        );

        results.push(response);
    }

    Ok(Json(results).into_response())
}
