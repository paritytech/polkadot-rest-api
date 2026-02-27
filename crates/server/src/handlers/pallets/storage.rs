// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for `/pallets/{palletId}/storage` and `/pallets/{palletId}/storage/{storageItemId}` endpoints.
//!
//! Returns storage item metadata for a pallet, matching Sidecar's response format.
//! Supports all metadata versions V9-V16.

// Allow large error types - PalletError contains subxt::error::OnlineClientAtBlockError
// which is large by design. Boxing would add indirection without significant benefit.
#![allow(clippy::result_large_err)]

use crate::extractors::{JsonQuery, QsQuery};
use crate::handlers::pallets::common::{
    PalletError, RcPalletQueryParams, resolve_block_for_pallet,
};
use crate::state::AppState;
use crate::utils;
use crate::utils::format::to_camel_case;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{Json, extract::Path, extract::State, response::IntoResponse, response::Response};
use frame_metadata::decode_different::DecodeDifferent;
use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};
use parity_scale_codec::Decode;
use polkadot_rest_api_config::ChainType;
use serde::Serialize;
use serde_json::json;
use subxt_rpcs::{RpcClient, rpc_params};
use utoipa::ToSchema;

// ============================================================================
// Query Parameters
// ============================================================================

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AtResponse {
    pub hash: String,
    pub height: String,
}

/// Metadata for a single storage item (matching Sidecar format)
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum StorageTypeInfo {
    Plain { plain: String },
    Map { map: MapTypeInfo },
}

#[derive(Debug, Clone, Serialize)]
pub struct MapTypeInfo {
    pub hashers: Vec<String>,
    pub key: String,
    pub value: String,
}

/// Sidecar format: { "notDeprecated": null } or { "deprecated": { note, since } }
#[derive(Debug, Clone, Serialize)]
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
// Storage Item Query Parameters and Response Types
// ============================================================================

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageItemQueryParams {
    pub at: Option<String>,
    /// Storage keys for map types (format: ?keys[]=key1&keys[]=key2)
    /// Note: serde_qs handles bracket notation automatically, no rename needed
    #[serde(default)]
    pub keys: Vec<String>,
    /// When true, include storage item metadata in response
    #[serde(default)]
    pub metadata: bool,
    /// When true, treat `at` as a relay chain block and find Asset Hub blocks within it
    #[serde(default)]
    pub use_rc_block: bool,
}

/// Response for /pallets/{palletId}/storage/{storageItemId} endpoint
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsStorageItemResponse {
    pub at: AtResponse,
    pub pallet: String,
    pub pallet_index: String,
    pub storage_item: String,
    pub keys: Vec<String>,
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<StorageItemMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
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

#[utoipa::path(
    get,
    path = "/v1/pallets/{palletId}/storage",
    tag = "pallets",
    summary = "Pallet storage items",
    description = "Returns the list of storage items for a given pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("onlyIds" = Option<bool>, description = "Only return storage item names"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Pallet storage items", body = Object),
        (status = 400, description = "Invalid pallet or parameters"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_pallets_storage(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    JsonQuery(params): JsonQuery<StorageQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, pallet_id, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let resolved_block = utils::ResolvedBlock {
        hash: resolved.at.hash.clone(),
        number: resolved.client_at_block.block_number(),
    };

    // Fetch raw metadata via RPC to access all metadata versions (V9-V16)
    let block_hash = &resolved.at.hash;
    let metadata = fetch_runtime_metadata(&state.rpc_client, block_hash).await?;

    let response = build_storage_response(&metadata, &pallet_id, &resolved_block, params.only_ids)?;
    Ok(Json(response).into_response())
}

/// Fetch raw RuntimeMetadata via RPC and decode it
async fn fetch_runtime_metadata(
    rpc_client: &RpcClient,
    block_hash: &str,
) -> Result<RuntimeMetadata, PalletError> {
    let metadata_hex: String = rpc_client
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

    state.get_relay_chain_client().await?;

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    let rc_rpc = state.get_relay_chain_rpc().await?;

    let rc_resolved_block =
        utils::resolve_block_with_rpc(&rc_rpc_client, &rc_rpc, Some(rc_block_id)).await?;

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
        let metadata = fetch_runtime_metadata(&state.rpc_client, &ah_block.hash).await?;

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

// ============================================================================
// Storage Item Handler
// ============================================================================

#[utoipa::path(
    get,
    path = "/v1/pallets/{palletId}/storage/{storageItemId}",
    tag = "pallets",
    summary = "Pallet storage item value",
    description = "Returns the value of a specific storage item in a pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("storageItemId" = String, Path, description = "Name of the storage item"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("keys[]" = Option<Vec<String>>, description = "Storage key arguments"),
        ("metadata" = Option<bool>, description = "Include metadata for the storage item"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Storage item value", body = Object),
        (status = 400, description = "Invalid parameters"),
        (status = 404, description = "Storage item not found"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_pallets_storage_item(
    State(state): State<AppState>,
    Path((pallet_id, storage_item_id)): Path<(String, String)>,
    QsQuery(params): QsQuery<StorageItemQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_storage_item_use_rc_block(state, pallet_id, storage_item_id, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let resolved_block = utils::ResolvedBlock {
        hash: resolved.at.hash.clone(),
        number: resolved.client_at_block.block_number(),
    };

    let block_hash = &resolved.at.hash;
    let metadata = fetch_runtime_metadata(&state.rpc_client, block_hash).await?;

    let response = build_storage_item_response(
        &state.rpc_client,
        &metadata,
        &pallet_id,
        &storage_item_id,
        &params.keys,
        &resolved_block,
        params.metadata,
        block_hash,
    )
    .await?;

    Ok(Json(response).into_response())
}

/// Handle useRcBlock for storage item endpoint
async fn handle_storage_item_use_rc_block(
    state: AppState,
    pallet_id: String,
    storage_item_id: String,
    params: StorageItemQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    state.get_relay_chain_client().await?;

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    let rc_rpc = state.get_relay_chain_rpc().await?;

    let rc_resolved_block =
        utils::resolve_block_with_rpc(&rc_rpc_client, &rc_rpc, Some(rc_block_id)).await?;

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

        let metadata = fetch_runtime_metadata(&state.rpc_client, &ah_block.hash).await?;

        let mut response = build_storage_item_response(
            &state.rpc_client,
            &metadata,
            &pallet_id,
            &storage_item_id,
            &params.keys,
            &ah_resolved_block,
            params.metadata,
            &ah_block.hash,
        )
        .await?;

        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        // Fetch timestamp via RPC
        let timestamp_key = "0x0d715f2646c8f85767b5d2764bb2782604a74d81251e398fd8a0a4d55023bb3f";
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

/// Build storage item response - query actual storage value
#[allow(clippy::too_many_arguments)]
async fn build_storage_item_response(
    rpc_client: &RpcClient,
    metadata: &RuntimeMetadata,
    pallet_id: &str,
    storage_item_id: &str,
    keys: &[String],
    resolved_block: &utils::ResolvedBlock,
    include_metadata: bool,
    block_hash: &str,
) -> Result<PalletsStorageItemResponse, PalletError> {
    // First get the storage metadata to find the item and build the key
    let storage_response = build_storage_response(metadata, pallet_id, resolved_block, false)?;

    let storage_items = match &storage_response.items {
        StorageItems::Full(items) => items,
        StorageItems::OnlyIds(_) => {
            return Err(PalletError::StorageItemNotFound {
                pallet: pallet_id.to_string(),
                item: storage_item_id.to_string(),
            });
        }
    };

    // Find the storage item by name (case-insensitive)
    let storage_item = storage_items
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(storage_item_id))
        .ok_or_else(|| PalletError::StorageItemNotFound {
            pallet: pallet_id.to_string(),
            item: storage_item_id.to_string(),
        })?;

    // Get the original pallet name (PascalCase) for storage key building
    let original_pallet_name = get_original_pallet_name(metadata, pallet_id)?;

    // Build storage key and query value - use original (PascalCase) pallet name
    let storage_key = build_storage_key(
        &original_pallet_name,
        &storage_item.name,
        keys,
        &storage_item.ty,
    )?;

    // Query storage value via RPC
    let value_hex: Option<String> = rpc_client
        .request("state_getStorage", rpc_params![&storage_key, block_hash])
        .await
        .ok();

    // Decode value - for now return as raw hex or decoded number for simple types
    let value = decode_storage_value(value_hex, &storage_item.ty)?;

    let metadata_field = if include_metadata {
        Some(storage_item.clone())
    } else {
        None
    };

    Ok(PalletsStorageItemResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: storage_response.pallet,
        pallet_index: storage_response.pallet_index,
        storage_item: to_camel_case(&storage_item.name),
        keys: keys.to_vec(),
        value,
        metadata: metadata_field,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

/// Get the original pallet name (PascalCase) from metadata
fn get_original_pallet_name(
    metadata: &RuntimeMetadata,
    pallet_id: &str,
) -> Result<String, PalletError> {
    use RuntimeMetadata::*;

    match metadata {
        V14(meta) => {
            if let Ok(idx) = pallet_id.parse::<u8>() {
                meta.pallets
                    .iter()
                    .find(|p| p.index == idx)
                    .map(|p| p.name.clone())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            } else {
                meta.pallets
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(pallet_id))
                    .map(|p| p.name.clone())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            }
        }
        V15(meta) => {
            if let Ok(idx) = pallet_id.parse::<u8>() {
                meta.pallets
                    .iter()
                    .find(|p| p.index == idx)
                    .map(|p| p.name.clone())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            } else {
                meta.pallets
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(pallet_id))
                    .map(|p| p.name.clone())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            }
        }
        V16(meta) => {
            if let Ok(idx) = pallet_id.parse::<u8>() {
                meta.pallets
                    .iter()
                    .find(|p| p.index == idx)
                    .map(|p| p.name.clone())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            } else {
                meta.pallets
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(pallet_id))
                    .map(|p| p.name.clone())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            }
        }
        // For older versions (V9-V13), pallet name comes from module.name
        V9(meta) => {
            let DecodeDifferent::Decoded(modules) = &meta.modules else {
                return Err(PalletError::PalletNotFound(pallet_id.to_string()));
            };
            if let Ok(idx) = pallet_id.parse::<usize>() {
                modules
                    .get(idx)
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            } else {
                modules
                    .iter()
                    .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            }
        }
        V10(meta) => {
            let DecodeDifferent::Decoded(modules) = &meta.modules else {
                return Err(PalletError::PalletNotFound(pallet_id.to_string()));
            };
            if let Ok(idx) = pallet_id.parse::<usize>() {
                modules
                    .get(idx)
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            } else {
                modules
                    .iter()
                    .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            }
        }
        V11(meta) => {
            let DecodeDifferent::Decoded(modules) = &meta.modules else {
                return Err(PalletError::PalletNotFound(pallet_id.to_string()));
            };
            if let Ok(idx) = pallet_id.parse::<usize>() {
                modules
                    .get(idx)
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            } else {
                modules
                    .iter()
                    .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            }
        }
        V12(meta) => {
            let DecodeDifferent::Decoded(modules) = &meta.modules else {
                return Err(PalletError::PalletNotFound(pallet_id.to_string()));
            };
            if let Ok(idx) = pallet_id.parse::<u8>() {
                modules
                    .iter()
                    .find(|m| m.index == idx)
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            } else {
                modules
                    .iter()
                    .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            }
        }
        V13(meta) => {
            let DecodeDifferent::Decoded(modules) = &meta.modules else {
                return Err(PalletError::PalletNotFound(pallet_id.to_string()));
            };
            if let Ok(idx) = pallet_id.parse::<u8>() {
                modules
                    .iter()
                    .find(|m| m.index == idx)
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            } else {
                modules
                    .iter()
                    .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
                    .map(|m| extract_str(&m.name).to_string())
                    .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
            }
        }
        _ => Err(PalletError::PalletNotFound(
            "Unsupported metadata version".to_string(),
        )),
    }
}

/// Hash a key using the specified hasher type
fn hash_key(key_bytes: &[u8], hasher: &str) -> Vec<u8> {
    use sp_crypto_hashing::{blake2_128, blake2_256, twox_64, twox_128, twox_256};

    match hasher {
        "Blake2_128" => blake2_128(key_bytes).to_vec(),
        "Blake2_256" => blake2_256(key_bytes).to_vec(),
        "Blake2_128Concat" => {
            let mut result = blake2_128(key_bytes).to_vec();
            result.extend_from_slice(key_bytes);
            result
        }
        "Twox64Concat" => {
            let mut result = twox_64(key_bytes).to_vec();
            result.extend_from_slice(key_bytes);
            result
        }
        "Twox128" => twox_128(key_bytes).to_vec(),
        "Twox256" => twox_256(key_bytes).to_vec(),
        "Identity" => key_bytes.to_vec(),
        _ => {
            // Unknown hasher, use identity as fallback
            key_bytes.to_vec()
        }
    }
}

/// Encode a storage key value to bytes based on expected type
///
/// For V14+ metadata, `key_type_id` is a type ID that can be looked up.
/// For older metadata, `key_type_id` is a type name string like "T::AccountId".
///
/// The function handles:
/// - SS58 addresses (AccountId)
/// - Hex-encoded bytes (0x...)
/// - Numeric types (u8, u16, u32, u64, u128) with proper sizing based on type hint
/// - Boolean values
fn encode_key_value(key: &str, key_type_id: &str) -> Result<Vec<u8>, PalletError> {
    use sp_core::crypto::Ss58Codec;

    // First, try to decode as hex - this is explicit and takes priority
    if let Some(hex_str) = key.strip_prefix("0x")
        && let Ok(bytes) = hex::decode(hex_str)
    {
        return Ok(bytes);
    }

    // Try to decode as SS58 address (for AccountId keys)
    if let Ok(account_id) = sp_core::crypto::AccountId32::from_ss58check(key) {
        let bytes: &[u8; 32] = account_id.as_ref();
        return Ok(bytes.to_vec());
    }

    // Use type hint to determine encoding
    // For V14+, key_type_id is a number; for older versions it's a type name
    let type_hint = key_type_id.to_lowercase();

    // Check for known type patterns
    if type_hint.contains("bool") || key == "true" || key == "false" {
        let value: bool = key.parse().unwrap_or(false);
        return Ok(vec![value as u8]);
    }

    // Handle numeric types - check type hint first for proper sizing
    if type_hint.contains("u8")
        && !type_hint.contains("u128")
        && let Ok(num) = key.parse::<u8>()
    {
        return Ok(vec![num]);
    }

    if type_hint.contains("u16")
        && !type_hint.contains("u128")
        && let Ok(num) = key.parse::<u16>()
    {
        return Ok(num.to_le_bytes().to_vec());
    }

    if type_hint.contains("u32")
        && !type_hint.contains("u128")
        && let Ok(num) = key.parse::<u32>()
    {
        return Ok(num.to_le_bytes().to_vec());
    }

    if type_hint.contains("u64")
        && !type_hint.contains("u128")
        && let Ok(num) = key.parse::<u64>()
    {
        return Ok(num.to_le_bytes().to_vec());
    }

    if type_hint.contains("u128")
        && let Ok(num) = key.parse::<u128>()
    {
        return Ok(num.to_le_bytes().to_vec());
    }

    // For V14+ type IDs, we don't have the type name directly
    // Try to infer from the value format
    if key_type_id.parse::<u32>().is_ok() {
        // It's a V14+ type ID - try to parse value intelligently
        return decode_key_value_auto(key);
    }

    // Fallback for older metadata with type names
    decode_key_value_auto(key)
}

/// Auto-detect and decode a key value when type is unknown
fn decode_key_value_auto(key: &str) -> Result<Vec<u8>, PalletError> {
    use sp_core::crypto::Ss58Codec;

    // Try SS58 address
    if let Ok(account_id) = sp_core::crypto::AccountId32::from_ss58check(key) {
        let bytes: &[u8; 32] = account_id.as_ref();
        return Ok(bytes.to_vec());
    }

    // Try hex
    if let Some(hex_str) = key.strip_prefix("0x")
        && let Ok(bytes) = hex::decode(hex_str)
    {
        return Ok(bytes);
    }

    // Try boolean
    if key == "true" {
        return Ok(vec![1]);
    }
    if key == "false" {
        return Ok(vec![0]);
    }

    // Try parsing as number
    // Default to u32 for numeric keys since block numbers, indices, etc. are typically u32
    // This matches Substrate's common conventions
    if let Ok(num) = key.parse::<u32>() {
        return Ok(num.to_le_bytes().to_vec());
    }

    if let Ok(num) = key.parse::<u64>() {
        return Ok(num.to_le_bytes().to_vec());
    }

    if let Ok(num) = key.parse::<u128>() {
        return Ok(num.to_le_bytes().to_vec());
    }

    // Last resort: raw bytes (unlikely to be correct but better than failing)
    Err(PalletError::PalletNotFound(format!(
        "Unable to encode key '{}' - provide as hex (0x...) or valid SS58 address",
        key
    )))
}

/// Build storage key from pallet name, storage item name, and optional keys
fn build_storage_key(
    pallet_name: &str,
    storage_name: &str,
    keys: &[String],
    storage_type: &StorageTypeInfo,
) -> Result<String, PalletError> {
    use sp_crypto_hashing::twox_128;

    // Storage key prefix: twox128(pallet_name) ++ twox128(storage_name)
    let pallet_hash = twox_128(pallet_name.as_bytes());
    let storage_hash = twox_128(storage_name.as_bytes());

    let mut key = Vec::with_capacity(32 + keys.len() * 64);
    key.extend_from_slice(&pallet_hash);
    key.extend_from_slice(&storage_hash);

    // For maps, hash each key using the corresponding hasher
    if !keys.is_empty() {
        let (hashers, key_type) = match storage_type {
            StorageTypeInfo::Map { map } => (&map.hashers, &map.key),
            StorageTypeInfo::Plain { .. } => {
                return Err(PalletError::PalletNotFound(
                    "Keys provided for plain storage type".to_string(),
                ));
            }
        };

        // Validate key count matches hasher count
        if keys.len() != hashers.len() {
            return Err(PalletError::PalletNotFound(format!(
                "Expected {} key(s) but got {}",
                hashers.len(),
                keys.len()
            )));
        }

        // Hash each key with its corresponding hasher
        for (key_str, hasher) in keys.iter().zip(hashers.iter()) {
            let key_bytes = encode_key_value(key_str, key_type)?;
            let hashed_key = hash_key(&key_bytes, hasher);
            key.extend_from_slice(&hashed_key);
        }
    }

    Ok(format!("0x{}", hex::encode(key)))
}

/// Decode storage value from hex
fn decode_storage_value(
    value_hex: Option<String>,
    storage_type: &StorageTypeInfo,
) -> Result<serde_json::Value, PalletError> {
    let Some(hex_str) = value_hex else {
        return Ok(serde_json::Value::Null);
    };

    let hex_clean = hex_str.strip_prefix("0x").unwrap_or(&hex_str);
    let bytes = hex::decode(hex_clean).map_err(|e| {
        PalletError::PalletNotFound(format!("Failed to decode storage value hex: {}", e))
    })?;

    // Try to decode based on storage type
    match storage_type {
        StorageTypeInfo::Plain { plain } => {
            // Common type IDs in Polkadot metadata:
            // 4 = u32 (block number)
            // 8 = u64 (timestamp)
            // For now, try common decodings
            if let Ok(type_id) = plain.parse::<u32>() {
                match type_id {
                    4 if bytes.len() >= 4 => {
                        // u32
                        let value = u32::from_le_bytes(bytes[..4].try_into().unwrap_or([0; 4]));
                        return Ok(serde_json::Value::String(value.to_string()));
                    }
                    8 if bytes.len() >= 8 => {
                        // u64
                        let value = u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0; 8]));
                        return Ok(serde_json::Value::String(value.to_string()));
                    }
                    _ => {}
                }
            }

            // Fallback: try to decode as various types based on byte length
            match bytes.len() {
                1 => {
                    let value = bytes[0];
                    Ok(serde_json::Value::String(value.to_string()))
                }
                2 => {
                    let value = u16::from_le_bytes(bytes[..2].try_into().unwrap_or([0; 2]));
                    Ok(serde_json::Value::String(value.to_string()))
                }
                4 => {
                    let value = u32::from_le_bytes(bytes[..4].try_into().unwrap_or([0; 4]));
                    Ok(serde_json::Value::String(value.to_string()))
                }
                8 => {
                    let value = u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0; 8]));
                    Ok(serde_json::Value::String(value.to_string()))
                }
                16 => {
                    let value = u128::from_le_bytes(bytes[..16].try_into().unwrap_or([0; 16]));
                    Ok(serde_json::Value::String(value.to_string()))
                }
                _ => {
                    // Return raw hex for complex types
                    Ok(serde_json::Value::String(format!("0x{}", hex_clean)))
                }
            }
        }
        StorageTypeInfo::Map { .. } => {
            // For maps, return raw hex
            Ok(serde_json::Value::String(format!("0x{}", hex_clean)))
        }
    }
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

// ============================================================================
// RC (Relay Chain) Handlers
// ============================================================================

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RcStorageItemQueryParams {
    pub at: Option<String>,
    /// Storage keys for map types (format: ?keys[]=key1&keys[]=key2)
    /// Note: serde_qs handles bracket notation automatically, no rename needed
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub metadata: bool,
}

/// Handler for GET `/rc/pallets/{palletId}/storage`
///
/// Returns storage items from the relay chain's pallet metadata.
#[utoipa::path(
    get,
    path = "/v1/rc/pallets/{palletId}/storage",
    tag = "rc",
    summary = "RC pallet storage items",
    description = "Returns the list of storage items for a relay chain pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("onlyIds" = Option<bool>, description = "Only return storage item names")
    ),
    responses(
        (status = 200, description = "Relay chain pallet storage items", body = Object),
        (status = 400, description = "Invalid pallet or parameters"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_get_pallets_storage(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    JsonQuery(params): JsonQuery<RcPalletQueryParams>,
) -> Result<Response, PalletError> {
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_rpc = state.get_relay_chain_rpc().await?;

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;
    let resolved =
        crate::utils::resolve_block_with_rpc(&relay_rpc_client, &relay_rpc, block_id).await?;

    let block_hash = resolved.hash.clone();
    let metadata = fetch_runtime_metadata(&relay_rpc_client, &block_hash).await?;

    let response = build_storage_response(&metadata, &pallet_id, &resolved, params.only_ids)?;
    Ok(Json(response).into_response())
}

/// Handler for GET `/rc/pallets/{palletId}/storage/{storageItemId}`
///
/// Returns a specific storage item from the relay chain.
#[utoipa::path(
    get,
    path = "/v1/rc/pallets/{palletId}/storage/{storageItemId}",
    tag = "rc",
    summary = "RC pallet storage item value",
    description = "Returns the value of a specific storage item from a relay chain pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("storageItemId" = String, Path, description = "Name of the storage item"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("keys[]" = Option<Vec<String>>, description = "Storage key arguments"),
        ("metadata" = Option<bool>, description = "Include metadata for the storage item")
    ),
    responses(
        (status = 200, description = "Relay chain storage item value", body = Object),
        (status = 400, description = "Invalid parameters"),
        (status = 404, description = "Storage item not found"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_get_pallets_storage_item(
    State(state): State<AppState>,
    Path((pallet_id, storage_item_id)): Path<(String, String)>,
    QsQuery(params): QsQuery<RcStorageItemQueryParams>,
) -> Result<Response, PalletError> {
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_rpc = state.get_relay_chain_rpc().await?;

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;
    let resolved =
        crate::utils::resolve_block_with_rpc(&relay_rpc_client, &relay_rpc, block_id).await?;

    let block_hash = resolved.hash.clone();
    let metadata = fetch_runtime_metadata(&relay_rpc_client, &block_hash).await?;

    let response = build_storage_item_response(
        &relay_rpc_client,
        &metadata,
        &pallet_id,
        &storage_item_id,
        &params.keys,
        &resolved,
        params.metadata,
        &block_hash,
    )
    .await?;

    Ok(Json(response).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<StorageQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_storage_item_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<StorageItemQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_storage_item_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<RcStorageItemQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
