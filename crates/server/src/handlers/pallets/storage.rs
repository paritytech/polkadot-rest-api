//! Handler for `/pallets/{palletId}/storage` endpoint.
//!
//! Returns storage item metadata for a pallet, matching Sidecar's response format.
//! Supports all metadata versions V9-V16.

// Allow large error types - PalletError contains subxt::error::OnlineClientAtBlockError
// which is large by design. Boxing would add indirection without significant benefit.
#![allow(clippy::result_large_err)]

use crate::handlers::pallets::common::PalletError;
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json, extract::Path, extract::Query, extract::State, response::IntoResponse, response::Response,
};
use config::ChainType;
use frame_metadata::decode_different::DecodeDifferent;
use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};
use parity_scale_codec::Decode;
use serde::Serialize;
use serde_json::json;
use subxt_rpcs::rpc_params;

// ============================================================================
// Query Parameters
// ============================================================================

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageQueryParams {
    pub at: Option<String>,
    /// When true, only return storage item names instead of full metadata
    #[serde(default)]
    pub only_ids: bool,
    /// When true, treat `at` as a relay chain block and find Asset Hub blocks within it
    #[serde(default)]
    pub use_rc_block: bool,
}

// ============================================================================
// Response Types (matching Sidecar format)
// ============================================================================

/// Response for /pallets/{palletId}/storage endpoint
/// When onlyIds=false (default): items contains full StorageItemMetadata
/// When onlyIds=true: items contains just strings (names)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsStorageResponse {
    pub at: AtResponse,
    pub pallet: String,
    /// Sidecar returns palletIndex as a string
    pub pallet_index: String,
    pub items: StorageItems,
    /// Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    /// Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    /// Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Storage items - either full metadata or just names
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum StorageItems {
    Full(Vec<StorageItemMetadata>),
    OnlyIds(Vec<String>),
}

/// Block information in the response
#[derive(Debug, Clone, Serialize)]
pub struct AtResponse {
    pub hash: String,
    pub height: String,
}

/// Metadata for a single storage item (matching Sidecar format)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageItemMetadata {
    pub name: String,
    pub modifier: String,
    #[serde(rename = "type")]
    pub ty: StorageTypeInfo,
    pub fallback: String,
    pub docs: String,
    pub deprecation_info: DeprecationInfo,
}

/// Storage type information - untagged enum for Sidecar format
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum StorageTypeInfo {
    Plain { plain: String },
    Map { map: MapTypeInfo },
}

#[derive(Debug, Serialize)]
pub struct MapTypeInfo {
    pub hashers: Vec<String>,
    pub key: String,
    pub value: String,
}

/// Sidecar format: { "notDeprecated": null } or { "deprecated": { note, since } }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeprecationInfo {
    NotDeprecated(Option<()>),
    Deprecated {
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        since: Option<String>,
    },
}

// ============================================================================
// Helper Functions
// ============================================================================

fn extract_str<'a>(s: &'a DecodeDifferent<&'static str, String>) -> &'a str {
    match s {
        DecodeDifferent::Decoded(v) => v.as_str(),
        DecodeDifferent::Encode(s) => s,
    }
}

fn extract_docs(docs: &DecodeDifferent<&'static [&'static str], Vec<String>>) -> String {
    match docs {
        DecodeDifferent::Decoded(v) => v.join("\n"),
        DecodeDifferent::Encode(s) => s.join("\n"),
    }
}

fn extract_default_bytes<G>(default: &DecodeDifferent<G, Vec<u8>>) -> String {
    match default {
        DecodeDifferent::Decoded(v) => format!("0x{}", hex::encode(v)),
        DecodeDifferent::Encode(_) => "0x".to_string(),
    }
}

// ============================================================================
// Main Handler
// ============================================================================

pub async fn get_pallets_storage(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<StorageQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, pallet_id, params).await;
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

    let resolved_block = utils::ResolvedBlock {
        hash: format!("{:#x}", client_at_block.block_hash()),
        number: client_at_block.block_number(),
    };

    // Fetch raw metadata via RPC to access all metadata versions (V9-V16)
    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let metadata = fetch_runtime_metadata(&state, &block_hash).await?;

    let response = build_storage_response(&metadata, &pallet_id, &resolved_block, params.only_ids)?;
    Ok(Json(response).into_response())
}

/// Fetch raw RuntimeMetadata via RPC and decode it
async fn fetch_runtime_metadata(
    state: &AppState,
    block_hash: &str,
) -> Result<RuntimeMetadata, PalletError> {
    let metadata_hex: String = state
        .rpc_client
        .request("state_getMetadata", rpc_params![block_hash])
        .await
        .map_err(|e| PalletError::PalletNotFound(format!("Failed to fetch metadata: {}", e)))?;

    let hex_str = metadata_hex.strip_prefix("0x").unwrap_or(&metadata_hex);
    let metadata_bytes = hex::decode(hex_str).map_err(|e| {
        PalletError::PalletNotFound(format!("Failed to decode metadata hex: {}", e))
    })?;

    let metadata_prefixed = RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])
        .map_err(|e| PalletError::PalletNotFound(format!("Failed to decode metadata: {}", e)))?;

    Ok(metadata_prefixed.1)
}

/// Handle useRcBlock parameter - find Asset Hub blocks within a Relay Chain block
async fn handle_use_rc_block(
    state: AppState,
    pallet_id: String,
    params: StorageQueryParams,
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
        return Ok(Json(json!([])).into_response());
    }

    let rc_block_number = rc_resolved_block.number.to_string();
    let rc_block_hash = rc_resolved_block.hash.clone();

    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved_block = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };

        // Fetch raw metadata via RPC for full version support
        let metadata = fetch_runtime_metadata(&state, &ah_block.hash).await?;

        let mut response =
            build_storage_response(&metadata, &pallet_id, &ah_resolved_block, params.only_ids)?;

        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        // Fetch timestamp via RPC
        let timestamp_key = "0x0d715f2646c8f85767b5d2764bb2782604a74d81251e398fd8a0a4d55023bb3f"; // Timestamp::Now storage key
        let timestamp_result: Option<String> = state
            .rpc_client
            .request(
                "state_getStorage",
                rpc_params![timestamp_key, &ah_block.hash],
            )
            .await
            .ok();

        if let Some(timestamp_hex) = timestamp_result {
            let hex_str = timestamp_hex.strip_prefix("0x").unwrap_or(&timestamp_hex);
            if let Ok(timestamp_bytes) = hex::decode(hex_str) {
                let mut cursor = &timestamp_bytes[..];
                if let Ok(timestamp_value) = u64::decode(&mut cursor) {
                    response.ah_timestamp = Some(timestamp_value.to_string());
                }
            }
        }

        results.push(response);
    }

    Ok(Json(json!(results)).into_response())
}

/// Build storage response from RuntimeMetadata for all supported versions
fn build_storage_response(
    metadata: &RuntimeMetadata,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
    only_ids: bool,
) -> Result<PalletsStorageResponse, PalletError> {
    use RuntimeMetadata::*;

    // Get full response from version-specific builder
    let full_response = match metadata {
        V9(meta) => build_storage_response_v9(meta, pallet_id, resolved_block),
        V10(meta) => build_storage_response_v10(meta, pallet_id, resolved_block),
        V11(meta) => build_storage_response_v11(meta, pallet_id, resolved_block),
        V12(meta) => build_storage_response_v12(meta, pallet_id, resolved_block),
        V13(meta) => build_storage_response_v13(meta, pallet_id, resolved_block),
        V14(meta) => build_storage_response_v14(meta, pallet_id, resolved_block),
        V15(meta) => build_storage_response_v15(meta, pallet_id, resolved_block),
        V16(meta) => build_storage_response_v16(meta, pallet_id, resolved_block),
        _ => {
            return Err(PalletError::PalletNotFound(
                "Unsupported metadata version".to_string(),
            ));
        }
    }?;

    // If only_ids requested, convert full items to just names
    if only_ids {
        let names: Vec<String> = match &full_response.items {
            StorageItems::Full(items) => items.iter().map(|item| item.name.clone()).collect(),
            StorageItems::OnlyIds(names) => names.clone(),
        };
        Ok(PalletsStorageResponse {
            at: full_response.at.clone(),
            pallet: full_response.pallet.clone(),
            pallet_index: full_response.pallet_index.clone(),
            items: StorageItems::OnlyIds(names),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        })
    } else {
        Ok(full_response)
    }
}

// ============================================================================
// Hasher conversion helpers
// ============================================================================

fn hasher_to_string_v9(hasher: &frame_metadata::v9::StorageHasher) -> String {
    use frame_metadata::v9::StorageHasher;
    match hasher {
        StorageHasher::Blake2_128 => "Blake2_128".to_string(),
        StorageHasher::Blake2_256 => "Blake2_256".to_string(),
        StorageHasher::Blake2_128Concat => "Blake2_128Concat".to_string(),
        StorageHasher::Twox128 => "Twox128".to_string(),
        StorageHasher::Twox256 => "Twox256".to_string(),
        StorageHasher::Twox64Concat => "Twox64Concat".to_string(),
    }
}
fn hasher_to_string_v10(hasher: &frame_metadata::v10::StorageHasher) -> String {
    use frame_metadata::v10::StorageHasher;
    match hasher {
        StorageHasher::Blake2_128 => "Blake2_128".to_string(),
        StorageHasher::Blake2_256 => "Blake2_256".to_string(),
        StorageHasher::Blake2_128Concat => "Blake2_128Concat".to_string(),
        StorageHasher::Twox128 => "Twox128".to_string(),
        StorageHasher::Twox256 => "Twox256".to_string(),
        StorageHasher::Twox64Concat => "Twox64Concat".to_string(),
    }
}

fn hasher_to_string_v11(hasher: &frame_metadata::v11::StorageHasher) -> String {
    use frame_metadata::v11::StorageHasher;
    match hasher {
        StorageHasher::Blake2_128 => "Blake2_128".to_string(),
        StorageHasher::Blake2_256 => "Blake2_256".to_string(),
        StorageHasher::Blake2_128Concat => "Blake2_128Concat".to_string(),
        StorageHasher::Twox128 => "Twox128".to_string(),
        StorageHasher::Twox256 => "Twox256".to_string(),
        StorageHasher::Twox64Concat => "Twox64Concat".to_string(),
        StorageHasher::Identity => "Identity".to_string(),
    }
}

fn hasher_to_string_v12(hasher: &frame_metadata::v12::StorageHasher) -> String {
    use frame_metadata::v12::StorageHasher;
    match hasher {
        StorageHasher::Blake2_128 => "Blake2_128".to_string(),
        StorageHasher::Blake2_256 => "Blake2_256".to_string(),
        StorageHasher::Blake2_128Concat => "Blake2_128Concat".to_string(),
        StorageHasher::Twox128 => "Twox128".to_string(),
        StorageHasher::Twox256 => "Twox256".to_string(),
        StorageHasher::Twox64Concat => "Twox64Concat".to_string(),
        StorageHasher::Identity => "Identity".to_string(),
    }
}

fn hasher_to_string_v13(hasher: &frame_metadata::v13::StorageHasher) -> String {
    use frame_metadata::v13::StorageHasher;
    match hasher {
        StorageHasher::Blake2_128 => "Blake2_128".to_string(),
        StorageHasher::Blake2_256 => "Blake2_256".to_string(),
        StorageHasher::Blake2_128Concat => "Blake2_128Concat".to_string(),
        StorageHasher::Twox128 => "Twox128".to_string(),
        StorageHasher::Twox256 => "Twox256".to_string(),
        StorageHasher::Twox64Concat => "Twox64Concat".to_string(),
        StorageHasher::Identity => "Identity".to_string(),
    }
}

fn hasher_to_string_v14(hasher: &frame_metadata::v14::StorageHasher) -> String {
    use frame_metadata::v14::StorageHasher;
    match hasher {
        StorageHasher::Blake2_128 => "Blake2_128".to_string(),
        StorageHasher::Blake2_256 => "Blake2_256".to_string(),
        StorageHasher::Blake2_128Concat => "Blake2_128Concat".to_string(),
        StorageHasher::Twox128 => "Twox128".to_string(),
        StorageHasher::Twox256 => "Twox256".to_string(),
        StorageHasher::Twox64Concat => "Twox64Concat".to_string(),
        StorageHasher::Identity => "Identity".to_string(),
    }
}

fn hasher_to_string_v16(hasher: &frame_metadata::v16::StorageHasher) -> String {
    use frame_metadata::v16::StorageHasher;
    match hasher {
        StorageHasher::Blake2_128 => "Blake2_128".to_string(),
        StorageHasher::Blake2_256 => "Blake2_256".to_string(),
        StorageHasher::Blake2_128Concat => "Blake2_128Concat".to_string(),
        StorageHasher::Twox128 => "Twox128".to_string(),
        StorageHasher::Twox256 => "Twox256".to_string(),
        StorageHasher::Twox64Concat => "Twox64Concat".to_string(),
        StorageHasher::Identity => "Identity".to_string(),
    }
}

// ============================================================================
// Type formatting helper (for PortableRegistry in V14+)
// Sidecar returns type IDs as strings, not resolved type names.
// TODO: Consider resolving type names from the registry for better readability
// if Sidecar compatibility is not required.
// ============================================================================

fn format_type_id(_types: &scale_info::PortableRegistry, type_id: u32) -> String {
    type_id.to_string()
}

// ============================================================================
// V16 deprecation helper
// ============================================================================

fn extract_deprecation_info_v16(
    info: &frame_metadata::v16::ItemDeprecationInfo<scale_info::form::PortableForm>,
) -> DeprecationInfo {
    use frame_metadata::v16::ItemDeprecationInfo;
    match info {
        ItemDeprecationInfo::NotDeprecated => DeprecationInfo::NotDeprecated(None),
        ItemDeprecationInfo::DeprecatedWithoutNote => DeprecationInfo::Deprecated {
            note: None,
            since: None,
        },
        ItemDeprecationInfo::Deprecated { note, since } => DeprecationInfo::Deprecated {
            note: Some(note.to_string()),
            since: since.as_ref().map(|s| s.to_string()),
        },
    }
}

// ============================================================================
// Version-specific Response Builders
// ============================================================================
//
// The following builders are organized by metadata version groups:
//
// GROUP 1: V9-V11 (Legacy, DecodeDifferent, no pallet index field)
//   - Use array position as pallet index
//   - Types encoded as strings via DecodeDifferent
//   - No NMap support
//   - Only difference between versions: StorageHasher enum variants
//
// GROUP 2: V12-V13 (Legacy, DecodeDifferent, has pallet index field)
//   - Pallets have explicit .index field
//   - V13 adds NMap storage type support
//
// GROUP 3: V14-V15 (Modern, PortableRegistry)
//   - Use scale_info::PortableRegistry for type resolution
//   - Types referenced by ID, not string names
//   - Identical structure (V15 reuses V14's StorageHasher)
//
// GROUP 4: V16 (Modern, PortableRegistry, with deprecation)
//   - Same as V14/V15 but adds deprecation_info field
//
// Note: Full consolidation via traits is not practical because each version
// has distinct Rust types from frame_metadata crate that are not trait-compatible.
// A macro could reduce source code duplication but wouldn't change the compiled output.
// ============================================================================

// ============================================================================
// V9 Response Builder (no index field - use array position)
// ============================================================================

fn build_storage_response_v9(
    meta: &frame_metadata::v9::RuntimeMetadataV9,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
) -> Result<PalletsStorageResponse, PalletError> {
    use frame_metadata::v9::{StorageEntryModifier, StorageEntryType};

    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    // Find module by name (case-insensitive) or numeric index (array position)
    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<usize>() {
        modules
            .get(idx)
            .map(|m| (m, idx as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .enumerate()
            .find(|(_, m)| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|(i, m)| (m, i as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let items = if let Some(storage) = &module.storage {
        let DecodeDifferent::Decoded(storage_meta) = storage else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        let DecodeDifferent::Decoded(entries) = &storage_meta.entries else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        entries
            .iter()
            .map(|entry| {
                let modifier = match entry.modifier {
                    StorageEntryModifier::Optional => "Optional",
                    StorageEntryModifier::Default => "Default",
                };

                let ty = match &entry.ty {
                    StorageEntryType::Plain(ty) => StorageTypeInfo::Plain {
                        plain: extract_str(ty).to_string(),
                    },
                    StorageEntryType::Map {
                        hasher, key, value, ..
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: vec![hasher_to_string_v9(hasher)],
                            key: extract_str(key).to_string(),
                            value: extract_str(value).to_string(),
                        },
                    },
                    StorageEntryType::DoubleMap {
                        hasher,
                        key1,
                        key2,
                        value,
                        key2_hasher,
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: vec![
                                hasher_to_string_v9(hasher),
                                hasher_to_string_v9(key2_hasher),
                            ],
                            key: format!("({}, {})", extract_str(key1), extract_str(key2)),
                            value: extract_str(value).to_string(),
                        },
                    },
                };

                StorageItemMetadata {
                    name: extract_str(&entry.name).to_string(),
                    modifier: modifier.to_string(),
                    ty,
                    fallback: extract_default_bytes(&entry.default),
                    docs: extract_docs(&entry.documentation),
                    deprecation_info: DeprecationInfo::NotDeprecated(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    Ok(PalletsStorageResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items: StorageItems::Full(items),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ============================================================================
// V10 Response Builder
// ============================================================================

fn build_storage_response_v10(
    meta: &frame_metadata::v10::RuntimeMetadataV10,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
) -> Result<PalletsStorageResponse, PalletError> {
    use frame_metadata::v10::{StorageEntryModifier, StorageEntryType};

    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<usize>() {
        modules
            .get(idx)
            .map(|m| (m, idx as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .enumerate()
            .find(|(_, m)| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|(i, m)| (m, i as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let items = if let Some(storage) = &module.storage {
        let DecodeDifferent::Decoded(storage_meta) = storage else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        let DecodeDifferent::Decoded(entries) = &storage_meta.entries else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        entries
            .iter()
            .map(|entry| {
                let modifier = match entry.modifier {
                    StorageEntryModifier::Optional => "Optional",
                    StorageEntryModifier::Default => "Default",
                };

                let ty = match &entry.ty {
                    StorageEntryType::Plain(ty) => StorageTypeInfo::Plain {
                        plain: extract_str(ty).to_string(),
                    },
                    StorageEntryType::Map {
                        hasher, key, value, ..
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: vec![hasher_to_string_v10(hasher)],
                            key: extract_str(key).to_string(),
                            value: extract_str(value).to_string(),
                        },
                    },
                    StorageEntryType::DoubleMap {
                        hasher,
                        key1,
                        key2,
                        value,
                        key2_hasher,
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: vec![
                                hasher_to_string_v10(hasher),
                                hasher_to_string_v10(key2_hasher),
                            ],
                            key: format!("({}, {})", extract_str(key1), extract_str(key2)),
                            value: extract_str(value).to_string(),
                        },
                    },
                };

                StorageItemMetadata {
                    name: extract_str(&entry.name).to_string(),
                    modifier: modifier.to_string(),
                    ty,
                    fallback: extract_default_bytes(&entry.default),
                    docs: extract_docs(&entry.documentation),
                    deprecation_info: DeprecationInfo::NotDeprecated(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    Ok(PalletsStorageResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items: StorageItems::Full(items),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ============================================================================
// V11 Response Builder
// ============================================================================

fn build_storage_response_v11(
    meta: &frame_metadata::v11::RuntimeMetadataV11,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
) -> Result<PalletsStorageResponse, PalletError> {
    use frame_metadata::v11::{StorageEntryModifier, StorageEntryType};

    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<usize>() {
        modules
            .get(idx)
            .map(|m| (m, idx as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .enumerate()
            .find(|(_, m)| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|(i, m)| (m, i as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let items = if let Some(storage) = &module.storage {
        let DecodeDifferent::Decoded(storage_meta) = storage else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        let DecodeDifferent::Decoded(entries) = &storage_meta.entries else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        entries
            .iter()
            .map(|entry| {
                let modifier = match entry.modifier {
                    StorageEntryModifier::Optional => "Optional",
                    StorageEntryModifier::Default => "Default",
                };

                let ty = match &entry.ty {
                    StorageEntryType::Plain(ty) => StorageTypeInfo::Plain {
                        plain: extract_str(ty).to_string(),
                    },
                    StorageEntryType::Map {
                        hasher, key, value, ..
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: vec![hasher_to_string_v11(hasher)],
                            key: extract_str(key).to_string(),
                            value: extract_str(value).to_string(),
                        },
                    },
                    StorageEntryType::DoubleMap {
                        hasher,
                        key1,
                        key2,
                        value,
                        key2_hasher,
                    } => {
                        let combined_key =
                            format!("({}, {})", extract_str(key1), extract_str(key2));
                        StorageTypeInfo::Map {
                            map: MapTypeInfo {
                                hashers: vec![
                                    hasher_to_string_v11(hasher),
                                    hasher_to_string_v11(key2_hasher),
                                ],
                                key: combined_key,
                                value: extract_str(value).to_string(),
                            },
                        }
                    }
                };

                StorageItemMetadata {
                    name: extract_str(&entry.name).to_string(),
                    modifier: modifier.to_string(),
                    ty,
                    fallback: extract_default_bytes(&entry.default),
                    docs: extract_docs(&entry.documentation),
                    deprecation_info: DeprecationInfo::NotDeprecated(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    Ok(PalletsStorageResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items: StorageItems::Full(items),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ============================================================================
// V12 Response Builder (has index field)
// ============================================================================

fn build_storage_response_v12(
    meta: &frame_metadata::v12::RuntimeMetadataV12,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
) -> Result<PalletsStorageResponse, PalletError> {
    use frame_metadata::v12::{StorageEntryModifier, StorageEntryType};

    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    // V12+ has .index field
    let module = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules.iter().find(|m| m.index == idx)
    } else {
        modules
            .iter()
            .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
    }
    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet_name = extract_str(&module.name).to_string();
    let module_index = module.index;

    let items = if let Some(storage) = &module.storage {
        let DecodeDifferent::Decoded(storage_meta) = storage else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        let DecodeDifferent::Decoded(entries) = &storage_meta.entries else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        entries
            .iter()
            .map(|entry| {
                let modifier = match entry.modifier {
                    StorageEntryModifier::Optional => "Optional",
                    StorageEntryModifier::Default => "Default",
                };

                let ty = match &entry.ty {
                    StorageEntryType::Plain(ty) => StorageTypeInfo::Plain {
                        plain: extract_str(ty).to_string(),
                    },
                    StorageEntryType::Map {
                        hasher, key, value, ..
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: vec![hasher_to_string_v12(hasher)],
                            key: extract_str(key).to_string(),
                            value: extract_str(value).to_string(),
                        },
                    },
                    StorageEntryType::DoubleMap {
                        hasher,
                        key1,
                        key2,
                        value,
                        key2_hasher,
                    } => {
                        let combined_key =
                            format!("({}, {})", extract_str(key1), extract_str(key2));
                        StorageTypeInfo::Map {
                            map: MapTypeInfo {
                                hashers: vec![
                                    hasher_to_string_v12(hasher),
                                    hasher_to_string_v12(key2_hasher),
                                ],
                                key: combined_key,
                                value: extract_str(value).to_string(),
                            },
                        }
                    }
                };

                StorageItemMetadata {
                    name: extract_str(&entry.name).to_string(),
                    modifier: modifier.to_string(),
                    ty,
                    fallback: extract_default_bytes(&entry.default),
                    docs: extract_docs(&entry.documentation),
                    deprecation_info: DeprecationInfo::NotDeprecated(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    Ok(PalletsStorageResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items: StorageItems::Full(items),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ============================================================================
// V13 Response Builder (adds NMap)
// ============================================================================

fn build_storage_response_v13(
    meta: &frame_metadata::v13::RuntimeMetadataV13,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
) -> Result<PalletsStorageResponse, PalletError> {
    use frame_metadata::v13::{StorageEntryModifier, StorageEntryType};

    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let module = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules.iter().find(|m| m.index == idx)
    } else {
        modules
            .iter()
            .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
    }
    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet_name = extract_str(&module.name).to_string();
    let module_index = module.index;

    let items = if let Some(storage) = &module.storage {
        let DecodeDifferent::Decoded(storage_meta) = storage else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        let DecodeDifferent::Decoded(entries) = &storage_meta.entries else {
            return Ok(PalletsStorageResponse {
                at: AtResponse {
                    hash: resolved_block.hash.clone(),
                    height: resolved_block.number.to_string(),
                },
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: StorageItems::Full(vec![]),
                rc_block_hash: None,
                rc_block_number: None,
                ah_timestamp: None,
            });
        };

        entries
            .iter()
            .map(|entry| {
                let modifier = match entry.modifier {
                    StorageEntryModifier::Optional => "Optional",
                    StorageEntryModifier::Default => "Default",
                };

                let ty = match &entry.ty {
                    StorageEntryType::Plain(ty) => StorageTypeInfo::Plain {
                        plain: extract_str(ty).to_string(),
                    },
                    StorageEntryType::Map {
                        hasher, key, value, ..
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: vec![hasher_to_string_v13(hasher)],
                            key: extract_str(key).to_string(),
                            value: extract_str(value).to_string(),
                        },
                    },
                    StorageEntryType::DoubleMap {
                        hasher,
                        key1,
                        key2,
                        value,
                        key2_hasher,
                    } => {
                        let combined_key =
                            format!("({}, {})", extract_str(key1), extract_str(key2));
                        StorageTypeInfo::Map {
                            map: MapTypeInfo {
                                hashers: vec![
                                    hasher_to_string_v13(hasher),
                                    hasher_to_string_v13(key2_hasher),
                                ],
                                key: combined_key,
                                value: extract_str(value).to_string(),
                            },
                        }
                    }
                    StorageEntryType::NMap {
                        keys,
                        hashers,
                        value,
                    } => {
                        let keys_str = match keys {
                            DecodeDifferent::Decoded(k) => {
                                if k.len() == 1 {
                                    k[0].to_string()
                                } else {
                                    format!("({})", k.join(", "))
                                }
                            }
                            DecodeDifferent::Encode(k) => {
                                if k.len() == 1 {
                                    k[0].to_string()
                                } else {
                                    format!("({})", k.join(", "))
                                }
                            }
                        };
                        let hashers_vec = match hashers {
                            DecodeDifferent::Decoded(h) => {
                                h.iter().map(hasher_to_string_v13).collect()
                            }
                            DecodeDifferent::Encode(_) => vec![],
                        };
                        StorageTypeInfo::Map {
                            map: MapTypeInfo {
                                hashers: hashers_vec,
                                key: keys_str,
                                value: extract_str(value).to_string(),
                            },
                        }
                    }
                };

                StorageItemMetadata {
                    name: extract_str(&entry.name).to_string(),
                    modifier: modifier.to_string(),
                    ty,
                    fallback: extract_default_bytes(&entry.default),
                    docs: extract_docs(&entry.documentation),
                    deprecation_info: DeprecationInfo::NotDeprecated(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    Ok(PalletsStorageResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items: StorageItems::Full(items),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ============================================================================
// V14 Response Builder (uses PortableRegistry)
// Note: V14 and V15 are structurally identical. V15's StorageHasher is the same
// as V14's. They could share implementation via a trait, but frame_metadata
// types are not trait-compatible across versions.
// ============================================================================

fn build_storage_response_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
) -> Result<PalletsStorageResponse, PalletError> {
    use frame_metadata::v14::{StorageEntryModifier, StorageEntryType};

    let pallet = if let Ok(idx) = pallet_id.parse::<u8>() {
        meta.pallets.iter().find(|p| p.index == idx)
    } else {
        meta.pallets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(pallet_id))
    }
    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let items = if let Some(storage) = &pallet.storage {
        storage
            .entries
            .iter()
            .map(|entry| {
                let modifier = match entry.modifier {
                    StorageEntryModifier::Optional => "Optional",
                    StorageEntryModifier::Default => "Default",
                };

                let ty = match &entry.ty {
                    StorageEntryType::Plain(ty) => StorageTypeInfo::Plain {
                        plain: format_type_id(&meta.types, ty.id),
                    },
                    StorageEntryType::Map {
                        hashers,
                        key,
                        value,
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: hashers.iter().map(hasher_to_string_v14).collect(),
                            key: format_type_id(&meta.types, key.id),
                            value: format_type_id(&meta.types, value.id),
                        },
                    },
                };

                StorageItemMetadata {
                    name: entry.name.clone(),
                    modifier: modifier.to_string(),
                    ty,
                    fallback: format!("0x{}", hex::encode(&entry.default)),
                    docs: entry.docs.join("\n"),
                    deprecation_info: DeprecationInfo::NotDeprecated(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    Ok(PalletsStorageResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: pallet.name.to_lowercase(),
        pallet_index: pallet.index.to_string(),
        items: StorageItems::Full(items),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ============================================================================
// V15 Response Builder
// ============================================================================

fn build_storage_response_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
) -> Result<PalletsStorageResponse, PalletError> {
    use frame_metadata::v15::{StorageEntryModifier, StorageEntryType};

    let pallet = if let Ok(idx) = pallet_id.parse::<u8>() {
        meta.pallets.iter().find(|p| p.index == idx)
    } else {
        meta.pallets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(pallet_id))
    }
    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let items = if let Some(storage) = &pallet.storage {
        storage
            .entries
            .iter()
            .map(|entry| {
                let modifier = match entry.modifier {
                    StorageEntryModifier::Optional => "Optional",
                    StorageEntryModifier::Default => "Default",
                };

                let ty = match &entry.ty {
                    StorageEntryType::Plain(ty) => StorageTypeInfo::Plain {
                        plain: format_type_id(&meta.types, ty.id),
                    },
                    StorageEntryType::Map {
                        hashers,
                        key,
                        value,
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: hashers.iter().map(hasher_to_string_v14).collect(),
                            key: format_type_id(&meta.types, key.id),
                            value: format_type_id(&meta.types, value.id),
                        },
                    },
                };

                StorageItemMetadata {
                    name: entry.name.clone(),
                    modifier: modifier.to_string(),
                    ty,
                    fallback: format!("0x{}", hex::encode(&entry.default)),
                    docs: entry.docs.join("\n"),
                    deprecation_info: DeprecationInfo::NotDeprecated(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    Ok(PalletsStorageResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: pallet.name.to_lowercase(),
        pallet_index: pallet.index.to_string(),
        items: StorageItems::Full(items),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ============================================================================
// V16 Response Builder (adds deprecation_info)
// ============================================================================

fn build_storage_response_v16(
    meta: &frame_metadata::v16::RuntimeMetadataV16,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
) -> Result<PalletsStorageResponse, PalletError> {
    use frame_metadata::v16::{StorageEntryModifier, StorageEntryType};

    let pallet = if let Ok(idx) = pallet_id.parse::<u8>() {
        meta.pallets.iter().find(|p| p.index == idx)
    } else {
        meta.pallets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(pallet_id))
    }
    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let items = if let Some(storage) = &pallet.storage {
        storage
            .entries
            .iter()
            .map(|entry| {
                let modifier = match entry.modifier {
                    StorageEntryModifier::Optional => "Optional",
                    StorageEntryModifier::Default => "Default",
                };

                let ty = match &entry.ty {
                    StorageEntryType::Plain(ty) => StorageTypeInfo::Plain {
                        plain: format_type_id(&meta.types, ty.id),
                    },
                    StorageEntryType::Map {
                        hashers,
                        key,
                        value,
                    } => StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: hashers.iter().map(hasher_to_string_v16).collect(),
                            key: format_type_id(&meta.types, key.id),
                            value: format_type_id(&meta.types, value.id),
                        },
                    },
                };

                StorageItemMetadata {
                    name: entry.name.clone(),
                    modifier: modifier.to_string(),
                    ty,
                    fallback: format!("0x{}", hex::encode(&entry.default)),
                    docs: entry.docs.join("\n"),
                    deprecation_info: extract_deprecation_info_v16(&entry.deprecation_info),
                }
            })
            .collect()
    } else {
        vec![]
    };

    Ok(PalletsStorageResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: pallet.name.to_lowercase(),
        pallet_index: pallet.index.to_string(),
        items: StorageItems::Full(items),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}
