// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{
    AccountsError, BlockInfo, DecodedPoolAssetApproval, PoolAssetApprovalQueryParams,
    PoolAssetApprovalResponse,
};
use super::utils::validate_and_parse_address;
use crate::extractors::JsonQuery;
use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use parity_scale_codec::Decode;
use polkadot_rest_api_config::ChainType;
use serde_json::json;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// SCALE Decode Types for PoolAssets::Approvals storage
// ================================================================================================

/// Pool asset approval structure
#[derive(Debug, Clone, Decode)]
struct PoolAssetApproval {
    amount: u128,
    deposit: u128,
}

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

    let client_at_block = match params.at {
        None => state.client.at_current_block().await?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<utils::BlockId>()?;
            match block_id {
                utils::BlockId::Hash(hash) => state.client.at_block(hash).await?,
                utils::BlockId::Number(number) => state.client.at_block(number).await?,
            }
        }
    };
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
    // Build the storage address for PoolAssets::Approvals(asset_id, owner, delegate)
    let storage_addr = subxt::dynamic::storage::<_, ()>("PoolAssets", "Approvals");

    // Check if the pallet exists by trying to create the storage entry
    if client_at_block
        .storage()
        .entry(("PoolAssets", "Approvals"))
        .is_err()
    {
        return Err(AccountsError::PalletNotAvailable("PoolAssets".to_string()));
    }

    // Storage key for Approvals: (asset_id, owner, delegate)
    let owner_bytes: [u8; 32] = *owner.as_ref();
    let delegate_bytes: [u8; 32] = *delegate.as_ref();

    let storage_value = client_at_block
        .storage()
        .fetch(storage_addr, (asset_id, owner_bytes, delegate_bytes))
        .await;

    let (amount, deposit) = if let Ok(value) = storage_value {
        // Get raw bytes and decode
        let raw_bytes = value.into_bytes();
        let decoded = decode_pool_asset_approval(&raw_bytes)?;
        match decoded {
            Some(approval) => (
                Some(approval.amount.to_string()),
                Some(approval.deposit.to_string()),
            ),
            None => (None, None),
        }
    } else {
        (None, None)
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
// Pool Asset Approval Decoding
// ================================================================================================

/// Decode pool asset approval from raw SCALE bytes
fn decode_pool_asset_approval(
    raw_bytes: &[u8],
) -> Result<Option<DecodedPoolAssetApproval>, AccountsError> {
    // Decode as PoolAssetApproval struct
    if let Ok(approval) = PoolAssetApproval::decode(&mut &raw_bytes[..]) {
        return Ok(Some(DecodedPoolAssetApproval {
            amount: approval.amount,
            deposit: approval.deposit,
        }));
    }

    // If decoding fails, return an error
    Err(AccountsError::DecodeFailed(
        parity_scale_codec::Error::from("Failed to decode pool asset approval: unknown format"),
    ))
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

    let rc_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(AccountsError::RelayChainNotConfigured)?;
    let rc_rpc = state
        .get_relay_chain_rpc()
        .ok_or(AccountsError::RelayChainNotConfigured)?;

    // Resolve RC block
    let rc_block_id = params
        .at
        .unwrap_or_else(|| "head".to_string())
        .parse::<utils::BlockId>()?;
    let rc_resolved =
        utils::resolve_block_with_rpc(rc_rpc_client, rc_rpc, Some(rc_block_id)).await?;

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
