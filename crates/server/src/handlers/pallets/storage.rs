// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for `/pallets/{palletId}/storage` and `/pallets/{palletId}/storage/{storageItemId}` endpoints.
//!
//! Returns storage item metadata for a pallet, matching Sidecar's response format.
//! Uses subxt's cached metadata for all metadata versions.

// Allow large error types - PalletError contains subxt::error::OnlineClientAtBlockError
// which is large by design. Boxing would add indirection without significant benefit.
#![allow(clippy::result_large_err)]

use crate::extractors::{JsonQuery, QsQuery};
use crate::handlers::blocks::decode::JsonVisitor;
use crate::handlers::pallets::common::{
    PalletError, RcPalletQueryParams, resolve_block_for_pallet,
};
use crate::state::AppState;
use crate::utils;
use crate::utils::format::to_camel_case;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{Json, extract::Path, extract::State, response::IntoResponse, response::Response};
use parity_scale_codec::Decode;
use polkadot_rest_api_config::ChainType;
use scale_decode::visitor::decode_with_visitor;
use serde::Serialize;
use serde_json::json;
use subxt::Metadata;
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
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("onlyIds" = Option<bool>, Query, description = "Only return storage item names"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier")
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

    // Resolve block and use subxt's cached metadata (same as other pallet handlers)
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let resolved_block = utils::ResolvedBlock {
        hash: resolved.at.hash.clone(),
        number: resolved.client_at_block.block_number(),
    };
    let metadata = resolved.client_at_block.metadata();
    let response = build_storage_response(&metadata, &pallet_id, &resolved_block, params.only_ids)?;
    Ok(Json(response).into_response())
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

        // Use subxt's cached metadata (same pattern as all other handlers)
        let client_at_block = state.client.at_block(ah_block.number).await?;
        let metadata = client_at_block.metadata();

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
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("keys[]" = Option<Vec<String>>, Query, description = "Storage key arguments"),
        ("metadata" = Option<bool>, Query, description = "Include metadata for the storage item"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier")
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

    // Resolve block and use subxt's cached metadata (same as other pallet handlers)
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let resolved_block = utils::ResolvedBlock {
        hash: resolved.at.hash.clone(),
        number: resolved.client_at_block.block_number(),
    };
    let block_hash = &resolved.at.hash;
    let metadata = resolved.client_at_block.metadata();
    let response = build_storage_item_response(
        &state.rpc_client,
        &metadata,
        &pallet_id,
        &storage_item_id,
        &params.keys,
        &resolved_block,
        params.metadata,
        block_hash,
        state.chain_info.ss58_prefix,
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

        // Use subxt's cached metadata (same pattern as all other handlers)
        let client_at_block = state.client.at_block(ah_block.number).await?;
        let metadata = client_at_block.metadata();

        let mut response = build_storage_item_response(
            &state.rpc_client,
            &metadata,
            &pallet_id,
            &storage_item_id,
            &params.keys,
            &ah_resolved_block,
            params.metadata,
            &ah_block.hash,
            state.chain_info.ss58_prefix,
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
/// `key_type_id` is a numeric type ID string from subxt's normalized metadata.
/// Subxt normalizes all metadata versions (V9-V16) to use numeric type IDs.
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

    // Use type hint to determine encoding (numeric type ID from subxt)
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

    // Numeric type ID — infer encoding from the value format
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

    Err(PalletError::PalletNotFound(format!(
        "Unable to encode key '{}'",
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
/// Decode storage value from hex using the metadata type registry.
///
/// Uses the `JsonVisitor` (from blocks/decode/args.rs) to decode SCALE-encoded bytes
/// against the type registry, producing properly-typed JSON output including:
/// - AccountId32 → SS58 addresses
/// - Enums → variant names / objects
/// - Structs → JSON objects with camelCase keys
/// - Options → null / value
fn decode_storage_value(
    value_hex: Option<String>,
    storage_type: &StorageTypeInfo,
    metadata: &Metadata,
    ss58_prefix: u16,
) -> Result<serde_json::Value, PalletError> {
    let Some(hex_str) = value_hex else {
        return Ok(serde_json::Value::Null);
    };

    let hex_clean = hex_str.strip_prefix("0x").unwrap_or(&hex_str);
    let bytes = hex::decode(hex_clean).map_err(|e| {
        PalletError::PalletNotFound(format!("Failed to decode storage value hex: {}", e))
    })?;

    // Get the value type ID from storage type info
    let type_id_str = match storage_type {
        StorageTypeInfo::Plain { plain } => plain,
        StorageTypeInfo::Map { map } => &map.value,
    };

    let type_id: u32 = type_id_str
        .parse()
        .map_err(|_| PalletError::PalletNotFound(format!("Invalid type ID: {}", type_id_str)))?;

    // Use the type-aware JsonVisitor to decode the value against the type registry
    let registry = metadata.types();
    let mut data = &bytes[..];
    let visitor = JsonVisitor::new(ss58_prefix, registry);

    match decode_with_visitor(&mut data, type_id, registry, visitor) {
        Ok(json_value) => Ok(json_value),
        Err(e) => {
            tracing::warn!(
                "Failed to decode storage value with type registry (type_id={}): {}. Falling back to raw hex.",
                type_id,
                e
            );
            // Fallback to raw hex if visitor-based decoding fails
            Ok(serde_json::Value::String(format!("0x{}", hex_clean)))
        }
    }
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
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("onlyIds" = Option<bool>, Query, description = "Only return storage item names")
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
    let relay_client = state.get_relay_chain_client().await?;
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_rpc = state.get_relay_chain_rpc().await?;

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;
    let resolved =
        crate::utils::resolve_block_with_rpc(&relay_rpc_client, &relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

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
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("keys[]" = Option<Vec<String>>, Query, description = "Storage key arguments"),
        ("metadata" = Option<bool>, Query, description = "Include metadata for the storage item")
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
    let relay_client = state.get_relay_chain_client().await?;
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
    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let response = build_storage_item_response(
        &relay_rpc_client,
        &metadata,
        &pallet_id,
        &storage_item_id,
        &params.keys,
        &resolved,
        params.metadata,
        &block_hash,
        state.chain_info.ss58_prefix,
    )
    .await?;

    Ok(Json(response).into_response())
}

// ============================================================================
// Subxt-based builders (all metadata versions V9-V16 via subxt's cached metadata)
// ============================================================================

/// Build storage response using subxt's Metadata (all versions V9-V16 via cached metadata).
fn build_storage_response(
    metadata: &Metadata,
    pallet_id: &str,
    resolved_block: &utils::ResolvedBlock,
    only_ids: bool,
) -> Result<PalletsStorageResponse, PalletError> {
    let pallet = find_pallet_subxt(metadata, pallet_id)?;

    let items: Vec<StorageItemMetadata> = if let Some(storage) = pallet.storage() {
        storage
            .entries()
            .iter()
            .map(|entry| {
                let has_default = entry.default_value().is_some();
                let modifier = if has_default { "Default" } else { "Optional" };

                let keys: Vec<_> = entry.keys().collect();
                let ty = if keys.is_empty() {
                    StorageTypeInfo::Plain {
                        plain: entry.value_ty().to_string(),
                    }
                } else {
                    StorageTypeInfo::Map {
                        map: MapTypeInfo {
                            hashers: keys
                                .iter()
                                .map(|k| hasher_to_string_fd(&k.hasher))
                                .collect(),
                            key: if keys.len() == 1 {
                                keys[0].key_id.to_string()
                            } else {
                                // Multiple keys: format as comma-separated type IDs
                                keys.iter()
                                    .map(|k| k.key_id.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            },
                            value: entry.value_ty().to_string(),
                        },
                    }
                };

                let fallback = match entry.default_value() {
                    Some(bytes) => format!("0x{}", hex::encode(bytes)),
                    None => "0x".to_string(),
                };

                StorageItemMetadata {
                    name: entry.name().to_string(),
                    modifier: modifier.to_string(),
                    ty,
                    fallback,
                    docs: entry.docs().join("\n"),
                    deprecation_info: DeprecationInfo::NotDeprecated(None),
                }
            })
            .collect()
    } else {
        vec![]
    };

    let full_response = PalletsStorageResponse {
        at: AtResponse {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        pallet: pallet.name().to_lowercase(),
        pallet_index: pallet.call_index().to_string(),
        items: StorageItems::Full(items),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    if only_ids {
        let names: Vec<String> = match &full_response.items {
            StorageItems::Full(items) => items.iter().map(|item| item.name.clone()).collect(),
            StorageItems::OnlyIds(names) => names.clone(),
        };
        Ok(PalletsStorageResponse {
            items: StorageItems::OnlyIds(names),
            ..full_response
        })
    } else {
        Ok(full_response)
    }
}

/// Build storage item response using subxt's Metadata (all versions V9-V16 via cached metadata).
#[allow(clippy::too_many_arguments)]
async fn build_storage_item_response(
    rpc_client: &RpcClient,
    metadata: &Metadata,
    pallet_id: &str,
    storage_item_id: &str,
    keys: &[String],
    resolved_block: &utils::ResolvedBlock,
    include_metadata: bool,
    block_hash: &str,
    ss58_prefix: u16,
) -> Result<PalletsStorageItemResponse, PalletError> {
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

    let storage_item = storage_items
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(storage_item_id))
        .ok_or_else(|| PalletError::StorageItemNotFound {
            pallet: pallet_id.to_string(),
            item: storage_item_id.to_string(),
        })?;

    let original_pallet_name = get_original_pallet_name_subxt(metadata, pallet_id)?;

    let storage_key = build_storage_key(
        &original_pallet_name,
        &storage_item.name,
        keys,
        &storage_item.ty,
    )?;

    let value_hex: Option<String> = rpc_client
        .request("state_getStorage", rpc_params![&storage_key, block_hash])
        .await
        .ok();

    let value = decode_storage_value(value_hex, &storage_item.ty, metadata, ss58_prefix)?;

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

/// Find a pallet by name or index using subxt's Metadata.
fn find_pallet_subxt<'a>(
    metadata: &'a Metadata,
    pallet_id: &str,
) -> Result<subxt_metadata::PalletMetadata<'a>, PalletError> {
    if let Ok(idx) = pallet_id.parse::<u8>() {
        metadata
            .pallets()
            .find(|p| p.call_index() == idx)
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
    } else {
        metadata
            .pallet_by_name(pallet_id)
            .or_else(|| {
                // Case-insensitive fallback
                metadata
                    .pallets()
                    .find(|p| p.name().eq_ignore_ascii_case(pallet_id))
            })
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
    }
}

/// Get the original pallet name (PascalCase) from subxt Metadata.
fn get_original_pallet_name_subxt(
    metadata: &Metadata,
    pallet_id: &str,
) -> Result<String, PalletError> {
    find_pallet_subxt(metadata, pallet_id).map(|p| p.name().to_string())
}

/// Convert a frame_decode StorageHasher to string.
fn hasher_to_string_fd(hasher: &frame_decode::storage::StorageHasher) -> String {
    use frame_decode::storage::StorageHasher;
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
