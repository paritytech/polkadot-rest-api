// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for /pallets/pool-assets/{assetId}/asset-info endpoint.
//!
//! This endpoint is nearly identical to /pallets/assets/{assetId}/asset-info
//! but queries the PoolAssets pallet instead of Assets. Pool assets are
//! LP (liquidity pool) tokens created by the AssetConversion pallet.

use crate::handlers::pallets::common::{
    AssetDetails, AssetMetadataStorage, AtResponse, ClientAtBlock, PalletError, format_account_id,
    resolve_block_for_pallet,
};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::{Path, Query, State},
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
pub struct PoolAssetsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub use_rc_block: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolAssetInfo {
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
pub struct PoolAssetMetadata {
    pub deposit: String,
    pub name: String,
    pub symbol: String,
    pub decimals: String,
    pub is_frozen: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsPoolAssetsInfoResponse {
    pub at: AtResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_asset_info: Option<PoolAssetInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_asset_meta_data: Option<PoolAssetMetadata>,
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
    path = "/v1/pallets/pool-assets/{assetId}/asset-info",
    tag = "pallets",
    summary = "Pool asset info",
    description = "Returns details for a specific pool asset including supply, admin, and metadata.",
    params(
        ("assetId" = String, Path, description = "Pool asset ID"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Pool asset information", body = Object),
        (status = 404, description = "Pool asset not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_pool_assets_asset_info(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    Query(params): Query<PoolAssetsQueryParams>,
) -> Result<Response, PalletError> {
    let asset_id: u32 = asset_id.parse().map_err(|_| {
        PalletError::PoolAssetNotFound(format!("Invalid pool asset ID: {}", asset_id))
    })?;

    if params.use_rc_block {
        return handle_use_rc_block(state, asset_id, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let ss58_prefix = state.chain_info.ss58_prefix;
    let pool_asset_info =
        fetch_pool_asset_info(&resolved.client_at_block, asset_id, ss58_prefix).await;
    let pool_asset_meta_data =
        fetch_pool_asset_meta_data(&resolved.client_at_block, asset_id).await;

    if pool_asset_info.is_none() && pool_asset_meta_data.is_none() {
        return Err(PalletError::PoolAssetNotFound(asset_id.to_string()));
    }

    Ok((
        StatusCode::OK,
        Json(PalletsPoolAssetsInfoResponse {
            at: resolved.at,
            pool_asset_info,
            pool_asset_meta_data,
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
    params: PoolAssetsQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(PalletError::RelayChainNotConfigured);
    }

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_resolved_block = utils::resolve_block_with_rpc(
        state
            .get_relay_chain_rpc_client()
            .expect("relay chain client checked above"),
        state
            .get_relay_chain_rpc()
            .expect("relay chain rpc checked above"),
        Some(rc_block_id),
    )
    .await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    // Return empty array when no AH blocks found (matching Sidecar behavior)
    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(serde_json::json!([]))).into_response());
    }

    let rc_block_number = rc_resolved_block.number.to_string();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let ss58_prefix = state.chain_info.ss58_prefix;

    // Process ALL AH blocks, not just the first one
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = fetch_timestamp(&client_at_block).await;
        let pool_asset_info = fetch_pool_asset_info(&client_at_block, asset_id, ss58_prefix).await;
        let pool_asset_meta_data = fetch_pool_asset_meta_data(&client_at_block, asset_id).await;

        // Skip blocks where the asset doesn't exist
        if pool_asset_info.is_none() && pool_asset_meta_data.is_none() {
            continue;
        }

        results.push(PalletsPoolAssetsInfoResponse {
            at,
            pool_asset_info,
            pool_asset_meta_data,
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        });
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Fetches pool asset details from PoolAssets::Asset storage.
async fn fetch_pool_asset_info(
    client_at_block: &ClientAtBlock,
    asset_id: u32,
    ss58_prefix: u16,
) -> Option<PoolAssetInfo> {
    // Query PoolAssets pallet with typed return
    let asset_addr = subxt::dynamic::storage::<_, AssetDetails>("PoolAssets", "Asset");
    let details = client_at_block
        .storage()
        .fetch(asset_addr, (asset_id,))
        .await
        .ok()?
        .decode()
        .ok()?;

    Some(PoolAssetInfo {
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

/// Fetches pool asset metadata from PoolAssets::Metadata storage.
async fn fetch_pool_asset_meta_data(
    client_at_block: &ClientAtBlock,
    asset_id: u32,
) -> Option<PoolAssetMetadata> {
    // Query PoolAssets pallet with typed return
    let metadata_addr =
        subxt::dynamic::storage::<_, AssetMetadataStorage>("PoolAssets", "Metadata");
    let metadata = client_at_block
        .storage()
        .fetch(metadata_addr, (asset_id,))
        .await
        .ok()?
        .decode()
        .ok()?;

    Some(PoolAssetMetadata {
        deposit: metadata.deposit.to_string(),
        name: format!("0x{}", hex::encode(&metadata.name)),
        symbol: format!("0x{}", hex::encode(&metadata.symbol)),
        decimals: metadata.decimals.to_string(),
        is_frozen: metadata.is_frozen,
    })
}

/// Fetches timestamp from Timestamp::Now storage.
async fn fetch_timestamp(client_at_block: &ClientAtBlock) -> Option<String> {
    let timestamp_addr = subxt::dynamic::storage::<(), u64>("Timestamp", "Now");
    let timestamp = client_at_block
        .storage()
        .fetch(timestamp_addr, ())
        .await
        .ok()?;
    let timestamp_value = timestamp.decode().ok()?;
    Some(timestamp_value.to_string())
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::pallets::common::AssetStatus;

    #[test]
    fn test_pool_asset_info_serialization() {
        let info = PoolAssetInfo {
            owner: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string(),
            issuer: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string(),
            admin: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string(),
            freezer: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string(),
            supply: "1000000000000".to_string(),
            deposit: "0".to_string(),
            min_balance: "1".to_string(),
            is_sufficient: false,
            accounts: "10".to_string(),
            sufficients: "0".to_string(),
            approvals: "0".to_string(),
            status: "Live".to_string(),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"owner\":"));
        assert!(json.contains("\"supply\":\"1000000000000\""));
        assert!(json.contains("\"isSufficient\":false"));
        assert!(json.contains("\"minBalance\":\"1\""));
    }

    #[test]
    fn test_pool_asset_meta_data_serialization() {
        let metadata = PoolAssetMetadata {
            deposit: "0".to_string(),
            name: "0x4c5020546f6b656e".to_string(),
            symbol: "0x4c50".to_string(),
            decimals: "12".to_string(),
            is_frozen: false,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"deposit\":\"0\""));
        assert!(json.contains("\"name\":\"0x4c5020546f6b656e\""));
        assert!(json.contains("\"isFrozen\":false"));
    }

    #[test]
    fn test_response_serialization_with_rc_block() {
        let response = PalletsPoolAssetsInfoResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "10260000".to_string(),
            },
            pool_asset_info: None,
            pool_asset_meta_data: None,
            rc_block_hash: Some("0xdef456".to_string()),
            rc_block_number: Some("28500000".to_string()),
            ah_timestamp: Some("1700000000000".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"rcBlockHash\":\"0xdef456\""));
        assert!(json.contains("\"rcBlockNumber\":\"28500000\""));
        assert!(json.contains("\"ahTimestamp\":\"1700000000000\""));
    }

    #[test]
    fn test_response_serialization_without_rc_block() {
        let response = PalletsPoolAssetsInfoResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "10260000".to_string(),
            },
            pool_asset_info: None,
            pool_asset_meta_data: None,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        // rc_block fields should not be present when None
        assert!(!json.contains("rcBlockHash"));
        assert!(!json.contains("rcBlockNumber"));
        assert!(!json.contains("ahTimestamp"));
    }

    #[test]
    fn test_asset_status_as_str() {
        assert_eq!(AssetStatus::Live.as_str(), "Live");
        assert_eq!(AssetStatus::Frozen.as_str(), "Frozen");
        assert_eq!(AssetStatus::Destroying.as_str(), "Destroying");
    }

    #[test]
    fn test_query_params_deserialization() {
        let json = r#"{"at": "10260000", "useRcBlock": true}"#;
        let params: PoolAssetsQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("10260000".to_string()));
        assert!(params.use_rc_block);
    }

    #[test]
    fn test_query_params_defaults() {
        let json = r#"{}"#;
        let params: PoolAssetsQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, None);
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_pool_assets_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<PoolAssetsQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
