// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{
    AccountsError, BlockInfo, PoolAssetApprovalQueryParams, PoolAssetApprovalResponse,
};
use super::utils::validate_and_parse_address;
use crate::extractors::JsonQuery;
use crate::handlers::runtime_queries::pool_assets as pool_assets_queries;
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

/// Handler for GET /accounts/{accountId}/pool-asset-approvals
///
/// Returns pool asset approval information for a given account, asset, and delegate.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `assetId` (required): The pool asset ID to query approval for
/// - `delegate` (required): The delegate address with spending approval
#[utoipa::path(
    get,
    path = "/v1/accounts/{accountId}/pool-asset-approvals",
    tag = "accounts",
    summary = "Account pool asset approvals",
    description = "Returns pool asset approval information for a given account, asset, and delegate.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier"),
        ("assetId" = String, Query, description = "The pool asset ID to query approval for"),
        ("delegate" = String, Query, description = "The delegate address with spending approval")
    ),
    responses(
        (status = 200, description = "Pool asset approval information", body = PoolAssetApprovalResponse),
        (status = 400, description = "Invalid parameters"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_pool_asset_approvals(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    JsonQuery(params): JsonQuery<PoolAssetApprovalQueryParams>,
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id, state.chain_info.ss58_prefix)?;

    let delegate = validate_and_parse_address(&params.delegate, state.chain_info.ss58_prefix)
        .map_err(|_| AccountsError::InvalidDelegateAddress(params.delegate.clone()))?;

    if params.use_rc_block.unwrap_or(false) {
        return handle_use_rc_block(state, account, delegate, params).await;
    }

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    let client_at_block = utils::resolve_client_at_block(&state.client, params.at.as_ref()).await?;

    let response = query_pool_asset_approval(
        &client_at_block,
        &account,
        &delegate,
        params.asset_id,
        &resolved_block,
    )
    .await?;

    Ok(Json(response).into_response())
}

async fn query_pool_asset_approval(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    owner: &AccountId32,
    delegate: &AccountId32,
    asset_id: u32,
    block: &utils::ResolvedBlock,
) -> Result<PoolAssetApprovalResponse, AccountsError> {
    // Check if the pallet exists
    if !pool_assets_queries::is_pool_assets_pallet_available(client_at_block) {
        return Err(AccountsError::PalletNotAvailable("PoolAssets".to_string()));
    }

    // Use centralized query function
    let approval =
        pool_assets_queries::get_pool_asset_approval(client_at_block, asset_id, owner, delegate)
            .await
            .map_err(|_| {
                AccountsError::DecodeFailed(parity_scale_codec::Error::from(
                    "Failed to query pool asset approval",
                ))
            })?;

    let (amount, deposit) = match approval {
        Some(a) => (Some(a.amount), Some(a.deposit)),
        None => (None, None),
    };

    Ok(PoolAssetApprovalResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        amount,
        deposit,
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
    delegate: AccountId32,
    params: PoolAssetApprovalQueryParams,
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
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };

        let client_at_block = state.client.at_block(ah_resolved.number).await?;
        let mut response = query_pool_asset_approval(
            &client_at_block,
            &account,
            &delegate,
            params.asset_id,
            &ah_resolved,
        )
        .await?;

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
