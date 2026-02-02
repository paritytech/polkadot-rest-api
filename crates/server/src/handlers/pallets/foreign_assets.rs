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
use heck::ToLowerCamelCase;
use parity_scale_codec::Decode;
use scale_info::PortableRegistry;
use scale_value::scale::decode_as_type;
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

/// Fetches all foreign assets by iterating over ForeignAssets::Asset storage.
/// Returns an error if the pallet doesn't exist or storage iteration fails.
async fn fetch_all_foreign_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<Vec<ForeignAssetItem>, PalletError> {
    let mut items = Vec::new();

    // First, fetch all metadata entries and store them by their key bytes
    // We use the key part bytes directly from Subxt's API
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
                if let Ok(entry) = entry_result
                    // Use Subxt's key().part(0) to get the MultiLocation key part directly
                    // This avoids manual byte offset calculations
                    && let Ok(key) = entry.key()
                    && let Some(key_part) = key.part(0)
                {
                    let key_part_bytes = key_part.bytes().to_vec();
                    let value_bytes = entry.value().bytes();
                    let metadata = decode_asset_metadata(value_bytes);
                    metadata_map.insert(key_part_bytes, metadata);
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

        // Use Subxt's key().part(0) to extract the MultiLocation key part directly
        // This is cleaner than manual byte offset calculations
        let key = match entry.key() {
            Ok(k) => k,
            Err(e) => {
                tracing::debug!("Failed to decode storage key: {:?}", e);
                continue;
            }
        };
        let key_part = match key.part(0) {
            Some(part) => part,
            None => {
                tracing::debug!("Storage key has no parts, skipping entry");
                continue;
            }
        };

        // Debug: log the key part bytes for first entry
        if items.is_empty() {
            tracing::debug!(
                "First Asset key part (len={}): 0x{}",
                key_part.bytes().len(),
                hex::encode(key_part.bytes())
            );
        }

        // Decode the MultiLocation from the key part bytes
        let multi_location = decode_multi_location_from_bytes(key_part.bytes());

        // Decode the asset details
        let value_bytes = entry.value().bytes();
        let foreign_asset_info = decode_asset_details(value_bytes, ss58_prefix);

        // Look up metadata using the key part bytes
        let key_part_bytes = key_part.bytes();
        let foreign_asset_metadata = metadata_map
            .get(key_part_bytes)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        items.push(ForeignAssetItem {
            multi_location,
            foreign_asset_info,
            foreign_asset_metadata,
        });
    }

    Ok(items)
}

/// Build a portable registry containing the XCM v4 Location type.
/// This is used for decoding MultiLocation storage keys.
fn build_location_registry() -> (PortableRegistry, u32) {
    let mut registry = scale_info::Registry::new();
    let type_id = registry.register_type(&scale_info::meta_type::<staging_xcm::v4::Location>());
    (registry.into(), type_id.id)
}

/// Format a number with comma separators (e.g., 2001 -> "2,001").
/// Matches Sidecar's formatting for parachain IDs and other numbers.
fn format_number_with_commas(n: u128) -> String {
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

/// Convert scale_value to JSON matching Sidecar's exact format.
///
/// Key differences from generic scale_value_to_json:
/// - Uses PascalCase for variant names (X1, X2, GlobalConsensus, Ethereum, etc.)
/// - Arrays are flat (not double-nested)  
/// - Numbers formatted with commas for large values
/// - Field order preserved (parents before interior for Location)
#[allow(clippy::only_used_in_recursion)]
fn scale_value_to_sidecar_json(
    value: scale_value::Value<u32>,
    registry: &PortableRegistry,
) -> serde_json::Value {
    use scale_value::{Composite, Primitive, ValueDef};
    use serde_json::Value;

    match value.value {
        ValueDef::Composite(composite) => match composite {
            Composite::Named(fields) => {
                // Collect fields preserving their order
                let fields_vec: Vec<_> = fields.into_iter().collect();

                // For Location type, ensure "parents" comes before "interior"
                let mut ordered_fields: Vec<(String, Value)> = Vec::new();
                let mut parents_val = None;
                let mut interior_val = None;
                let mut other_fields = Vec::new();

                for (name, val) in fields_vec {
                    let key = name.to_lower_camel_case();
                    let json_val = scale_value_to_sidecar_json(val, registry);
                    if key == "parents" {
                        parents_val = Some(json_val);
                    } else if key == "interior" {
                        interior_val = Some(json_val);
                    } else {
                        other_fields.push((key, json_val));
                    }
                }

                // Build in Sidecar order: parents, interior, then others
                if let Some(p) = parents_val {
                    ordered_fields.push(("parents".to_string(), p));
                }
                if let Some(i) = interior_val {
                    ordered_fields.push(("interior".to_string(), i));
                }
                ordered_fields.extend(other_fields);

                // Use indexmap to preserve order, then convert to serde_json::Map
                let map: serde_json::Map<String, Value> = ordered_fields.into_iter().collect();
                Value::Object(map)
            }
            Composite::Unnamed(fields) => {
                let fields_vec: Vec<_> = fields.into_iter().collect();
                // Check for byte array
                if !fields_vec.is_empty() && is_byte_array(&fields_vec) {
                    Value::String(bytes_to_hex(&fields_vec))
                } else if fields_vec.len() == 1 {
                    // Single field - unwrap
                    scale_value_to_sidecar_json(fields_vec.into_iter().next().unwrap(), registry)
                } else {
                    // Multiple fields - array
                    Value::Array(
                        fields_vec
                            .into_iter()
                            .map(|v| scale_value_to_sidecar_json(v, registry))
                            .collect(),
                    )
                }
            }
        },
        ValueDef::Variant(variant) => {
            // Handle Option::None as JSON null
            if variant.name == "None" {
                return Value::Null;
            }

            // Use PascalCase for variant names to match Sidecar
            let name = variant.name.clone();

            let inner = match variant.values {
                Composite::Named(fields) if !fields.is_empty() => {
                    // Collect fields and ensure specific ordering for known types
                    let fields_vec: Vec<_> = fields.into_iter().collect();
                    let mut ordered_fields: Vec<(String, Value)> = Vec::new();

                    // For AccountKey20, Sidecar order is: network, key
                    let mut network_val = None;
                    let mut key_val = None;
                    let mut other_fields = Vec::new();

                    for (n, v) in fields_vec {
                        let key = n.to_lower_camel_case();
                        let json_val = scale_value_to_sidecar_json(v, registry);
                        if key == "network" {
                            network_val = Some(json_val);
                        } else if key == "key" {
                            key_val = Some(json_val);
                        } else {
                            other_fields.push((key, json_val));
                        }
                    }

                    // Build in Sidecar order
                    if let Some(n) = network_val {
                        ordered_fields.push(("network".to_string(), n));
                    }
                    if let Some(k) = key_val {
                        ordered_fields.push(("key".to_string(), k));
                    }
                    ordered_fields.extend(other_fields);

                    let map: serde_json::Map<String, Value> = ordered_fields.into_iter().collect();
                    Value::Object(map)
                }
                Composite::Unnamed(fields) if !fields.is_empty() => {
                    let fields_vec: Vec<_> = fields.into_iter().collect();
                    if is_byte_array(&fields_vec) {
                        Value::String(bytes_to_hex(&fields_vec))
                    } else if fields_vec.len() == 1 && !is_junction_variant(&name) {
                        // Single field - unwrap unless it's a junction
                        scale_value_to_sidecar_json(
                            fields_vec.into_iter().next().unwrap(),
                            registry,
                        )
                    } else {
                        // For junctions (X1, X2, etc.) - flatten the array
                        // The interior of Location is a tuple, so X2 contains a single tuple with 2 elements
                        // We need to flatten: X2: [[a, b]] -> X2: [a, b]
                        if is_junction_variant(&name) && fields_vec.len() == 1 {
                            // Single tuple containing the junction items - unwrap it
                            let inner = scale_value_to_sidecar_json(
                                fields_vec.into_iter().next().unwrap(),
                                registry,
                            );
                            // If the inner is an array, use it directly; otherwise wrap
                            if inner.is_array() {
                                inner
                            } else {
                                Value::Array(vec![inner])
                            }
                        } else {
                            Value::Array(
                                fields_vec
                                    .into_iter()
                                    .map(|v| scale_value_to_sidecar_json(v, registry))
                                    .collect(),
                            )
                        }
                    }
                }
                _ => Value::Null,
            };

            // For unit variants (no data), Sidecar outputs just the string name
            // e.g., "GlobalConsensus": "Polkadot" instead of "GlobalConsensus": {"Polkadot": null}
            if inner.is_null() {
                return Value::String(name);
            }

            let mut map = serde_json::Map::new();
            map.insert(name, inner);
            Value::Object(map)
        }
        ValueDef::Primitive(prim) => match prim {
            Primitive::Bool(b) => Value::Bool(b),
            Primitive::Char(c) => Value::String(c.to_string()),
            Primitive::String(s) => Value::String(s),
            Primitive::U128(n) => {
                // Format with commas for numbers >= 1000 to match Sidecar
                if n >= 1000 {
                    Value::String(format_number_with_commas(n))
                } else {
                    Value::String(n.to_string())
                }
            }
            Primitive::I128(n) => Value::String(n.to_string()),
            Primitive::U256(n) => Value::String(format!("{:?}", n)),
            Primitive::I256(n) => Value::String(format!("{:?}", n)),
        },
        ValueDef::BitSequence(bits) => {
            let bytes: Vec<u8> = bits
                .iter()
                .collect::<Vec<_>>()
                .chunks(8)
                .map(|chunk| {
                    chunk
                        .iter()
                        .enumerate()
                        .fold(0u8, |acc, (i, &bit)| acc | ((bit as u8) << i))
                })
                .collect();
            Value::String(format!("0x{}", hex::encode(bytes)))
        }
    }
}

/// Check if variant name is an X1-X8 junction.
fn is_junction_variant(name: &str) -> bool {
    matches!(name, "X1" | "X2" | "X3" | "X4" | "X5" | "X6" | "X7" | "X8")
}

/// Check if an array of values looks like a byte array.
fn is_byte_array(values: &[scale_value::Value<u32>]) -> bool {
    values.len() >= 2
        && values.iter().all(|v| {
            matches!(
                &v.value,
                scale_value::ValueDef::Primitive(scale_value::Primitive::U128(n)) if *n <= 255
            )
        })
}

/// Convert a slice of values to a hex string.
fn bytes_to_hex(values: &[scale_value::Value<u32>]) -> String {
    let bytes: Vec<u8> = values
        .iter()
        .filter_map(|v| match &v.value {
            scale_value::ValueDef::Primitive(scale_value::Primitive::U128(n)) => Some(*n as u8),
            _ => None,
        })
        .collect();
    format!("0x{}", hex::encode(bytes))
}

/// Decode MultiLocation from SCALE-encoded bytes.
/// The bytes are obtained directly from Subxt's key().part(0).bytes() which
/// already extracts just the MultiLocation portion without prefix hashes.
///
/// Uses staging_xcm::v4::Location type with scale_value for decoding that handles:
/// - Proper variant indices matching the actual XCM types
/// - Numeric type coercion (e.g., u8 -> u64)
/// - Sidecar-compatible JSON format (PascalCase variants, comma-formatted numbers)
fn decode_multi_location_from_bytes(multi_location_bytes: &[u8]) -> serde_json::Value {
    if multi_location_bytes.is_empty() {
        return serde_json::json!({"raw": "0x"});
    }

    // Build registry with XCM v4 Location type
    let (registry, type_id) = build_location_registry();

    // Decode using scale-value for proper JSON serialization
    match decode_as_type(&mut &multi_location_bytes[..], type_id, &registry) {
        Ok(value) => scale_value_to_sidecar_json(value, &registry),
        Err(_) => {
            // Decoding failed - fall back to raw hex representation
            // This ensures we always return something useful even for unknown XCM versions
            serde_json::json!({"raw": format!("0x{}", hex::encode(multi_location_bytes))})
        }
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
    fn test_build_location_registry() {
        // Test that we can build a location registry
        let (registry, type_id) = build_location_registry();

        // The registry should have the Location type
        assert!(registry.resolve(type_id).is_some());
    }

    #[test]
    fn test_decode_multi_location_simple() {
        // Location { parents: 0, interior: Here } encodes to: [0, 0]
        // Now we pass just the SCALE-encoded MultiLocation bytes directly
        let location_bytes = vec![
            0, // parents = 0
            0, // interior = Here variant
        ];

        let result = decode_multi_location_from_bytes(&location_bytes);

        // Should successfully decode
        assert!(
            result.get("raw").is_none(),
            "Should not fall back to raw hex"
        );
        assert_eq!(result["parents"], "0");
        // interior is "Here" string (unit variant in Sidecar format)
        assert_eq!(result["interior"], "Here");
    }

    #[test]
    fn test_decode_multi_location_with_parachain() {
        // Location { parents: 1, interior: X1([Parachain(1000)]) }
        // SCALE encoding:
        // parents = 1
        // interior variant X1 = 1
        // Junction::Parachain variant = 0
        // 1000 as compact u32 = [0xa1, 0x0f] (1000 << 2 = 4000 = 0x0fa0, little endian)
        let location_bytes = vec![
            1, // parents = 1
            1, // X1 variant
            0, // Parachain variant
            0xa1, 0x0f, // compact encoded 1000
        ];

        let result = decode_multi_location_from_bytes(&location_bytes);

        // Should successfully decode
        assert!(
            result.get("raw").is_none(),
            "Should not fall back to raw hex: {}",
            result
        );
        assert_eq!(result["parents"], "1");
    }

    #[test]
    fn test_decode_multi_location_invalid() {
        // Test with invalid bytes that can't be decoded
        let invalid_bytes = vec![255, 255, 255]; // Invalid variant indices

        let result = decode_multi_location_from_bytes(&invalid_bytes);

        // Should fall back to raw hex
        assert!(result["raw"].is_string());
    }

    #[test]
    fn test_asset_status_as_str() {
        assert_eq!(AssetStatus::Live.as_str(), "Live");
        assert_eq!(AssetStatus::Frozen.as_str(), "Frozen");
        assert_eq!(AssetStatus::Destroying.as_str(), "Destroying");
    }

    #[test]
    fn test_decode_multi_location_empty() {
        // Empty bytes should return raw hex
        let empty_bytes: Vec<u8> = vec![];
        let result = decode_multi_location_from_bytes(&empty_bytes);
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
