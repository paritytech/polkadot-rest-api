//! Handler for /pallets/assets/{assetId}/asset-info endpoint.

use crate::handlers::pallets::common::{AtResponse, PalletError};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize};
use subxt_historic::client::OnlineClientAtBlockT;
use subxt_historic::config::Config;

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
// Internal SCALE Decode Types
// ============================================================================

#[derive(Debug, Clone, Decode)]
enum AssetStatus {
    Live,
    Frozen,
    Destroying,
}

impl AssetStatus {
    fn as_str(&self) -> &'static str {
        match self {
            AssetStatus::Live => "Live",
            AssetStatus::Frozen => "Frozen",
            AssetStatus::Destroying => "Destroying",
        }
    }
}

#[derive(Debug, Clone, Decode)]
struct AssetDetails {
    owner: [u8; 32],
    issuer: [u8; 32],
    admin: [u8; 32],
    freezer: [u8; 32],
    supply: u128,
    deposit: u128,
    min_balance: u128,
    is_sufficient: bool,
    accounts: u32,
    sufficients: u32,
    approvals: u32,
    status: AssetStatus,
}

#[derive(Debug, Clone, Decode)]
struct AssetMetadataStorage {
    deposit: u128,
    name: Vec<u8>,
    symbol: Vec<u8>,
    decimals: u8,
    is_frozen: bool,
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

    let block_id = params.at.map(|s| s.parse::<utils::BlockId>()).transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;
    let client_at_block = state.client.at(resolved_block.number).await?;

    let at = AtResponse {
        hash: resolved_block.hash,
        height: resolved_block.number.to_string(),
    };

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

    if ah_blocks.is_empty() {
        return Ok(build_empty_rc_response(&rc_resolved_block));
    }

    let ah_block = &ah_blocks[0];
    let client_at_block = state.client.at(ah_block.number).await?;

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    let ah_timestamp = fetch_timestamp(&client_at_block).await;
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
            rc_block_hash: Some(rc_resolved_block.hash),
            rc_block_number: Some(rc_resolved_block.number.to_string()),
            ah_timestamp,
        }),
    )
        .into_response())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Fetches asset details from Assets::Asset storage.
async fn fetch_asset_info<'client, T, C>(
    client_at_block: &'client subxt_historic::client::ClientAtBlock<C, T>,
    asset_id: u32,
    ss58_prefix: u16,
) -> Option<AssetInfo>
where
    T: Config + 'client,
    C: OnlineClientAtBlockT<'client, T>,
{
    let asset_storage = client_at_block.storage().entry("Assets", "Asset").ok()?;

    let asset_value = asset_storage.fetch([asset_id]).await.ok()??;
    let raw_bytes = asset_value.into_bytes();
    let details = AssetDetails::decode(&mut &raw_bytes[..]).ok()?;

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
async fn fetch_asset_metadata<'client, T, C>(
    client_at_block: &'client subxt_historic::client::ClientAtBlock<C, T>,
    asset_id: u32,
) -> Option<AssetMetadata>
where
    T: Config + 'client,
    C: OnlineClientAtBlockT<'client, T>,
{
    let metadata_storage = client_at_block.storage().entry("Assets", "Metadata").ok()?;

    let metadata_value = metadata_storage.fetch([asset_id]).await.ok()??;
    let raw_bytes = metadata_value.into_bytes();
    let metadata = AssetMetadataStorage::decode(&mut &raw_bytes[..]).ok()?;

    Some(AssetMetadata {
        deposit: metadata.deposit.to_string(),
        name: format!("0x{}", hex::encode(&metadata.name)),
        symbol: format!("0x{}", hex::encode(&metadata.symbol)),
        decimals: metadata.decimals.to_string(),
        is_frozen: metadata.is_frozen,
    })
}

/// Fetches timestamp from Timestamp::Now storage.
async fn fetch_timestamp<'client, T, C>(
    client_at_block: &'client subxt_historic::client::ClientAtBlock<C, T>,
) -> Option<String>
where
    T: Config + 'client,
    C: OnlineClientAtBlockT<'client, T>,
{
    let timestamp_entry = client_at_block.storage().entry("Timestamp", "Now").ok()?;

    let timestamp = timestamp_entry.fetch(()).await.ok()??;
    let timestamp_bytes = timestamp.into_bytes();
    let timestamp_value = u64::decode(&mut &timestamp_bytes[..]).ok()?;

    Some(timestamp_value.to_string())
}

/// Builds an empty response when no AH blocks are found in the RC block.
fn build_empty_rc_response(rc_resolved_block: &utils::ResolvedBlock) -> Response {
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

/// Formats a 32-byte account ID to SS58 format.
fn format_account_id(account: &[u8; 32], ss58_prefix: u16) -> String {
    use sp_core::crypto::Ss58Codec;
    sp_core::sr25519::Public::from_raw(*account).to_ss58check_with_version(ss58_prefix.into())
}
