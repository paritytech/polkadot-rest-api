//! Handler for /pallets/assets/{assetId}/asset-info endpoint.

use crate::handlers::pallets::common::{
    AssetDetails, AssetMetadataStorage, AtResponse, ClientAtBlock, PalletError,
    build_rc_block_fields, format_account_id, resolve_block_for_pallet,
    validate_and_resolve_rc_block,
};
use crate::state::AppState;
use crate::utils::{ResolvedBlock, fetch_block_timestamp};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub use_rc_block: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetInfo {
    pub owner: String,
    pub issuer: String,
    pub admin: String,
    pub freezer: String,
    pub supply: String,
    pub deposit: String,
    pub min_balance: String,
    pub is_sufficient: bool,
    pub accounts: String,
    pub sufficients: String,
    pub approvals: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetMetadata {
    pub deposit: String,
    pub name: String,
    pub symbol: String,
    pub decimals: String,
    pub is_frozen: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsAssetsInfoResponse {
    pub at: AtResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_info: Option<AssetInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_meta_data: Option<AssetMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// ============================================================================
// Main Handler
// ============================================================================

pub async fn pallets_assets_asset_info(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    Query(params): Query<AssetsQueryParams>,
) -> Result<Response, PalletError> {
    let asset_id: u32 = asset_id
        .parse()
        .map_err(|_| PalletError::AssetNotFound(format!("Invalid asset ID: {}", asset_id)))?;

    if params.use_rc_block {
        return handle_use_rc_block(state, asset_id, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let ss58_prefix = state.chain_info.ss58_prefix;
    let asset_info = fetch_asset_info(&resolved.client_at_block, asset_id, ss58_prefix).await;
    let asset_meta_data = fetch_asset_metadata(&resolved.client_at_block, asset_id).await;

    if asset_info.is_none() && asset_meta_data.is_none() {
        return Err(PalletError::AssetNotFound(asset_id.to_string()));
    }

    Ok((
        StatusCode::OK,
        Json(PalletsAssetsInfoResponse {
            at: resolved.at,
            asset_info,
            asset_meta_data,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

// ============================================================================
// RC Block Handler
// ============================================================================

async fn handle_use_rc_block(
    state: AppState,
    asset_id: u32,
    params: AssetsQueryParams,
) -> Result<Response, PalletError> {
    // Validate and resolve RC block using the common helper
    let rc_context = validate_and_resolve_rc_block(&state, params.at.as_ref()).await?;

    if rc_context.ah_blocks.is_empty() {
        return Ok(build_empty_rc_response(&rc_context.rc_resolved_block));
    }

    let ah_block = &rc_context.ah_blocks[0];
    let client_at_block = state.client.at_block(ah_block.number).await?;

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    let ah_timestamp = fetch_block_timestamp(&client_at_block).await;
    let rc_fields = build_rc_block_fields(&rc_context.rc_resolved_block, ah_timestamp);
    let ss58_prefix = state.chain_info.ss58_prefix;
    let asset_info = fetch_asset_info(&client_at_block, asset_id, ss58_prefix).await;
    let asset_meta_data = fetch_asset_metadata(&client_at_block, asset_id).await;

    if asset_info.is_none() && asset_meta_data.is_none() {
        return Err(PalletError::AssetNotFound(asset_id.to_string()));
    }

    Ok((
        StatusCode::OK,
        Json(PalletsAssetsInfoResponse {
            at,
            asset_info,
            asset_meta_data,
            rc_block_hash: rc_fields.rc_block_hash,
            rc_block_number: rc_fields.rc_block_number,
            ah_timestamp: rc_fields.ah_timestamp,
        }),
    )
        .into_response())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Fetches asset details from Assets::Asset storage.
async fn fetch_asset_info(
    client_at_block: &ClientAtBlock,
    asset_id: u32,
    ss58_prefix: u16,
) -> Option<AssetInfo> {
    // Query Assets pallet with typed return
    let asset_addr = subxt::dynamic::storage::<_, AssetDetails>("Assets", "Asset");
    let details = client_at_block
        .storage()
        .fetch(asset_addr, (asset_id,))
        .await
        .ok()?
        .decode()
        .ok()?;

    Some(AssetInfo {
        owner: format_account_id(&details.owner, ss58_prefix),
        issuer: format_account_id(&details.issuer, ss58_prefix),
        admin: format_account_id(&details.admin, ss58_prefix),
        freezer: format_account_id(&details.freezer, ss58_prefix),
        supply: details.supply.to_string(),
        deposit: details.deposit.to_string(),
        min_balance: details.min_balance.to_string(),
        is_sufficient: details.is_sufficient,
        accounts: details.accounts.to_string(),
        sufficients: details.sufficients.to_string(),
        approvals: details.approvals.to_string(),
        status: details.status.as_str().to_string(),
    })
}

/// Fetches asset metadata from Assets::Metadata storage.
async fn fetch_asset_metadata(
    client_at_block: &ClientAtBlock,
    asset_id: u32,
) -> Option<AssetMetadata> {
    // Query Assets pallet with typed return
    let metadata_addr = subxt::dynamic::storage::<_, AssetMetadataStorage>("Assets", "Metadata");
    let metadata = client_at_block
        .storage()
        .fetch(metadata_addr, (asset_id,))
        .await
        .ok()?
        .decode()
        .ok()?;

    Some(AssetMetadata {
        deposit: metadata.deposit.to_string(),
        name: format!("0x{}", hex::encode(&metadata.name)),
        symbol: format!("0x{}", hex::encode(&metadata.symbol)),
        decimals: metadata.decimals.to_string(),
        is_frozen: metadata.is_frozen,
    })
}

/// Builds an empty response when no AH blocks are found in the RC block.
fn build_empty_rc_response(rc_resolved_block: &ResolvedBlock) -> Response {
    let at = AtResponse {
        hash: rc_resolved_block.hash.clone(),
        height: rc_resolved_block.number.to_string(),
    };

    (
        StatusCode::OK,
        Json(PalletsAssetsInfoResponse {
            at,
            asset_info: None,
            asset_meta_data: None,
            rc_block_hash: Some(rc_resolved_block.hash.clone()),
            rc_block_number: Some(rc_resolved_block.number.to_string()),
            ah_timestamp: None,
        }),
    )
        .into_response()
}
