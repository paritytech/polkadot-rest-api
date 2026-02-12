use super::types::{
    AccountsError, BlockInfo, ForeignAssetBalancesQueryParams, ForeignAssetBalancesResponse,
};
use super::utils::{
    parse_foreign_asset_locations, query_all_foreign_asset_locations, query_foreign_assets,
    validate_and_parse_address,
};
use crate::state::AppState;
use crate::utils::{self, fetch_block_timestamp, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use serde_json::json;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/foreign-asset-balances
///
/// Returns foreign asset balances for a given account on Asset Hub chains.
/// Foreign assets use XCM MultiLocation as their identifier.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `foreignAssets` (optional): List of multilocation JSON strings to filter by
#[utoipa::path(
    get,
    path = "/v1/accounts/{accountId}/foreign-asset-balances",
    tag = "accounts",
    summary = "Account foreign asset balances",
    description = "Returns foreign asset balances for a given account on Asset Hub chains. Foreign assets use XCM MultiLocation as their identifier.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier"),
        ("foreignAssets" = Option<Vec<String>>, Query, description = "List of multilocation JSON strings to filter by")
    ),
    responses(
        (status = 200, description = "Foreign asset balances", body = Object),
        (status = 400, description = "Invalid account or parameters"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_foreign_asset_balances(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<ForeignAssetBalancesQueryParams>,
) -> Result<Response, AccountsError> {
    let account = validate_and_parse_address(&account_id)?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
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

    let response = query_foreign_asset_balances(
        &client_at_block,
        &account,
        &resolved_block,
        &params.foreign_assets,
    )
    .await?;
    Ok(Json(response).into_response())
}

// ================================================================================================
// Query Logic
// ================================================================================================

async fn query_foreign_asset_balances(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &utils::ResolvedBlock,
    foreign_assets_filter: &[String],
) -> Result<ForeignAssetBalancesResponse, AccountsError> {
    // Check pallet exists
    let pallet_exists = client_at_block
        .storage()
        .entry(("ForeignAssets", "Account"))
        .is_ok();
    if !pallet_exists {
        return Err(AccountsError::PalletNotAvailable(
            "ForeignAssets".to_string(),
        ));
    }

    // Determine which locations to query
    let locations_to_query = if foreign_assets_filter.is_empty() {
        // No filter: query all registered foreign assets
        query_all_foreign_asset_locations(client_at_block).await?
    } else {
        // Parse each JSON string as a Location
        parse_foreign_asset_locations(foreign_assets_filter)?
    };

    let foreign_assets =
        query_foreign_assets(client_at_block, account, &locations_to_query).await?;

    Ok(ForeignAssetBalancesResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        foreign_assets,
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
    params: ForeignAssetBalancesQueryParams,
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
    let foreign_assets_filter = params.foreign_assets;
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };

        let client_at_block = state.client.at_block(ah_resolved.number).await?;
        let mut response = query_foreign_asset_balances(
            &client_at_block,
            &account,
            &ah_resolved,
            &foreign_assets_filter,
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
