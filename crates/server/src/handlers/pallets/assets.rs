// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for /pallets/assets/{assetId}/asset-info endpoint.

use crate::extractors::JsonQuery;
use crate::handlers::pallets::common::{
    AssetDetails, AssetMetadataStorage, AtResponse, ClientAtBlock, PalletError, format_account_id,
    resolve_block_for_pallet,
};
use crate::state::AppState;
use crate::utils::{
    BlockId, DEFAULT_CONCURRENCY, fetch_block_timestamp, rc_block::find_ah_blocks_in_rc_block,
    resolve_block_with_rpc, run_with_concurrency,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use polkadot_rest_api_config::ChainType;
use serde::{Deserialize, Serialize};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

#[utoipa::path(
    get,
    path = "/v1/pallets/assets/{assetId}/asset-info",
    tag = "pallets",
    summary = "Asset info",
    description = "Returns details for a specific asset including supply, admin, and metadata.",
    params(
        ("assetId" = String, Path, description = "Asset ID"),
        ("at" = Option<String>, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Asset information", body = Object),
        (status = 404, description = "Asset not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_assets_asset_info(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    JsonQuery(params): JsonQuery<AssetsQueryParams>,
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
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    state.get_relay_chain_client().await?;

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<BlockId>()?;

    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    let rc_rpc = state.get_relay_chain_rpc().await?;

    let rc_resolved_block =
        resolve_block_with_rpc(&rc_rpc_client, &rc_rpc, Some(rc_block_id)).await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    if ah_blocks.is_empty() {
        return Ok((
            StatusCode::OK,
            Json(Vec::<PalletsAssetsInfoResponse>::new()),
        )
            .into_response());
    }

    let ss58_prefix = state.chain_info.ss58_prefix;
    let rc_hash = rc_resolved_block.hash.clone();
    let rc_number = rc_resolved_block.number.to_string();

    let futures = ah_blocks.iter().map(|ah_block| {
        let state = state.clone();
        let rc_hash = rc_hash.clone();
        let rc_number = rc_number.clone();
        let ah_block_hash = ah_block.hash.clone();
        let ah_block_number = ah_block.number;

        async move {
            let client_at_block = state.client.at_block(ah_block_number).await?;

            let at = AtResponse {
                hash: ah_block_hash,
                height: ah_block_number.to_string(),
            };

            let (ah_timestamp, asset_info, asset_meta_data) = tokio::join!(
                fetch_block_timestamp(&client_at_block),
                fetch_asset_info(&client_at_block, asset_id, ss58_prefix),
                fetch_asset_metadata(&client_at_block, asset_id)
            );

            if asset_info.is_none() && asset_meta_data.is_none() {
                return Err(PalletError::AssetNotFoundAtBlock {
                    asset_id: asset_id.to_string(),
                    block_number: ah_block_number.to_string(),
                });
            }

            Ok(PalletsAssetsInfoResponse {
                at,
                asset_info,
                asset_meta_data,
                rc_block_hash: Some(rc_hash),
                rc_block_number: Some(rc_number),
                ah_timestamp,
            })
        }
    });

    let responses = run_with_concurrency(DEFAULT_CONCURRENCY, futures).await?;

    Ok((StatusCode::OK, Json(responses)).into_response())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assets_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<AssetsQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
