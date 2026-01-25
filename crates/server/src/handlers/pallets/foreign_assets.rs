//! Handler for /pallets/foreign-assets endpoint.
//!
//! Returns information about all foreign assets on Asset Hub chains.
//! Foreign assets are cross-chain assets identified by XCM MultiLocation.

use crate::handlers::pallets::common::{format_account_id, AtResponse, PalletError};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use config::ChainType;
use futures::StreamExt;
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize};
use subxt::{client::OnlineClientAtBlock, SubstrateConfig};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignAssetsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub use_rc_block: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignAssetInfo {
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
pub struct ForeignAssetMetadata {
    pub deposit: String,
    pub name: String,
    pub symbol: String,
    pub decimals: String,
    pub is_frozen: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignAssetItem {
    /// The XCM MultiLocation identifier for this foreign asset (as JSON)
    pub multi_location: serde_json::Value,
    /// Asset details (owner, supply, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreign_asset_info: Option<ForeignAssetInfo>,
    /// Asset metadata (name, symbol, decimals)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreign_asset_metadata: Option<ForeignAssetMetadata>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsForeignAssetsResponse {
    pub at: AtResponse,
    pub items: Vec<ForeignAssetItem>,
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

/// Handler for GET /pallets/foreign-assets
///
/// Returns all foreign assets with their details and metadata.
/// Foreign assets use XCM MultiLocation as their identifier instead of simple u32 IDs.
pub async fn pallets_foreign_assets(
    State(state): State<AppState>,
    Query(params): Query<ForeignAssetsQueryParams>,
) -> Result<Response, PalletError> {
    // Foreign assets only exist on Asset Hub chains
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::PalletNotAvailable("ForeignAssets"));
    }

    if params.use_rc_block {
        return handle_use_rc_block(state, params).await;
    }

    // Create client at the specified block
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

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    let ss58_prefix = state.chain_info.ss58_prefix;
    let items = fetch_all_foreign_assets(&client_at_block, ss58_prefix).await;

    Ok((
        StatusCode::OK,
        Json(PalletsForeignAssetsResponse {
            at,
            items,
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
    params: ForeignAssetsQueryParams,
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
    let client_at_block = state.client.at_block(ah_block.number).await?;

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    let ah_timestamp = fetch_timestamp(&client_at_block).await;
    let ss58_prefix = state.chain_info.ss58_prefix;
    let items = fetch_all_foreign_assets(&client_at_block, ss58_prefix).await;

    Ok((
        StatusCode::OK,
        Json(PalletsForeignAssetsResponse {
            at,
            items,
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

/// Fetches all foreign assets by iterating over ForeignAssets::Asset storage.
async fn fetch_all_foreign_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Vec<ForeignAssetItem> {
    let mut items = Vec::new();

    // Use dynamic storage iteration to get all foreign assets
    // ForeignAssets::Asset is a map with MultiLocation as key
    let storage_addr =
        subxt::dynamic::storage::<(scale_value::Value,), scale_value::Value>("ForeignAssets", "Asset");

    let mut stream = match client_at_block.storage().iter(storage_addr, ()).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to iterate ForeignAssets::Asset storage: {:?}", e);
            return items;
        }
    };

    while let Some(entry_result) = stream.next().await {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("Error reading foreign asset entry: {:?}", e);
                continue;
            }
        };

        // Extract the MultiLocation key from the storage key
        let key_bytes = entry.key_bytes();
        let key_bytes_owned = key_bytes.to_vec();
        let multi_location = extract_multi_location_from_key(key_bytes);

        // Decode the asset details - use bytes() which returns a reference
        let value_bytes = entry.value().bytes();
        let foreign_asset_info = decode_asset_details(value_bytes, ss58_prefix);

        // Fetch metadata for this asset using the same key
        let foreign_asset_metadata =
            fetch_foreign_asset_metadata(client_at_block, &key_bytes_owned).await;

        items.push(ForeignAssetItem {
            multi_location,
            foreign_asset_info,
            foreign_asset_metadata,
        });
    }

    items
}

/// Extract MultiLocation from storage key bytes.
/// The key format is: twox128(pallet) ++ twox128(storage) ++ blake2_128_concat(multilocation)
/// We need to skip the first 32 bytes (prefix hashes) and decode the rest.
fn extract_multi_location_from_key(key_bytes: &[u8]) -> serde_json::Value {
    // Skip twox128(ForeignAssets) = 16 bytes + twox128(Asset) = 16 bytes = 32 bytes
    // Then skip blake2_128 hash = 16 bytes
    // The remaining bytes are the SCALE-encoded MultiLocation
    if key_bytes.len() <= 48 {
        return serde_json::json!({"raw": format!("0x{}", hex::encode(key_bytes))});
    }

    let multi_location_bytes = &key_bytes[48..];
    
    // Try to decode as a versioned location (XCM v3/v4)
    // The MultiLocation structure varies by XCM version, so we'll return a hex representation
    // along with attempting to decode common patterns
    match decode_multi_location(multi_location_bytes) {
        Some(decoded) => decoded,
        None => serde_json::json!({"raw": format!("0x{}", hex::encode(multi_location_bytes))}),
    }
}

/// Attempt to decode MultiLocation bytes into a JSON structure.
/// XCM MultiLocation has the structure: { parents: u8, interior: Junctions }
fn decode_multi_location(bytes: &[u8]) -> Option<serde_json::Value> {
    if bytes.is_empty() {
        return None;
    }

    // Try to decode using scale_value for a generic approach
    // This provides a best-effort decode without requiring exact type definitions
    let cursor = &mut &bytes[..];
    
    // First byte is `parents` (u8)
    let parents = u8::decode(cursor).ok()?;
    
    // Remaining bytes are the interior junctions
    // For now, return a structured representation
    let interior_bytes = *cursor;
    
    Some(serde_json::json!({
        "parents": parents,
        "interior": decode_junctions(interior_bytes).unwrap_or_else(|| {
            serde_json::json!({"raw": format!("0x{}", hex::encode(interior_bytes))})
        })
    }))
}

/// Decode XCM Junctions enum.
/// Junctions is an enum: Here, X1, X2, X3, X4, X5, X6, X7, X8
fn decode_junctions(bytes: &[u8]) -> Option<serde_json::Value> {
    if bytes.is_empty() {
        return None;
    }

    let cursor = &mut &bytes[..];
    let variant_index = u8::decode(cursor).ok()?;

    match variant_index {
        0 => Some(serde_json::json!("Here")),
        1 => {
            // X1 - single junction
            let junction = decode_junction(cursor)?;
            Some(serde_json::json!({"X1": [junction]}))
        }
        2 => {
            // X2 - two junctions
            let j1 = decode_junction(cursor)?;
            let j2 = decode_junction(cursor)?;
            Some(serde_json::json!({"X2": [j1, j2]}))
        }
        3 => {
            // X3 - three junctions
            let j1 = decode_junction(cursor)?;
            let j2 = decode_junction(cursor)?;
            let j3 = decode_junction(cursor)?;
            Some(serde_json::json!({"X3": [j1, j2, j3]}))
        }
        _ => {
            // For higher arities or unknown variants, return raw
            Some(serde_json::json!({"raw": format!("0x{}", hex::encode(bytes))}))
        }
    }
}

/// Decode a single XCM Junction.
fn decode_junction(cursor: &mut &[u8]) -> Option<serde_json::Value> {
    if cursor.is_empty() {
        return None;
    }

    let variant_index = u8::decode(cursor).ok()?;

    match variant_index {
        0 => {
            // Parachain(u32)
            let para_id = u32::decode(cursor).ok()?;
            Some(serde_json::json!({"Parachain": para_id}))
        }
        1 => {
            // AccountId32 { network: Option<NetworkId>, id: [u8; 32] }
            let _network = decode_option_network_id(cursor);
            let mut id = [0u8; 32];
            if cursor.len() >= 32 {
                id.copy_from_slice(&cursor[..32]);
                *cursor = &cursor[32..];
            }
            Some(serde_json::json!({"AccountId32": {"id": format!("0x{}", hex::encode(id))}}))
        }
        2 => {
            // AccountIndex64 { network: Option<NetworkId>, index: u64 }
            let _network = decode_option_network_id(cursor);
            let index = u64::decode(cursor).ok()?;
            Some(serde_json::json!({"AccountIndex64": {"index": index}}))
        }
        3 => {
            // AccountKey20 { network: Option<NetworkId>, key: [u8; 20] }
            let _network = decode_option_network_id(cursor);
            let mut key = [0u8; 20];
            if cursor.len() >= 20 {
                key.copy_from_slice(&cursor[..20]);
                *cursor = &cursor[20..];
            }
            Some(serde_json::json!({"AccountKey20": {"key": format!("0x{}", hex::encode(key))}}))
        }
        4 => {
            // PalletInstance(u8)
            let instance = u8::decode(cursor).ok()?;
            Some(serde_json::json!({"PalletInstance": instance}))
        }
        5 => {
            // GeneralIndex(u128)
            let index = u128::decode(cursor).ok()?;
            Some(serde_json::json!({"GeneralIndex": index.to_string()}))
        }
        6 => {
            // GeneralKey { length: u8, data: [u8; 32] }
            let length = u8::decode(cursor).ok()?;
            let mut data = [0u8; 32];
            if cursor.len() >= 32 {
                data.copy_from_slice(&cursor[..32]);
                *cursor = &cursor[32..];
            }
            Some(serde_json::json!({"GeneralKey": {
                "length": length,
                "data": format!("0x{}", hex::encode(&data[..length as usize]))
            }}))
        }
        7 => {
            // OnlyChild - no data
            Some(serde_json::json!("OnlyChild"))
        }
        9 => {
            // GlobalConsensus(NetworkId)
            let network = decode_network_id(cursor)?;
            Some(serde_json::json!({"GlobalConsensus": network}))
        }
        _ => {
            // Unknown junction type
            Some(serde_json::json!({"Unknown": variant_index}))
        }
    }
}

/// Decode Option<NetworkId>
fn decode_option_network_id(cursor: &mut &[u8]) -> Option<serde_json::Value> {
    if cursor.is_empty() {
        return None;
    }
    let is_some = u8::decode(cursor).ok()?;
    if is_some == 0 {
        Some(serde_json::json!(null))
    } else {
        decode_network_id(cursor)
    }
}

/// Decode NetworkId enum
fn decode_network_id(cursor: &mut &[u8]) -> Option<serde_json::Value> {
    if cursor.is_empty() {
        return None;
    }
    let variant = u8::decode(cursor).ok()?;
    match variant {
        0 => Some(serde_json::json!("ByGenesis")), // Would need 32-byte hash
        1 => Some(serde_json::json!("ByFork")),    // Would need version info
        2 => Some(serde_json::json!("Polkadot")),
        3 => Some(serde_json::json!("Kusama")),
        4 => Some(serde_json::json!("Westend")),
        5 => Some(serde_json::json!("Rococo")),
        6 => Some(serde_json::json!("Wococo")),
        7 => Some(serde_json::json!("Ethereum")),  // Would need chain_id
        8 => Some(serde_json::json!("BitcoinCore")),
        9 => Some(serde_json::json!("BitcoinCash")),
        10 => Some(serde_json::json!("PolkadotBulletin")),
        _ => Some(serde_json::json!({"Unknown": variant})),
    }
}

/// Decode asset details from raw bytes.
fn decode_asset_details(bytes: &[u8], ss58_prefix: u16) -> Option<ForeignAssetInfo> {
    let details = AssetDetails::decode(&mut &bytes[..]).ok()?;

    Some(ForeignAssetInfo {
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

/// Fetch metadata for a foreign asset using its storage key.
/// 
/// Note: This function constructs the metadata storage key from the asset storage key.
/// Both use blake2_128_concat hashing for the MultiLocation key.
async fn fetch_foreign_asset_metadata(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    asset_key_bytes: &[u8],
) -> Option<ForeignAssetMetadata> {
    // The asset key format is: twox128(ForeignAssets) ++ twox128(Asset) ++ blake2_128_concat(multilocation)
    // For metadata, we need: twox128(ForeignAssets) ++ twox128(Metadata) ++ blake2_128_concat(multilocation)
    // 
    // The blake2_128_concat portion (16 bytes hash + SCALE-encoded multilocation) starts at byte 32
    if asset_key_bytes.len() <= 32 {
        return None;
    }

    // Extract the blake2_128_concat(multilocation) portion (everything after the two twox128 prefixes)
    let multi_location_with_hash = &asset_key_bytes[32..];

    // Build the metadata storage key using pre-computed twox128 hashes
    // twox128("ForeignAssets") = 0x5e4b6f02564ae6307da1a98362192d3c
    // twox128("Metadata") = 0x312fca352e82f3b5d23f36b1ea2635ce
    let mut metadata_key = Vec::with_capacity(32 + multi_location_with_hash.len());
    
    // twox128("ForeignAssets") - pre-computed
    metadata_key.extend_from_slice(&[
        0x5e, 0x4b, 0x6f, 0x02, 0x56, 0x4a, 0xe6, 0x30,
        0x7d, 0xa1, 0xa9, 0x83, 0x62, 0x19, 0x2d, 0x3c,
    ]);
    // twox128("Metadata") - pre-computed
    metadata_key.extend_from_slice(&[
        0x31, 0x2f, 0xca, 0x35, 0x2e, 0x82, 0xf3, 0xb5,
        0xd2, 0x3f, 0x36, 0xb1, 0xea, 0x26, 0x35, 0xce,
    ]);
    // The blake2_128_concat(multilocation) portion from the asset key
    metadata_key.extend_from_slice(multi_location_with_hash);

    // Fetch using raw storage key
    let metadata_value = match client_at_block
        .storage()
        .fetch_raw(metadata_key)
        .await
    {
        Ok(value) if !value.is_empty() => value,
        Ok(_) => return None,
        Err(e) => {
            tracing::debug!("Failed to fetch foreign asset metadata: {:?}", e);
            return None;
        }
    };

    let metadata = AssetMetadataStorage::decode(&mut &metadata_value[..]).ok()?;

    Some(ForeignAssetMetadata {
        deposit: metadata.deposit.to_string(),
        name: format!("0x{}", hex::encode(&metadata.name)),
        symbol: format!("0x{}", hex::encode(&metadata.symbol)),
        decimals: metadata.decimals.to_string(),
        is_frozen: metadata.is_frozen,
    })
}

/// Fetches timestamp from Timestamp::Now storage.
async fn fetch_timestamp(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> Option<String> {
    let timestamp_addr = subxt::dynamic::storage::<(), u64>("Timestamp", "Now");
    let timestamp = client_at_block
        .storage()
        .fetch(timestamp_addr, ())
        .await
        .ok()?;
    let timestamp_value = timestamp.decode().ok()?;
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
        Json(PalletsForeignAssetsResponse {
            at,
            items: vec![],
            rc_block_hash: Some(rc_resolved_block.hash.clone()),
            rc_block_number: Some(rc_resolved_block.number.to_string()),
            ah_timestamp: None,
        }),
    )
        .into_response()
}
