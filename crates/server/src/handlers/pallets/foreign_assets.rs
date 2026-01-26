//! Handler for /pallets/foreign-assets endpoint.
//!
//! Returns information about all foreign assets on Asset Hub chains.
//! Foreign assets are cross-chain assets identified by XCM MultiLocation.

use crate::handlers::pallets::common::{AtResponse, PalletError, format_account_id};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use futures::StreamExt;
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize};
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

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
pub struct ForeignAssetItem {
    /// The XCM MultiLocation identifier for this foreign asset (as JSON or hex string)
    pub multi_location: serde_json::Value,
    /// Asset details (owner, supply, etc.) - always present, empty object if not found
    pub foreign_asset_info: serde_json::Value,
    /// Asset metadata (name, symbol, decimals) - always present, empty object if not found
    pub foreign_asset_metadata: serde_json::Value,
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
    let items = fetch_all_foreign_assets(&client_at_block, ss58_prefix).await?;

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
        let items = fetch_all_foreign_assets(&client_at_block, ss58_prefix).await?;

        results.push(PalletsForeignAssetsResponse {
            at,
            items,
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

/// Format a number with thousand separators (commas) to match Sidecar format.
fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Fetches all foreign assets by iterating over ForeignAssets::Asset storage.
/// Returns an error if the pallet doesn't exist or storage iteration fails.
async fn fetch_all_foreign_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<Vec<ForeignAssetItem>, PalletError> {
    let mut items = Vec::new();

    // First, fetch all metadata entries and store them by their blake2_128_concat key portion
    let mut metadata_map: std::collections::HashMap<Vec<u8>, serde_json::Value> =
        std::collections::HashMap::new();

    let metadata_addr = subxt::dynamic::storage::<(scale_value::Value,), scale_value::Value>(
        "ForeignAssets",
        "Metadata",
    );

    // Try to iterate metadata - if this fails, the pallet might not exist
    match client_at_block.storage().iter(metadata_addr, ()).await {
        Ok(mut metadata_stream) => {
            while let Some(entry_result) = metadata_stream.next().await {
                if let Ok(entry) = entry_result {
                    let key_bytes = entry.key_bytes();
                    // Extract the blake2_128_concat portion (bytes 32 onwards)
                    if key_bytes.len() > 32 {
                        let blake2_concat_portion = key_bytes[32..].to_vec();
                        let value_bytes = entry.value().bytes();
                        let metadata = decode_asset_metadata(value_bytes);
                        metadata_map.insert(blake2_concat_portion, metadata);
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to iterate ForeignAssets::Metadata storage: {:?}", e);
            // Continue - metadata might be empty but Asset storage could still work
        }
    }

    tracing::debug!("Fetched {} metadata entries", metadata_map.len());

    // Use dynamic storage iteration to get all foreign assets
    // ForeignAssets::Asset is a map with MultiLocation as key
    let storage_addr = subxt::dynamic::storage::<(scale_value::Value,), scale_value::Value>(
        "ForeignAssets",
        "Asset",
    );

    let mut stream = client_at_block
        .storage()
        .iter(storage_addr, ())
        .await
        .map_err(|e| {
            tracing::error!("Failed to iterate ForeignAssets::Asset storage: {:?}", e);
            PalletError::PalletNotAvailable("ForeignAssets")
        })?;

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

        // Debug: log the full Asset storage key for first entry
        if items.is_empty() {
            tracing::debug!(
                "First Asset storage key (len={}): 0x{}",
                key_bytes.len(),
                hex::encode(key_bytes)
            );
        }

        let multi_location = extract_multi_location_from_key(key_bytes);

        // Decode the asset details - use bytes() which returns a reference
        let value_bytes = entry.value().bytes();
        let foreign_asset_info = decode_asset_details(value_bytes, ss58_prefix);

        // Look up metadata using the blake2_128_concat portion of the key
        let foreign_asset_metadata = if key_bytes.len() > 32 {
            let blake2_concat_portion = &key_bytes[32..];
            metadata_map
                .get(blake2_concat_portion)
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        items.push(ForeignAssetItem {
            multi_location,
            foreign_asset_info,
            foreign_asset_metadata,
        });
    }

    Ok(items)
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
        "parents": parents.to_string(),
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
            // Parachain(Compact<u32>)
            let para_id = parity_scale_codec::Compact::<u32>::decode(cursor).ok()?.0;
            // Format with thousand separators to match Sidecar
            Some(serde_json::json!({"Parachain": format_with_commas(para_id as u64)}))
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
            let network = decode_option_network_id(cursor);
            let mut key = [0u8; 20];
            if cursor.len() >= 20 {
                key.copy_from_slice(&cursor[..20]);
                *cursor = &cursor[20..];
            }
            Some(
                serde_json::json!({"AccountKey20": {"network": network, "key": format!("0x{}", hex::encode(key))}}),
            )
        }
        4 => {
            // PalletInstance(u8)
            let instance = u8::decode(cursor).ok()?;
            Some(serde_json::json!({"PalletInstance": instance.to_string()}))
        }
        5 => {
            // GeneralIndex(Compact<u128>)
            let index = parity_scale_codec::Compact::<u128>::decode(cursor).ok()?.0;
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
                "length": length.to_string(),
                "data": format!("0x{}", hex::encode(data))
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
        0 => {
            // ByGenesis - has 32-byte hash
            let mut hash = [0u8; 32];
            if cursor.len() >= 32 {
                hash.copy_from_slice(&cursor[..32]);
                *cursor = &cursor[32..];
            }
            Some(serde_json::json!({"ByGenesis": format!("0x{}", hex::encode(hash))}))
        }
        1 => Some(serde_json::json!("ByFork")), // Would need version info
        2 => Some(serde_json::json!("Polkadot")),
        3 => Some(serde_json::json!("Kusama")),
        4 => Some(serde_json::json!("Westend")),
        5 => Some(serde_json::json!("Rococo")),
        6 => Some(serde_json::json!("Wococo")),
        7 => {
            // Ethereum - has Compact<u64> chain_id
            let chain_id = parity_scale_codec::Compact::<u64>::decode(cursor).ok()?.0;
            Some(serde_json::json!({"Ethereum": {"chainId": chain_id.to_string()}}))
        }
        8 => Some(serde_json::json!("BitcoinCore")),
        9 => Some(serde_json::json!("BitcoinCash")),
        10 => Some(serde_json::json!("PolkadotBulletin")),
        _ => Some(serde_json::json!({"Unknown": variant})),
    }
}

/// Decode asset details from raw bytes into JSON.
/// Returns an empty object `{}` if decoding fails.
fn decode_asset_details(bytes: &[u8], ss58_prefix: u16) -> serde_json::Value {
    let details = match AssetDetails::decode(&mut &bytes[..]) {
        Ok(d) => d,
        Err(_) => return serde_json::json!({}),
    };

    serde_json::json!({
        "owner": format_account_id(&details.owner, ss58_prefix),
        "issuer": format_account_id(&details.issuer, ss58_prefix),
        "admin": format_account_id(&details.admin, ss58_prefix),
        "freezer": format_account_id(&details.freezer, ss58_prefix),
        "supply": details.supply.to_string(),
        "deposit": details.deposit.to_string(),
        "minBalance": details.min_balance.to_string(),
        "isSufficient": details.is_sufficient,
        "accounts": details.accounts.to_string(),
        "sufficients": details.sufficients.to_string(),
        "approvals": details.approvals.to_string(),
        "status": details.status.as_str().to_string(),
    })
}

/// Decode asset metadata from raw bytes into JSON.
/// Returns an empty object `{}` if decoding fails.
fn decode_asset_metadata(bytes: &[u8]) -> serde_json::Value {
    let metadata = match AssetMetadataStorage::decode(&mut &bytes[..]) {
        Ok(m) => m,
        Err(_) => return serde_json::json!({}),
    };

    serde_json::json!({
        "deposit": metadata.deposit.to_string(),
        "name": format!("0x{}", hex::encode(&metadata.name)),
        "symbol": format!("0x{}", hex::encode(&metadata.symbol)),
        "decimals": metadata.decimals.to_string(),
        "isFrozen": metadata.is_frozen,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_foreign_asset_item_serialization() {
        let item = ForeignAssetItem {
            multi_location: serde_json::json!({
                "parents": "2",
                "interior": {
                    "X1": {
                        "GlobalConsensus": "Polkadot"
                    }
                }
            }),
            foreign_asset_info: serde_json::json!({
                "owner": "FxqimVubBRPqJ8kTwb3wL7G4q645hEkBEnXPyttLsTrFc5Q",
                "issuer": "FxqimVubBRPqJ8kTwb3wL7G4q645hEkBEnXPyttLsTrFc5Q",
                "admin": "FxqimVubBRPqJ8kTwb3wL7G4q645hEkBEnXPyttLsTrFc5Q",
                "freezer": "FxqimVubBRPqJ8kTwb3wL7G4q645hEkBEnXPyttLsTrFc5Q",
                "supply": "0",
                "deposit": "0",
                "minBalance": "100000000",
                "isSufficient": true,
                "accounts": "0",
                "sufficients": "0",
                "approvals": "0",
                "status": "Live"
            }),
            foreign_asset_metadata: serde_json::json!({
                "deposit": "0",
                "name": "0x506f6c6b61646f74",
                "symbol": "0x444f54",
                "decimals": "10",
                "isFrozen": false
            }),
        };

        let json = serde_json::to_string(&item).unwrap();

        // Verify camelCase serialization
        assert!(json.contains("\"multiLocation\""));
        assert!(json.contains("\"foreignAssetInfo\""));
        assert!(json.contains("\"foreignAssetMetadata\""));
        assert!(json.contains("\"minBalance\""));
        assert!(json.contains("\"isSufficient\""));
        assert!(json.contains("\"isFrozen\""));

        // Verify no snake_case
        assert!(!json.contains("\"multi_location\""));
        assert!(!json.contains("\"foreign_asset_info\""));
        assert!(!json.contains("\"foreign_asset_metadata\""));
    }

    #[test]
    fn test_foreign_assets_response_serialization() {
        let response = PalletsForeignAssetsResponse {
            at: AtResponse {
                hash: "0x1234567890abcdef".to_string(),
                height: "12345".to_string(),
            },
            items: vec![],
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();

        // Verify structure
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"items\""));
        assert!(json.contains("\"hash\""));
        assert!(json.contains("\"height\""));

        // Verify optional fields are not included when None
        assert!(!json.contains("\"rcBlockHash\""));
        assert!(!json.contains("\"rcBlockNumber\""));
        assert!(!json.contains("\"ahTimestamp\""));
    }

    #[test]
    fn test_foreign_assets_response_with_rc_block() {
        let response = PalletsForeignAssetsResponse {
            at: AtResponse {
                hash: "0x1234567890abcdef".to_string(),
                height: "12345".to_string(),
            },
            items: vec![],
            rc_block_hash: Some("0xabcdef".to_string()),
            rc_block_number: Some("67890".to_string()),
            ah_timestamp: Some("1234567890000".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();

        // Verify RC block fields are included in camelCase
        assert!(json.contains("\"rcBlockHash\""));
        assert!(json.contains("\"rcBlockNumber\""));
        assert!(json.contains("\"ahTimestamp\""));
    }

    #[test]
    fn test_empty_foreign_asset_info() {
        let item = ForeignAssetItem {
            multi_location: serde_json::json!({
                "parents": "0",
                "interior": "Here"
            }),
            foreign_asset_info: serde_json::json!({}),
            foreign_asset_metadata: serde_json::json!({}),
        };

        let json = serde_json::to_string(&item).unwrap();

        // Verify empty objects are serialized correctly
        assert!(json.contains("\"foreignAssetInfo\":{}"));
        assert!(json.contains("\"foreignAssetMetadata\":{}"));
    }

    #[test]
    fn test_decode_asset_details() {
        // Test that decode_asset_details returns empty JSON on invalid data
        let invalid_bytes = vec![0u8; 10];
        let result = decode_asset_details(&invalid_bytes, 0);
        assert_eq!(result, serde_json::json!({}));
    }

    #[test]
    fn test_decode_multi_location_empty() {
        let result = decode_multi_location(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_decode_multi_location_simple() {
        // parents=0, interior=Here (variant 0)
        let bytes = vec![0u8, 0u8];
        let result = decode_multi_location(&bytes);
        assert!(result.is_some());
        let json = result.unwrap();
        assert_eq!(json["parents"], "0");
        assert_eq!(json["interior"], "Here");
    }

    #[test]
    fn test_decode_junctions_here() {
        let bytes = vec![0u8];
        let result = decode_junctions(&bytes);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), serde_json::json!("Here"));
    }

    #[test]
    fn test_decode_junction_parachain() {
        // Junction::Parachain(1000) = variant 0, then Compact<u32>
        // Compact encoding: 1000 = 0x3e8, which in compact is 0xa10f (two-byte mode)
        // Two-byte mode: value << 2 | 0b01 = 1000 << 2 | 1 = 4001 = 0x0fa1 -> little endian 0xa1, 0x0f
        let mut cursor: &[u8] = &[0u8, 0xa1, 0x0f]; // 0 = Parachain, Compact(1000)
        let result = decode_junction(&mut cursor);
        assert!(result.is_some());
        let json = result.unwrap();
        assert_eq!(json["Parachain"], "1,000");
    }

    #[test]
    fn test_decode_network_id_polkadot() {
        // NetworkId::Polkadot = variant 2
        let mut cursor: &[u8] = &[2u8];
        let result = decode_network_id(&mut cursor);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), serde_json::json!("Polkadot"));
    }

    #[test]
    fn test_decode_network_id_kusama() {
        // NetworkId::Kusama = variant 3
        let mut cursor: &[u8] = &[3u8];
        let result = decode_network_id(&mut cursor);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), serde_json::json!("Kusama"));
    }

    #[test]
    fn test_asset_status_as_str() {
        assert_eq!(AssetStatus::Live.as_str(), "Live");
        assert_eq!(AssetStatus::Frozen.as_str(), "Frozen");
        assert_eq!(AssetStatus::Destroying.as_str(), "Destroying");
    }

    #[test]
    fn test_extract_multi_location_from_key_short() {
        // Key shorter than expected prefix length
        let short_key = vec![0u8; 32];
        let result = extract_multi_location_from_key(&short_key);
        // Should return raw hex representation
        assert!(result["raw"].is_string());
    }

    #[test]
    fn test_query_params_deserialization() {
        // Test default use_rc_block is false
        let params: ForeignAssetsQueryParams = serde_json::from_str(r#"{"at":"12345"}"#).unwrap();
        assert_eq!(params.at, Some("12345".to_string()));
        assert!(!params.use_rc_block);

        // Test explicit use_rc_block
        let params: ForeignAssetsQueryParams =
            serde_json::from_str(r#"{"at":"12345","useRcBlock":true}"#).unwrap();
        assert!(params.use_rc_block);
    }
}
