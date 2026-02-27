// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{AccountsError, AssetBalancesQueryParams, AssetBalancesResponse, BlockInfo};
use super::utils::validate_and_parse_address;
use crate::extractors::JsonQuery;
use crate::handlers::accounts::utils::{query_all_assets_id, query_assets};
use crate::handlers::runtime_queries::assets as assets_queries;
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
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/asset-balances
///
/// Returns asset balances for a given account.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `assets` (optional): List of asset IDs to query (queries all if omitted)
#[utoipa::path(
    get,
    path = "/v1/accounts/{accountId}/asset-balances",
    tag = "accounts",
    summary = "Account asset balances",
    description = "Returns asset balances for a given account on Asset Hub chains.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier"),
        ("assets" = Option<String>, description = "Comma-separated list of asset IDs to query")
    ),
    responses(
        (status = 200, description = "Account asset balances", body = AssetBalancesResponse),
        (status = 400, description = "Invalid parameters"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_asset_balances(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    JsonQuery(params): JsonQuery<AssetBalancesQueryParams>,
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

    let client_at_block = utils::resolve_client_at_block(&state.client, params.at.as_ref()).await?;

    let assets = params.assets.as_deref().unwrap_or(&[]);
    let response =
        query_asset_balances(&client_at_block, &account, &resolved_block, assets).await?;
    Ok(Json(response).into_response())
}

async fn query_asset_balances(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &utils::ResolvedBlock,
    asset_ids: &[u32],
) -> Result<AssetBalancesResponse, AccountsError> {
    // Check if Assets pallet is available using centralized function
    if !assets_queries::is_assets_pallet_available(client_at_block) {
        return Err(AccountsError::PalletNotAvailable("Assets".to_string()));
    }

    // Determine which assets to query
    let assets_to_query = if asset_ids.is_empty() {
        // Query all asset IDs using centralized function
        query_all_assets_id(client_at_block).await.map_err(|e| {
            tracing::warn!("Failed to query all asset IDs: {e}");
            AccountsError::PalletNotAvailable("Assets".to_string())
        })?
    } else {
        asset_ids.to_vec()
    };

    // Query each asset balance in parallel
    let assets = query_assets(client_at_block, account, &assets_to_query).await?;

    Ok(AssetBalancesResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        assets,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ================================================================================================
// Relay Chain Block Handling
// ================================================================================================

async fn handle_use_rc_block(
    state: AppState,
    account: AccountId32,
    params: AssetBalancesQueryParams,
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
    let assets = params.assets.as_deref().unwrap_or(&[]);
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };
        let client_at_block = state.client.at_block(ah_resolved.number).await?;
        let mut response =
            query_asset_balances(&client_at_block, &account, &ah_resolved, assets).await?;

        // Add RC block info
        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        // Fetch AH timestamp
        if let Some(timestamp) = fetch_block_timestamp(&client_at_block).await {
            response.ah_timestamp = Some(timestamp);
        }

        results.push(response);
    }

    Ok(Json(results).into_response())
}
