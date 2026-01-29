//! Handler for the `/pallets/{palletId}/dispatchables` endpoint.
//!
//! This endpoint returns the dispatchables (extrinsics/calls) defined in a pallet's metadata.
//! It supports querying at specific blocks and relay chain block resolution
//! for Asset Hub chains.
//!
//! # Sidecar Compatibility
//!
//! This endpoint aims to match the Sidecar `/pallets/{palletId}/dispatchables` response format.

// Allow large error types - PalletError contains subxt::error::OnlineClientAtBlockError
// which is large by design. Boxing would add indirection without significant benefit.
#![allow(clippy::result_large_err)]

use crate::handlers::pallets::common::{
    AtResponse, PalletError, PalletItemQueryParams, PalletQueryParams, RcBlockFields,
    find_pallet_v14, find_pallet_v15, find_pallet_v16,
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
use config::ChainType;
use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed, decode_different::DecodeDifferent};
use parity_scale_codec::Decode;
use serde::Serialize;
use subxt_rpcs::rpc_params;

// ============================================================================
// Response Types
// ============================================================================

/// Response for the `/pallets/{palletId}/dispatchables` endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsDispatchablesResponse {
    /// Block reference information.
    pub at: AtResponse,

    /// The pallet name (lowercase).
    pub pallet: String,

    /// The pallet index in the metadata.
    pub pallet_index: String,

    /// The list of dispatchables (full metadata or just names).
    pub items: DispatchablesItems,

    /// Relay chain block hash (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,

    /// Relay chain block number (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,

    /// Asset Hub timestamp (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Dispatchables items - either full metadata or just names.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DispatchablesItems {
    /// Full dispatchable metadata.
    Full(Vec<DispatchableItemMetadata>),

    /// Only dispatchable names (when `onlyIds=true`).
    OnlyIds(Vec<String>),
}

/// Metadata for a single dispatchable.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchableItemMetadata {
    /// The dispatchable name.
    pub name: String,

    /// The fields/arguments of the dispatchable (with type IDs).
    pub fields: Vec<DispatchableField>,

    /// The index of the dispatchable in the pallet's call enum.
    pub index: String,

    /// Documentation for the dispatchable.
    pub docs: Vec<String>,

    /// The arguments with resolved type names (for Sidecar compatibility).
    pub args: Vec<DispatchableArg>,
}

/// A field/argument of a dispatchable (with type ID).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchableField {
    /// The field name.
    pub name: String,

    /// The type ID or type name.
    #[serde(rename = "type")]
    pub ty: String,

    /// The type name (human-readable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,

    /// Documentation for the field.
    pub docs: Vec<String>,
}

/// An argument of a dispatchable with resolved type name (Sidecar compatible).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchableArg {
    /// The argument name (camelCase).
    pub name: String,

    /// The resolved type name from the type registry.
    #[serde(rename = "type")]
    pub ty: String,

    /// The simplified type name (without generics).
    pub type_name: String,
}

/// Response for the `/pallets/{palletId}/dispatchables/{dispatchableItemId}` endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletDispatchableItemResponse {
    /// Block reference information.
    pub at: AtResponse,

    /// The pallet name (lowercase).
    pub pallet: String,

    /// The pallet index in the metadata.
    pub pallet_index: String,

    /// The dispatchable name (camelCase).
    pub dispatchable_item: String,

    /// Full metadata for the dispatchable (only when `metadata=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<DispatchableItemMetadata>,

    /// Relay chain block hash (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,

    /// Relay chain block number (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,

    /// Asset Hub timestamp (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// ============================================================================
// Handler
// ============================================================================

/// Handler for `GET /pallets/{palletId}/dispatchables`.
///
/// Returns the dispatchables defined in the specified pallet.
///
/// # Query Parameters
///
/// - `at`: Block hash or number to query at. Defaults to the latest block.
/// - `onlyIds`: If `true`, only return dispatchable names without full metadata.
/// - `useRcBlock`: If `true`, resolve the block from the relay chain (Asset Hub only).
///
/// # Errors
///
/// - `400 Bad Request`: Invalid block parameter or unsupported `useRcBlock` usage.
/// - `404 Not Found`: Pallet not found in metadata.
/// - `500 Internal Server Error`: Unsupported metadata version.
/// - `503 Service Unavailable`: RPC connection lost.
pub async fn get_pallets_dispatchables(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<PalletQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, pallet_id, params).await;
    }

    // Create client at the specified block
    let client_at_block = match params.at {
        None => state.client.at_current_block().await?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<utils::BlockId>()?;
            state.client.at_block(block_id).await?
        }
    };

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    // Fetch raw metadata via RPC to access all metadata versions (V9-V16)
    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let metadata = fetch_runtime_metadata(&state, &block_hash).await?;

    let response = extract_dispatchables_from_metadata(
        &metadata,
        &pallet_id,
        at,
        params.only_ids,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Handler for `GET /pallets/{palletId}/dispatchables/{dispatchableItemId}`.
///
/// Returns a single dispatchable defined in the specified pallet.
///
/// # Query Parameters
///
/// - `at`: Block hash or number to query at. Defaults to the latest block.
/// - `metadata`: If `true`, include full metadata for the dispatchable.
/// - `useRcBlock`: If `true`, resolve the block from the relay chain (Asset Hub only).
///
/// # Errors
///
/// - `400 Bad Request`: Invalid block parameter or unsupported `useRcBlock` usage.
/// - `404 Not Found`: Pallet or dispatchable not found in metadata.
/// - `500 Internal Server Error`: Unsupported metadata version.
/// - `503 Service Unavailable`: RPC connection lost.
pub async fn get_pallet_dispatchable_item(
    State(state): State<AppState>,
    Path((pallet_id, dispatchable_id)): Path<(String, String)>,
    Query(params): Query<PalletItemQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_dispatchable_item_use_rc_block(state, pallet_id, dispatchable_id, params)
            .await;
    }

    // Create client at the specified block
    let client_at_block = match params.at {
        None => state.client.at_current_block().await?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<utils::BlockId>()?;
            state.client.at_block(block_id).await?
        }
    };

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    // Fetch raw metadata via RPC to access all metadata versions (V9-V16)
    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let metadata = fetch_runtime_metadata(&state, &block_hash).await?;

    let response = extract_dispatchable_item_from_metadata(
        &metadata,
        &pallet_id,
        &dispatchable_id,
        at,
        params.metadata,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
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

// ============================================================================
// Relay Chain Block Handler
// ============================================================================

/// Handle requests with `useRcBlock=true` for Asset Hub chains.
async fn handle_use_rc_block(
    state: AppState,
    pallet_id: String,
    params: PalletQueryParams,
) -> Result<Response, PalletError> {
    // Validate this is an Asset Hub chain
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    // Validate relay chain connection is configured
    if state.get_relay_chain_client().is_none() {
        return Err(PalletError::RelayChainNotConfigured);
    }

    // Parse the relay chain block ID
    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    // Resolve the relay chain block
    let rc_resolved_block = utils::resolve_block_with_rpc(
        state
            .get_relay_chain_rpc_client()
            .expect("relay chain client checked above"),
        state
            .get_relay_chain_rpc()
            .expect("relay chain RPC checked above"),
        Some(rc_block_id),
    )
    .await?;

    // Find Asset Hub blocks in the relay chain block
    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    // If no Asset Hub blocks found, return empty response
    if ah_blocks.is_empty() {
        let at = AtResponse {
            hash: rc_resolved_block.hash.clone(),
            height: rc_resolved_block.number.to_string(),
        };
        return Ok((
            StatusCode::OK,
            Json(PalletsDispatchablesResponse {
                at,
                pallet: pallet_id.to_lowercase(),
                pallet_index: "0".to_string(),
                items: DispatchablesItems::Full(vec![]),
                rc_block_hash: Some(rc_resolved_block.hash),
                rc_block_number: Some(rc_resolved_block.number.to_string()),
                ah_timestamp: None,
            }),
        )
            .into_response());
    }

    // Use the first Asset Hub block
    let ah_block = &ah_blocks[0];

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    // Fetch raw metadata via RPC for full version support
    let metadata = fetch_runtime_metadata(&state, &ah_block.hash).await?;

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

    let mut ah_timestamp = None;
    if let Some(timestamp_hex) = timestamp_result {
        let hex_str = timestamp_hex.strip_prefix("0x").unwrap_or(&timestamp_hex);
        if let Ok(timestamp_bytes) = hex::decode(hex_str) {
            let mut cursor = &timestamp_bytes[..];
            if let Ok(timestamp_value) = u64::decode(&mut cursor) {
                ah_timestamp = Some(timestamp_value.to_string());
            }
        }
    }

    let rc_fields = RcBlockFields {
        rc_block_hash: Some(rc_resolved_block.hash),
        rc_block_number: Some(rc_resolved_block.number.to_string()),
        ah_timestamp,
    };

    let response =
        extract_dispatchables_from_metadata(&metadata, &pallet_id, at, params.only_ids, rc_fields)?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Handle requests with `useRcBlock=true` for Asset Hub chains (single dispatchable item).
async fn handle_dispatchable_item_use_rc_block(
    state: AppState,
    pallet_id: String,
    dispatchable_id: String,
    params: PalletItemQueryParams,
) -> Result<Response, PalletError> {
    // Validate this is an Asset Hub chain
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    // Validate relay chain connection is configured
    if state.get_relay_chain_client().is_none() {
        return Err(PalletError::RelayChainNotConfigured);
    }

    // Parse the relay chain block ID
    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    // Resolve the relay chain block
    let rc_resolved_block = utils::resolve_block_with_rpc(
        state
            .get_relay_chain_rpc_client()
            .expect("relay chain client checked above"),
        state
            .get_relay_chain_rpc()
            .expect("relay chain RPC checked above"),
        Some(rc_block_id),
    )
    .await?;

    // Find Asset Hub blocks in the relay chain block
    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    // If no Asset Hub blocks found, return error
    if ah_blocks.is_empty() {
        return Err(PalletError::DispatchableNotFound(dispatchable_id));
    }

    // Use the first Asset Hub block
    let ah_block = &ah_blocks[0];

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    // Fetch raw metadata via RPC for full version support
    let metadata = fetch_runtime_metadata(&state, &ah_block.hash).await?;

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

    let mut ah_timestamp = None;
    if let Some(timestamp_hex) = timestamp_result {
        let hex_str = timestamp_hex.strip_prefix("0x").unwrap_or(&timestamp_hex);
        if let Ok(timestamp_bytes) = hex::decode(hex_str) {
            let mut cursor = &timestamp_bytes[..];
            if let Ok(timestamp_value) = u64::decode(&mut cursor) {
                ah_timestamp = Some(timestamp_value.to_string());
            }
        }
    }

    let rc_fields = RcBlockFields {
        rc_block_hash: Some(rc_resolved_block.hash),
        rc_block_number: Some(rc_resolved_block.number.to_string()),
        ah_timestamp,
    };

    let response = extract_dispatchable_item_from_metadata(
        &metadata,
        &pallet_id,
        &dispatchable_id,
        at,
        params.metadata,
        rc_fields,
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Metadata Extraction
// ============================================================================

/// Extract dispatchables from runtime metadata.
fn extract_dispatchables_from_metadata(
    metadata: &RuntimeMetadata,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsDispatchablesResponse, PalletError> {
    use RuntimeMetadata::*;
    match metadata {
        V9(meta) => extract_dispatchables_v9(meta, pallet_id, at, only_ids, rc_fields),
        V10(meta) => extract_dispatchables_v10(meta, pallet_id, at, only_ids, rc_fields),
        V11(meta) => extract_dispatchables_v11(meta, pallet_id, at, only_ids, rc_fields),
        V12(meta) => extract_dispatchables_v12(meta, pallet_id, at, only_ids, rc_fields),
        V13(meta) => extract_dispatchables_v13(meta, pallet_id, at, only_ids, rc_fields),
        V14(meta) => extract_dispatchables_v14(meta, pallet_id, at, only_ids, rc_fields),
        V15(meta) => extract_dispatchables_v15(meta, pallet_id, at, only_ids, rc_fields),
        V16(meta) => extract_dispatchables_v16(meta, pallet_id, at, only_ids, rc_fields),
        _ => Err(PalletError::UnsupportedMetadataVersion),
    }
}

/// Extract a single dispatchable from runtime metadata.
fn extract_dispatchable_item_from_metadata(
    metadata: &RuntimeMetadata,
    pallet_id: &str,
    dispatchable_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletDispatchableItemResponse, PalletError> {
    use RuntimeMetadata::*;
    match metadata {
        V9(meta) => extract_dispatchable_item_v9(
            meta,
            pallet_id,
            dispatchable_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V10(meta) => extract_dispatchable_item_v10(
            meta,
            pallet_id,
            dispatchable_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V11(meta) => extract_dispatchable_item_v11(
            meta,
            pallet_id,
            dispatchable_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V12(meta) => extract_dispatchable_item_v12(
            meta,
            pallet_id,
            dispatchable_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V13(meta) => extract_dispatchable_item_v13(
            meta,
            pallet_id,
            dispatchable_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V14(meta) => extract_dispatchable_item_v14(
            meta,
            pallet_id,
            dispatchable_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V15(meta) => extract_dispatchable_item_v15(
            meta,
            pallet_id,
            dispatchable_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V16(meta) => extract_dispatchable_item_v16(
            meta,
            pallet_id,
            dispatchable_id,
            at,
            include_metadata,
            rc_fields,
        ),
        _ => Err(PalletError::UnsupportedMetadataVersion),
    }
}

// ============================================================================
// Helper Functions for V9-V13 Metadata
// ============================================================================

/// Helper to extract a string from a DecodeDifferent type used in V9-V13 metadata.
fn extract_str<'a>(dd: &'a DecodeDifferent<&'static str, String>) -> &'a str {
    match dd {
        DecodeDifferent::Decoded(s) => s.as_str(),
        DecodeDifferent::Encode(s) => s,
    }
}

/// Helper to extract docs from DecodeDifferent used in V9-V13 metadata.
fn extract_docs(docs: &DecodeDifferent<&'static [&'static str], Vec<String>>) -> Vec<String> {
    match docs {
        DecodeDifferent::Decoded(v) => v.clone(),
        DecodeDifferent::Encode(s) => s.iter().map(|x| x.to_string()).collect(),
    }
}

/// Convert snake_case to camelCase
fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Simplify type name by removing `T::` prefix from generic parameters
/// e.g., "Vec<T::AccountId>" -> "Vec<AccountId>"
/// e.g., "AccountIdLookupOf<T>" -> "AccountIdLookupOf"
fn simplify_type_name(type_name: &str) -> String {
    // Replace T:: prefixes inside generic parameters
    let simplified = type_name.replace("T::", "");

    // Remove trailing <T> (for types like AccountIdLookupOf<T>)

    simplified
        .trim_end_matches("<T>")
        .trim_end_matches("Of<T>")
        .to_string()
}

/// Resolve a type ID to its display name from the V14 type registry
fn resolve_type_name_v14(registry: &scale_info::PortableRegistry, type_id: u32) -> String {
    let Some(ty) = registry.resolve(type_id) else {
        return format!("Type{}", type_id);
    };

    format_type_def_v14(registry, ty)
}

/// Convert path segments to PascalCase joined format (e.g., ["pallet_balances", "AdjustmentDirection"] -> "PalletBalancesAdjustmentDirection")
/// Skips intermediate module segments like "types", "pallet" to match Sidecar format
fn path_to_pascal_case(segments: &[String]) -> String {
    // Segments to skip (intermediate module names)
    let skip_segments = ["types", "pallet"];

    segments
        .iter()
        .filter(|seg| !skip_segments.contains(&seg.as_str()))
        .map(|seg| {
            // Convert snake_case to PascalCase
            seg.split('_')
                .filter(|s| !s.is_empty())
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().chain(chars).collect(),
                    }
                })
                .collect::<String>()
        })
        .collect()
}

/// Format a type definition to a human-readable string
fn format_type_def_v14(
    registry: &scale_info::PortableRegistry,
    ty: &scale_info::Type<scale_info::form::PortableForm>,
) -> String {
    use scale_info::TypeDef;

    match &ty.type_def {
        TypeDef::Composite(_) => {
            // Use the type path for composites (e.g., "MultiAddress")
            ty.path
                .segments
                .last()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Composite".to_string())
        }
        TypeDef::Variant(_) => {
            // Use full path for pallet types (e.g., pallet_balances::AdjustmentDirection -> PalletBalancesAdjustmentDirection)
            // Use short name for non-pallet types (e.g., sp_runtime::multiaddress::MultiAddress -> MultiAddress)
            if ty.path.segments.is_empty() {
                "Enum".to_string()
            } else if ty
                .path
                .segments
                .first()
                .map(|s| s.starts_with("pallet_"))
                .unwrap_or(false)
            {
                // Pallet type: use full path joined as PascalCase
                path_to_pascal_case(&ty.path.segments)
            } else {
                // Non-pallet type: use just the last segment (type name)
                ty.path
                    .segments
                    .last()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Enum".to_string())
            }
        }
        TypeDef::Sequence(seq) => {
            let inner = resolve_type_name_v14(registry, seq.type_param.id);
            format!("Vec<{}>", inner)
        }
        TypeDef::Array(arr) => {
            let inner = resolve_type_name_v14(registry, arr.type_param.id);
            format!("[{}; {}]", inner, arr.len)
        }
        TypeDef::Tuple(tuple) => {
            if tuple.fields.is_empty() {
                "()".to_string()
            } else {
                let fields: Vec<String> = tuple
                    .fields
                    .iter()
                    .map(|f| resolve_type_name_v14(registry, f.id))
                    .collect();
                format!("({})", fields.join(", "))
            }
        }
        TypeDef::Primitive(prim) => {
            use scale_info::TypeDefPrimitive;
            match prim {
                TypeDefPrimitive::Bool => "bool".to_string(),
                TypeDefPrimitive::Char => "char".to_string(),
                TypeDefPrimitive::Str => "str".to_string(),
                TypeDefPrimitive::U8 => "u8".to_string(),
                TypeDefPrimitive::U16 => "u16".to_string(),
                TypeDefPrimitive::U32 => "u32".to_string(),
                TypeDefPrimitive::U64 => "u64".to_string(),
                TypeDefPrimitive::U128 => "u128".to_string(),
                TypeDefPrimitive::U256 => "u256".to_string(),
                TypeDefPrimitive::I8 => "i8".to_string(),
                TypeDefPrimitive::I16 => "i16".to_string(),
                TypeDefPrimitive::I32 => "i32".to_string(),
                TypeDefPrimitive::I64 => "i64".to_string(),
                TypeDefPrimitive::I128 => "i128".to_string(),
                TypeDefPrimitive::I256 => "i256".to_string(),
            }
        }
        TypeDef::Compact(compact) => {
            let inner = resolve_type_name_v14(registry, compact.type_param.id);
            format!("Compact<{}>", inner)
        }
        TypeDef::BitSequence(_) => "BitSequence".to_string(),
    }
}

// ============================================================================
// V9 Metadata Extraction
// ============================================================================

fn extract_dispatchables_v9(
    meta: &frame_metadata::v9::RuntimeMetadataV9,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsDispatchablesResponse, PalletError> {
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

    // V9 calls are in module.calls as Option<DecodeDifferent<FnEncode<&'static [FunctionMetadata]>, Vec<FunctionMetadata>>>
    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        DispatchablesItems::OnlyIds(
            calls
                .iter()
                .map(|c| extract_str(&c.name).to_string())
                .collect(),
        )
    } else {
        DispatchablesItems::Full(
            calls
                .iter()
                .enumerate()
                .map(|(idx, c)| {
                    let DecodeDifferent::Decoded(call_args) = &c.arguments else {
                        return DispatchableItemMetadata {
                            name: extract_str(&c.name).to_string(),
                            fields: vec![],
                            index: idx.to_string(),
                            docs: extract_docs(&c.documentation),
                            args: vec![],
                        };
                    };
                    let fields: Vec<DispatchableField> = call_args
                        .iter()
                        .map(|arg| DispatchableField {
                            name: extract_str(&arg.name).to_string(),
                            ty: extract_str(&arg.ty).to_string(),
                            type_name: None,
                            docs: vec![],
                        })
                        .collect();
                    let args: Vec<DispatchableArg> = call_args
                        .iter()
                        .map(|arg| {
                            let name = extract_str(&arg.name).to_string();
                            let ty = extract_str(&arg.ty).to_string();
                            DispatchableArg {
                                name: to_camel_case(&name),
                                ty: ty.clone(),
                                type_name: ty,
                            }
                        })
                        .collect();
                    DispatchableItemMetadata {
                        name: extract_str(&c.name).to_string(),
                        fields,
                        index: idx.to_string(),
                        docs: extract_docs(&c.documentation),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsDispatchablesResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_dispatchable_item_v9(
    meta: &frame_metadata::v9::RuntimeMetadataV9,
    pallet_id: &str,
    dispatchable_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletDispatchableItemResponse, PalletError> {
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

    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let dispatchable_id_lower = dispatchable_id.to_lowercase();
    let (idx, call) = calls
        .iter()
        .enumerate()
        .find(|(_, c)| extract_str(&c.name).to_lowercase() == dispatchable_id_lower)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let call_name = extract_str(&call.name).to_string();

    let metadata = if include_metadata {
        let DecodeDifferent::Decoded(call_args) = &call.arguments else {
            return Ok(PalletDispatchableItemResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                dispatchable_item: to_camel_case(&call_name),
                metadata: Some(DispatchableItemMetadata {
                    name: extract_str(&call.name).to_string(),
                    fields: vec![],
                    index: idx.to_string(),
                    docs: extract_docs(&call.documentation),
                    args: vec![],
                }),
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        };
        let fields: Vec<DispatchableField> = call_args
            .iter()
            .map(|arg| DispatchableField {
                name: extract_str(&arg.name).to_string(),
                ty: extract_str(&arg.ty).to_string(),
                type_name: None,
                docs: vec![],
            })
            .collect();
        let args: Vec<DispatchableArg> = call_args
            .iter()
            .map(|arg| {
                let name = extract_str(&arg.name).to_string();
                let ty = extract_str(&arg.ty).to_string();
                DispatchableArg {
                    name: to_camel_case(&name),
                    ty: ty.clone(),
                    type_name: ty,
                }
            })
            .collect();
        Some(DispatchableItemMetadata {
            name: extract_str(&call.name).to_string(),
            fields,
            index: idx.to_string(),
            docs: extract_docs(&call.documentation),
            args,
        })
    } else {
        None
    };

    Ok(PalletDispatchableItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        dispatchable_item: to_camel_case(&call_name),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V10 Metadata Extraction
// ============================================================================

fn extract_dispatchables_v10(
    meta: &frame_metadata::v10::RuntimeMetadataV10,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsDispatchablesResponse, PalletError> {
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

    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        DispatchablesItems::OnlyIds(
            calls
                .iter()
                .map(|c| extract_str(&c.name).to_string())
                .collect(),
        )
    } else {
        DispatchablesItems::Full(
            calls
                .iter()
                .enumerate()
                .map(|(idx, c)| {
                    let DecodeDifferent::Decoded(call_args) = &c.arguments else {
                        return DispatchableItemMetadata {
                            name: extract_str(&c.name).to_string(),
                            fields: vec![],
                            index: idx.to_string(),
                            docs: extract_docs(&c.documentation),
                            args: vec![],
                        };
                    };
                    let fields: Vec<DispatchableField> = call_args
                        .iter()
                        .map(|arg| DispatchableField {
                            name: extract_str(&arg.name).to_string(),
                            ty: extract_str(&arg.ty).to_string(),
                            type_name: None,
                            docs: vec![],
                        })
                        .collect();
                    let args: Vec<DispatchableArg> = call_args
                        .iter()
                        .map(|arg| {
                            let name = extract_str(&arg.name).to_string();
                            let ty = extract_str(&arg.ty).to_string();
                            DispatchableArg {
                                name: to_camel_case(&name),
                                ty: ty.clone(),
                                type_name: ty,
                            }
                        })
                        .collect();
                    DispatchableItemMetadata {
                        name: extract_str(&c.name).to_string(),
                        fields,
                        index: idx.to_string(),
                        docs: extract_docs(&c.documentation),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsDispatchablesResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_dispatchable_item_v10(
    meta: &frame_metadata::v10::RuntimeMetadataV10,
    pallet_id: &str,
    dispatchable_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletDispatchableItemResponse, PalletError> {
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

    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let dispatchable_id_lower = dispatchable_id.to_lowercase();
    let (idx, call) = calls
        .iter()
        .enumerate()
        .find(|(_, c)| extract_str(&c.name).to_lowercase() == dispatchable_id_lower)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let call_name = extract_str(&call.name).to_string();

    let metadata = if include_metadata {
        let DecodeDifferent::Decoded(call_args) = &call.arguments else {
            return Ok(PalletDispatchableItemResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                dispatchable_item: to_camel_case(&call_name),
                metadata: Some(DispatchableItemMetadata {
                    name: extract_str(&call.name).to_string(),
                    fields: vec![],
                    index: idx.to_string(),
                    docs: extract_docs(&call.documentation),
                    args: vec![],
                }),
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        };
        let fields: Vec<DispatchableField> = call_args
            .iter()
            .map(|arg| DispatchableField {
                name: extract_str(&arg.name).to_string(),
                ty: extract_str(&arg.ty).to_string(),
                type_name: None,
                docs: vec![],
            })
            .collect();
        let args: Vec<DispatchableArg> = call_args
            .iter()
            .map(|arg| {
                let name = extract_str(&arg.name).to_string();
                let ty = extract_str(&arg.ty).to_string();
                DispatchableArg {
                    name: to_camel_case(&name),
                    ty: ty.clone(),
                    type_name: ty,
                }
            })
            .collect();
        Some(DispatchableItemMetadata {
            name: extract_str(&call.name).to_string(),
            fields,
            index: idx.to_string(),
            docs: extract_docs(&call.documentation),
            args,
        })
    } else {
        None
    };

    Ok(PalletDispatchableItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        dispatchable_item: to_camel_case(&call_name),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V11 Metadata Extraction
// ============================================================================

fn extract_dispatchables_v11(
    meta: &frame_metadata::v11::RuntimeMetadataV11,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsDispatchablesResponse, PalletError> {
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

    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        DispatchablesItems::OnlyIds(
            calls
                .iter()
                .map(|c| extract_str(&c.name).to_string())
                .collect(),
        )
    } else {
        DispatchablesItems::Full(
            calls
                .iter()
                .enumerate()
                .map(|(idx, c)| {
                    let DecodeDifferent::Decoded(call_args) = &c.arguments else {
                        return DispatchableItemMetadata {
                            name: extract_str(&c.name).to_string(),
                            fields: vec![],
                            index: idx.to_string(),
                            docs: extract_docs(&c.documentation),
                            args: vec![],
                        };
                    };
                    let fields: Vec<DispatchableField> = call_args
                        .iter()
                        .map(|arg| DispatchableField {
                            name: extract_str(&arg.name).to_string(),
                            ty: extract_str(&arg.ty).to_string(),
                            type_name: None,
                            docs: vec![],
                        })
                        .collect();
                    let args: Vec<DispatchableArg> = call_args
                        .iter()
                        .map(|arg| {
                            let name = extract_str(&arg.name).to_string();
                            let ty = extract_str(&arg.ty).to_string();
                            DispatchableArg {
                                name: to_camel_case(&name),
                                ty: ty.clone(),
                                type_name: ty,
                            }
                        })
                        .collect();
                    DispatchableItemMetadata {
                        name: extract_str(&c.name).to_string(),
                        fields,
                        index: idx.to_string(),
                        docs: extract_docs(&c.documentation),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsDispatchablesResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_dispatchable_item_v11(
    meta: &frame_metadata::v11::RuntimeMetadataV11,
    pallet_id: &str,
    dispatchable_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletDispatchableItemResponse, PalletError> {
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

    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let dispatchable_id_lower = dispatchable_id.to_lowercase();
    let (idx, call) = calls
        .iter()
        .enumerate()
        .find(|(_, c)| extract_str(&c.name).to_lowercase() == dispatchable_id_lower)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let call_name = extract_str(&call.name).to_string();

    let metadata = if include_metadata {
        let DecodeDifferent::Decoded(call_args) = &call.arguments else {
            return Ok(PalletDispatchableItemResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                dispatchable_item: to_camel_case(&call_name),
                metadata: Some(DispatchableItemMetadata {
                    name: extract_str(&call.name).to_string(),
                    fields: vec![],
                    index: idx.to_string(),
                    docs: extract_docs(&call.documentation),
                    args: vec![],
                }),
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        };
        let fields: Vec<DispatchableField> = call_args
            .iter()
            .map(|arg| DispatchableField {
                name: extract_str(&arg.name).to_string(),
                ty: extract_str(&arg.ty).to_string(),
                type_name: None,
                docs: vec![],
            })
            .collect();
        let args: Vec<DispatchableArg> = call_args
            .iter()
            .map(|arg| {
                let name = extract_str(&arg.name).to_string();
                let ty = extract_str(&arg.ty).to_string();
                DispatchableArg {
                    name: to_camel_case(&name),
                    ty: ty.clone(),
                    type_name: ty,
                }
            })
            .collect();
        Some(DispatchableItemMetadata {
            name: extract_str(&call.name).to_string(),
            fields,
            index: idx.to_string(),
            docs: extract_docs(&call.documentation),
            args,
        })
    } else {
        None
    };

    Ok(PalletDispatchableItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        dispatchable_item: to_camel_case(&call_name),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V12 Metadata Extraction
// ============================================================================

fn extract_dispatchables_v12(
    meta: &frame_metadata::v12::RuntimeMetadataV12,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsDispatchablesResponse, PalletError> {
    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    // V12 has index field
    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules
            .iter()
            .find(|m| m.index == idx)
            .map(|m| (m, idx))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        DispatchablesItems::OnlyIds(
            calls
                .iter()
                .map(|c| extract_str(&c.name).to_string())
                .collect(),
        )
    } else {
        DispatchablesItems::Full(
            calls
                .iter()
                .enumerate()
                .map(|(idx, c)| {
                    let DecodeDifferent::Decoded(call_args) = &c.arguments else {
                        return DispatchableItemMetadata {
                            name: extract_str(&c.name).to_string(),
                            fields: vec![],
                            index: idx.to_string(),
                            docs: extract_docs(&c.documentation),
                            args: vec![],
                        };
                    };
                    let fields: Vec<DispatchableField> = call_args
                        .iter()
                        .map(|arg| DispatchableField {
                            name: extract_str(&arg.name).to_string(),
                            ty: extract_str(&arg.ty).to_string(),
                            type_name: None,
                            docs: vec![],
                        })
                        .collect();
                    let args: Vec<DispatchableArg> = call_args
                        .iter()
                        .map(|arg| {
                            let name = extract_str(&arg.name).to_string();
                            let ty = extract_str(&arg.ty).to_string();
                            DispatchableArg {
                                name: to_camel_case(&name),
                                ty: ty.clone(),
                                type_name: ty,
                            }
                        })
                        .collect();
                    DispatchableItemMetadata {
                        name: extract_str(&c.name).to_string(),
                        fields,
                        index: idx.to_string(),
                        docs: extract_docs(&c.documentation),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsDispatchablesResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_dispatchable_item_v12(
    meta: &frame_metadata::v12::RuntimeMetadataV12,
    pallet_id: &str,
    dispatchable_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletDispatchableItemResponse, PalletError> {
    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules
            .iter()
            .find(|m| m.index == idx)
            .map(|m| (m, idx))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let dispatchable_id_lower = dispatchable_id.to_lowercase();
    let (idx, call) = calls
        .iter()
        .enumerate()
        .find(|(_, c)| extract_str(&c.name).to_lowercase() == dispatchable_id_lower)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let call_name = extract_str(&call.name).to_string();

    let metadata = if include_metadata {
        let DecodeDifferent::Decoded(call_args) = &call.arguments else {
            return Ok(PalletDispatchableItemResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                dispatchable_item: to_camel_case(&call_name),
                metadata: Some(DispatchableItemMetadata {
                    name: extract_str(&call.name).to_string(),
                    fields: vec![],
                    index: idx.to_string(),
                    docs: extract_docs(&call.documentation),
                    args: vec![],
                }),
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        };
        let fields: Vec<DispatchableField> = call_args
            .iter()
            .map(|arg| DispatchableField {
                name: extract_str(&arg.name).to_string(),
                ty: extract_str(&arg.ty).to_string(),
                type_name: None,
                docs: vec![],
            })
            .collect();
        let args: Vec<DispatchableArg> = call_args
            .iter()
            .map(|arg| {
                let name = extract_str(&arg.name).to_string();
                let ty = extract_str(&arg.ty).to_string();
                DispatchableArg {
                    name: to_camel_case(&name),
                    ty: ty.clone(),
                    type_name: ty,
                }
            })
            .collect();
        Some(DispatchableItemMetadata {
            name: extract_str(&call.name).to_string(),
            fields,
            index: idx.to_string(),
            docs: extract_docs(&call.documentation),
            args,
        })
    } else {
        None
    };

    Ok(PalletDispatchableItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        dispatchable_item: to_camel_case(&call_name),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V13 Metadata Extraction
// ============================================================================

fn extract_dispatchables_v13(
    meta: &frame_metadata::v13::RuntimeMetadataV13,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsDispatchablesResponse, PalletError> {
    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules
            .iter()
            .find(|m| m.index == idx)
            .map(|m| (m, idx))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        DispatchablesItems::OnlyIds(
            calls
                .iter()
                .map(|c| extract_str(&c.name).to_string())
                .collect(),
        )
    } else {
        DispatchablesItems::Full(
            calls
                .iter()
                .enumerate()
                .map(|(idx, c)| {
                    let DecodeDifferent::Decoded(call_args) = &c.arguments else {
                        return DispatchableItemMetadata {
                            name: extract_str(&c.name).to_string(),
                            fields: vec![],
                            index: idx.to_string(),
                            docs: extract_docs(&c.documentation),
                            args: vec![],
                        };
                    };
                    let fields: Vec<DispatchableField> = call_args
                        .iter()
                        .map(|arg| DispatchableField {
                            name: extract_str(&arg.name).to_string(),
                            ty: extract_str(&arg.ty).to_string(),
                            type_name: None,
                            docs: vec![],
                        })
                        .collect();
                    let args: Vec<DispatchableArg> = call_args
                        .iter()
                        .map(|arg| {
                            let name = extract_str(&arg.name).to_string();
                            let ty = extract_str(&arg.ty).to_string();
                            DispatchableArg {
                                name: to_camel_case(&name),
                                ty: ty.clone(),
                                type_name: ty,
                            }
                        })
                        .collect();
                    DispatchableItemMetadata {
                        name: extract_str(&c.name).to_string(),
                        fields,
                        index: idx.to_string(),
                        docs: extract_docs(&c.documentation),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsDispatchablesResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_dispatchable_item_v13(
    meta: &frame_metadata::v13::RuntimeMetadataV13,
    pallet_id: &str,
    dispatchable_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletDispatchableItemResponse, PalletError> {
    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules
            .iter()
            .find(|m| m.index == idx)
            .map(|m| (m, idx))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .find(|m| extract_str(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let calls = match &module.calls {
        Some(DecodeDifferent::Decoded(calls)) => calls,
        _ => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let dispatchable_id_lower = dispatchable_id.to_lowercase();
    let (idx, call) = calls
        .iter()
        .enumerate()
        .find(|(_, c)| extract_str(&c.name).to_lowercase() == dispatchable_id_lower)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let call_name = extract_str(&call.name).to_string();

    let metadata = if include_metadata {
        let DecodeDifferent::Decoded(call_args) = &call.arguments else {
            return Ok(PalletDispatchableItemResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                dispatchable_item: to_camel_case(&call_name),
                metadata: Some(DispatchableItemMetadata {
                    name: extract_str(&call.name).to_string(),
                    fields: vec![],
                    index: idx.to_string(),
                    docs: extract_docs(&call.documentation),
                    args: vec![],
                }),
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        };
        let fields: Vec<DispatchableField> = call_args
            .iter()
            .map(|arg| DispatchableField {
                name: extract_str(&arg.name).to_string(),
                ty: extract_str(&arg.ty).to_string(),
                type_name: None,
                docs: vec![],
            })
            .collect();
        let args: Vec<DispatchableArg> = call_args
            .iter()
            .map(|arg| {
                let name = extract_str(&arg.name).to_string();
                let ty = extract_str(&arg.ty).to_string();
                DispatchableArg {
                    name: to_camel_case(&name),
                    ty: ty.clone(),
                    type_name: ty,
                }
            })
            .collect();
        Some(DispatchableItemMetadata {
            name: extract_str(&call.name).to_string(),
            fields,
            index: idx.to_string(),
            docs: extract_docs(&call.documentation),
            args,
        })
    } else {
        None
    };

    Ok(PalletDispatchableItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        dispatchable_item: to_camel_case(&call_name),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V14 Metadata Extraction
// ============================================================================

fn extract_dispatchables_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsDispatchablesResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v14(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get the call type from the pallet
    let calls_type_id = match &pallet.calls {
        Some(calls) => calls.ty.id,
        None => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    // Resolve the type to get variants (dispatchables)
    let calls_type = meta.types.resolve(calls_type_id).ok_or_else(|| {
        PalletError::PalletNotFound(format!("Could not resolve calls type for {}", pallet_id))
    })?;

    let variants = match &calls_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        DispatchablesItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
    } else {
        DispatchablesItems::Full(
            variants
                .iter()
                .map(|v| {
                    let fields: Vec<DispatchableField> = v
                        .fields
                        .iter()
                        .map(|f| {
                            let type_name = f.type_name.clone();
                            DispatchableField {
                                name: f.name.clone().unwrap_or_default(),
                                ty: f.ty.id.to_string(),
                                type_name,
                                docs: f.docs.clone(),
                            }
                        })
                        .collect();

                    let args: Vec<DispatchableArg> = v
                        .fields
                        .iter()
                        .map(|f| {
                            let field_name = f.name.clone().unwrap_or_default();
                            let resolved_type = resolve_type_name_v14(&meta.types, f.ty.id);
                            let type_name = f
                                .type_name
                                .clone()
                                .map(|tn| simplify_type_name(&tn))
                                .unwrap_or_else(|| resolved_type.clone());
                            DispatchableArg {
                                name: to_camel_case(&field_name),
                                ty: resolved_type,
                                type_name,
                            }
                        })
                        .collect();

                    DispatchableItemMetadata {
                        name: v.name.clone(),
                        fields,
                        index: v.index.to_string(),
                        docs: v.docs.clone(),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsDispatchablesResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_dispatchable_item_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    dispatchable_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletDispatchableItemResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v14(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let calls_type_id = match &pallet.calls {
        Some(calls) => calls.ty.id,
        None => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let calls_type = meta
        .types
        .resolve(calls_type_id)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let variants = match &calls_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let dispatchable_id_lower = dispatchable_id.to_lowercase();
    let variant = variants
        .iter()
        .find(|v| v.name.to_lowercase() == dispatchable_id_lower)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let metadata = if include_metadata {
        let fields: Vec<DispatchableField> = variant
            .fields
            .iter()
            .map(|f| {
                let type_name = f.type_name.clone();
                DispatchableField {
                    name: f.name.clone().unwrap_or_default(),
                    ty: f.ty.id.to_string(),
                    type_name,
                    docs: f.docs.clone(),
                }
            })
            .collect();

        let args: Vec<DispatchableArg> = variant
            .fields
            .iter()
            .map(|f| {
                let field_name = f.name.clone().unwrap_or_default();
                let resolved_type = resolve_type_name_v14(&meta.types, f.ty.id);
                let type_name = f
                    .type_name
                    .clone()
                    .map(|tn| simplify_type_name(&tn))
                    .unwrap_or_else(|| resolved_type.clone());
                DispatchableArg {
                    name: to_camel_case(&field_name),
                    ty: resolved_type,
                    type_name,
                }
            })
            .collect();

        Some(DispatchableItemMetadata {
            name: variant.name.clone(),
            fields,
            index: variant.index.to_string(),
            docs: variant.docs.clone(),
            args,
        })
    } else {
        None
    };

    Ok(PalletDispatchableItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        dispatchable_item: to_camel_case(&variant.name),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V15 Metadata Extraction
// ============================================================================

fn extract_dispatchables_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsDispatchablesResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v15(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let calls_type_id = match &pallet.calls {
        Some(calls) => calls.ty.id,
        None => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let calls_type = meta.types.resolve(calls_type_id).ok_or_else(|| {
        PalletError::PalletNotFound(format!("Could not resolve calls type for {}", pallet_id))
    })?;

    let variants = match &calls_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        DispatchablesItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
    } else {
        DispatchablesItems::Full(
            variants
                .iter()
                .map(|v| {
                    let fields: Vec<DispatchableField> = v
                        .fields
                        .iter()
                        .map(|f| {
                            let type_name = f.type_name.clone();
                            DispatchableField {
                                name: f.name.clone().unwrap_or_default(),
                                ty: f.ty.id.to_string(),
                                type_name,
                                docs: f.docs.clone(),
                            }
                        })
                        .collect();

                    let args: Vec<DispatchableArg> = v
                        .fields
                        .iter()
                        .map(|f| {
                            let field_name = f.name.clone().unwrap_or_default();
                            let resolved_type = resolve_type_name_v14(&meta.types, f.ty.id);
                            let type_name = f
                                .type_name
                                .clone()
                                .map(|tn| simplify_type_name(&tn))
                                .unwrap_or_else(|| resolved_type.clone());
                            DispatchableArg {
                                name: to_camel_case(&field_name),
                                ty: resolved_type,
                                type_name,
                            }
                        })
                        .collect();

                    DispatchableItemMetadata {
                        name: v.name.clone(),
                        fields,
                        index: v.index.to_string(),
                        docs: v.docs.clone(),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsDispatchablesResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_dispatchable_item_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    dispatchable_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletDispatchableItemResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v15(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let calls_type_id = match &pallet.calls {
        Some(calls) => calls.ty.id,
        None => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let calls_type = meta
        .types
        .resolve(calls_type_id)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let variants = match &calls_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let dispatchable_id_lower = dispatchable_id.to_lowercase();
    let variant = variants
        .iter()
        .find(|v| v.name.to_lowercase() == dispatchable_id_lower)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let metadata = if include_metadata {
        let fields: Vec<DispatchableField> = variant
            .fields
            .iter()
            .map(|f| {
                let type_name = f.type_name.clone();
                DispatchableField {
                    name: f.name.clone().unwrap_or_default(),
                    ty: f.ty.id.to_string(),
                    type_name,
                    docs: f.docs.clone(),
                }
            })
            .collect();

        let args: Vec<DispatchableArg> = variant
            .fields
            .iter()
            .map(|f| {
                let field_name = f.name.clone().unwrap_or_default();
                let resolved_type = resolve_type_name_v14(&meta.types, f.ty.id);
                let type_name = f
                    .type_name
                    .clone()
                    .map(|tn| simplify_type_name(&tn))
                    .unwrap_or_else(|| resolved_type.clone());
                DispatchableArg {
                    name: to_camel_case(&field_name),
                    ty: resolved_type,
                    type_name,
                }
            })
            .collect();

        Some(DispatchableItemMetadata {
            name: variant.name.clone(),
            fields,
            index: variant.index.to_string(),
            docs: variant.docs.clone(),
            args,
        })
    } else {
        None
    };

    Ok(PalletDispatchableItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        dispatchable_item: to_camel_case(&variant.name),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V16 Metadata Extraction
// ============================================================================

fn extract_dispatchables_v16(
    meta: &frame_metadata::v16::RuntimeMetadataV16,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsDispatchablesResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v16(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let calls_type_id = match &pallet.calls {
        Some(calls) => calls.ty.id,
        None => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let calls_type = meta.types.resolve(calls_type_id).ok_or_else(|| {
        PalletError::PalletNotFound(format!("Could not resolve calls type for {}", pallet_id))
    })?;

    let variants = match &calls_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            return Ok(PalletsDispatchablesResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    DispatchablesItems::OnlyIds(vec![])
                } else {
                    DispatchablesItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        DispatchablesItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
    } else {
        DispatchablesItems::Full(
            variants
                .iter()
                .map(|v| {
                    let fields: Vec<DispatchableField> = v
                        .fields
                        .iter()
                        .map(|f| {
                            let type_name = f.type_name.clone();
                            DispatchableField {
                                name: f.name.clone().unwrap_or_default(),
                                ty: f.ty.id.to_string(),
                                type_name,
                                docs: f.docs.clone(),
                            }
                        })
                        .collect();

                    let args: Vec<DispatchableArg> = v
                        .fields
                        .iter()
                        .map(|f| {
                            let field_name = f.name.clone().unwrap_or_default();
                            let resolved_type = resolve_type_name_v14(&meta.types, f.ty.id);
                            let type_name = f
                                .type_name
                                .clone()
                                .map(|tn| simplify_type_name(&tn))
                                .unwrap_or_else(|| resolved_type.clone());
                            DispatchableArg {
                                name: to_camel_case(&field_name),
                                ty: resolved_type,
                                type_name,
                            }
                        })
                        .collect();

                    DispatchableItemMetadata {
                        name: v.name.clone(),
                        fields,
                        index: v.index.to_string(),
                        docs: v.docs.clone(),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsDispatchablesResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_dispatchable_item_v16(
    meta: &frame_metadata::v16::RuntimeMetadataV16,
    pallet_id: &str,
    dispatchable_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletDispatchableItemResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v16(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let calls_type_id = match &pallet.calls {
        Some(calls) => calls.ty.id,
        None => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let calls_type = meta
        .types
        .resolve(calls_type_id)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let variants = match &calls_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            return Err(PalletError::DispatchableNotFound(
                dispatchable_id.to_string(),
            ));
        }
    };

    let dispatchable_id_lower = dispatchable_id.to_lowercase();
    let variant = variants
        .iter()
        .find(|v| v.name.to_lowercase() == dispatchable_id_lower)
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.to_string()))?;

    let metadata = if include_metadata {
        let fields: Vec<DispatchableField> = variant
            .fields
            .iter()
            .map(|f| {
                let type_name = f.type_name.clone();
                DispatchableField {
                    name: f.name.clone().unwrap_or_default(),
                    ty: f.ty.id.to_string(),
                    type_name,
                    docs: f.docs.clone(),
                }
            })
            .collect();

        let args: Vec<DispatchableArg> = variant
            .fields
            .iter()
            .map(|f| {
                let field_name = f.name.clone().unwrap_or_default();
                let resolved_type = resolve_type_name_v14(&meta.types, f.ty.id);
                let type_name = f
                    .type_name
                    .clone()
                    .map(|tn| simplify_type_name(&tn))
                    .unwrap_or_else(|| resolved_type.clone());
                DispatchableArg {
                    name: to_camel_case(&field_name),
                    ty: resolved_type,
                    type_name,
                }
            })
            .collect();

        Some(DispatchableItemMetadata {
            name: variant.name.clone(),
            fields,
            index: variant.index.to_string(),
            docs: variant.docs.clone(),
            args,
        })
    } else {
        None
    };

    Ok(PalletDispatchableItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        dispatchable_item: to_camel_case(&variant.name),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatchables_items_serialization() {
        let items =
            DispatchablesItems::OnlyIds(vec!["transfer".to_string(), "approve".to_string()]);
        let json = serde_json::to_string(&items).expect("Failed to serialize DispatchablesItems");
        assert!(json.contains("transfer"));
        assert!(json.contains("approve"));
    }

    #[test]
    fn test_dispatchable_field_serialization() {
        let field = DispatchableField {
            name: "amount".to_string(),
            ty: "u128".to_string(),
            type_name: Some("Balance".to_string()),
            docs: vec![],
        };
        let json = serde_json::to_string(&field).expect("Failed to serialize DispatchableField");
        assert!(json.contains("\"name\":\"amount\""));
        assert!(json.contains("\"type\":\"u128\""));
        assert!(json.contains("\"typeName\":\"Balance\""));
    }

    #[test]
    fn test_dispatchable_item_metadata_serialization() {
        let metadata = DispatchableItemMetadata {
            name: "transfer".to_string(),
            fields: vec![],
            index: "0".to_string(),
            docs: vec!["Transfer tokens".to_string()],
            args: vec![],
        };
        let json =
            serde_json::to_string(&metadata).expect("Failed to serialize DispatchableItemMetadata");
        assert!(json.contains("\"name\":\"transfer\""));
        assert!(json.contains("\"index\":\"0\""));
    }
}
