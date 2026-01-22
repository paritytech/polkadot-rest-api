use super::types::{
    BlockInfo, AccountsError, PoolAssetBalancesQueryParams,
    PoolAssetBalancesResponse,
};
use super::utils::{fetch_timestamp, query_all_pool_assets_id, query_pool_assets, validate_and_parse_address};
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use config::ChainType;
use serde_json::json;
use sp_core::crypto::AccountId32;

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/pool-asset-balances
///
/// Returns pool asset balances for a given account.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `assets` (optional): List of asset IDs to query (queries all if omitted)
pub async fn get_pool_asset_balances(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<PoolAssetBalancesQueryParams>,
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id)?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let block_id = params.at.map(|s| s.parse::<utils::BlockId>()).transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    println!(
        "Fetching pool asset balances for account {:?} at assets {:?}",
        account, &params.assets
    );

    let response =
        query_pool_asset_balances(&state, &account, &resolved_block, &params.assets).await?;

    Ok(Json(response).into_response())
}

async fn query_pool_asset_balances(
    state: &AppState,
    account: &AccountId32,
    block: &utils::ResolvedBlock,
    asset_ids: &[u32],
) -> Result<PoolAssetBalancesResponse, AccountsError> {
    let client_at_block = state.client.at(block.number).await?;

    let pool_assets_exists = client_at_block
        .storage()
        .entry("PoolAssets", "Account")
        .is_ok();

    if !pool_assets_exists {
        return Err(AccountsError::PalletNotAvailable("PoolAssets".to_string()));
    }

    // Determine which assets to query
    let assets_to_query = if asset_ids.is_empty() {
        // Query all pool asset IDs
        let assets = query_all_pool_assets_id(state, block.number).await;
        match assets {
            Ok(ids) => ids,
            Err(e) => {
                tracing::warn!("Failed to query all pool asset IDs: {e}");
                return Err(AccountsError::PalletNotAvailable("PoolAssets".to_string()));
            }
        }
    } else {
        asset_ids.to_vec()
    };

    // Query each pool asset balance in parallel
    let pool_assets = query_pool_assets(state, block.number, account, &assets_to_query).await?;

    Ok(PoolAssetBalancesResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        pool_assets,
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
    params: PoolAssetBalancesQueryParams,
) -> Result<Response, AccountsError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(AccountsError::UseRcBlockNotSupported);
    }

    let rc_rpc_client = state.get_relay_chain_rpc_client()
        .ok_or(AccountsError::RelayChainNotConfigured)?;
    let rc_rpc = state.get_relay_chain_rpc()
        .ok_or(AccountsError::RelayChainNotConfigured)?;

    // Resolve RC block
    let rc_block_id = params
        .at
        .unwrap_or_else(|| "head".to_string())
        .parse::<utils::BlockId>()?;
    let rc_resolved = utils::resolve_block_with_rpc(
        rc_rpc_client,
        rc_rpc,
        Some(rc_block_id),
    )
    .await?;

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

        let mut response =
            query_pool_asset_balances(&state, &account, &ah_resolved, &params.assets).await?;

        // Add RC block info
        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        // Fetch AH timestamp
        if let Ok(timestamp) = fetch_timestamp(&state, ah_block.number).await {
            response.ah_timestamp = Some(timestamp);
        }

        results.push(response);
    }

    Ok(Json(results).into_response())
}
