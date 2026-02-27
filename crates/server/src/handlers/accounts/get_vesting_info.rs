// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{
    AccountsError, BlockInfo, VestingInfoQueryParams, VestingInfoResponse, VestingSchedule,
};
use super::utils::validate_and_parse_address;
use crate::extractors::JsonQuery;
use crate::handlers::common::accounts::{RawVestingInfo, query_vesting_info};
use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use polkadot_rest_api_config::ChainType;
use serde_json::json;
use sp_core::crypto::AccountId32;

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/vesting-info
///
/// Returns vesting information for a given account.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `includeClaimable` (optional): When true, calculate vested amounts
#[utoipa::path(
    get,
    path = "/v1/accounts/{accountId}/vesting-info",
    tag = "accounts",
    summary = "Account vesting info",
    description = "Returns vesting information for a given account including vesting schedules and locked amounts.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier"),
        ("includeClaimable" = Option<bool>, description = "When true, calculate vested amounts")
    ),
    responses(
        (status = 200, description = "Vesting information", body = VestingInfoResponse),
        (status = 400, description = "Invalid parameters"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_vesting_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    JsonQuery(params): JsonQuery<VestingInfoQueryParams>,
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id, state.chain_info.ss58_prefix)?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let client_at_block = utils::resolve_client_at_block(&state.client, params.at.as_ref()).await?;
    let resolved_block = utils::ResolvedBlock {
        hash: format!("{:#x}", client_at_block.block_hash()),
        number: client_at_block.block_number(),
    };

    let raw_info = query_vesting_info(&client_at_block, &account, &resolved_block).await?;

    let response = format_response(&raw_info, None, None, None);

    Ok(Json(response).into_response())
}

// ================================================================================================
// Response Formatting
// ================================================================================================

fn format_response(
    raw: &RawVestingInfo,
    rc_block_hash: Option<String>,
    rc_block_number: Option<String>,
    ah_timestamp: Option<String>,
) -> VestingInfoResponse {
    let schedules = raw
        .schedules
        .iter()
        .map(|s| VestingSchedule {
            locked: s.locked.clone(),
            per_block: s.per_block.clone(),
            starting_block: s.starting_block.clone(),
        })
        .collect();

    VestingInfoResponse {
        at: BlockInfo {
            hash: raw.block.hash.clone(),
            height: raw.block.number.to_string(),
        },
        vesting: schedules,
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
    params: VestingInfoQueryParams,
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
    let rc_block_number_str = rc_resolved.number.to_string();

    // Process all AH blocks concurrently
    let results = futures::future::try_join_all(ah_blocks.into_iter().map(|ah_block| {
        let state = &state;
        let account = &account;
        let rc_block_hash = &rc_block_hash;
        let rc_block_number_str = &rc_block_number_str;
        async move {
            let ah_resolved = utils::ResolvedBlock {
                hash: ah_block.hash.clone(),
                number: ah_block.number,
            };
            let client_at_block = state.client.at_block(ah_resolved.number).await?;
            let raw_info = query_vesting_info(&client_at_block, account, &ah_resolved).await?;

            let response = format_response(
                &raw_info,
                Some(rc_block_hash.clone()),
                Some(rc_block_number_str.clone()),
                fetch_block_timestamp(&client_at_block).await,
            );

            Ok::<_, AccountsError>(response)
        }
    }))
    .await?;

    Ok(Json(results).into_response())
}
