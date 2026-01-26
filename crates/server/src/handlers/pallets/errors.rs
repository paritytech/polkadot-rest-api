//! Handler for the `/pallets/{palletId}/errors` endpoint.
//!
//! This endpoint returns the errors defined in a pallet's metadata.
//! It supports querying at specific blocks and relay chain block resolution
//! for Asset Hub chains.
//!
//! # Sidecar Compatibility
//!
//! This endpoint aims to match the Sidecar `/pallets/{palletId}/errors` response format.

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
use heck::ToLowerCamelCase;
use parity_scale_codec::Decode;
use serde::Serialize;
use subxt_rpcs::rpc_params;

// ============================================================================
// Response Types
// ============================================================================

/// Response for the `/pallets/{palletId}/errors` endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsErrorsResponse {
    /// Block reference information.
    pub at: AtResponse,

    /// The pallet name (lowercase).
    pub pallet: String,

    /// The pallet index in the metadata.
    pub pallet_index: String,

    /// The list of errors (full metadata or just names).
    pub items: ErrorsItems,

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

/// Errors items - either full metadata or just names.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ErrorsItems {
    /// Full error metadata.
    Full(Vec<ErrorItemMetadata>),

    /// Only error names (when `onlyIds=true`).
    OnlyIds(Vec<String>),
}

/// Metadata for a single error.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorItemMetadata {
    /// The error name.
    pub name: String,

    /// The fields of the error (usually empty for errors).
    pub fields: Vec<ErrorField>,

    /// The index of the error in the pallet's error enum.
    pub index: String,

    /// Documentation for the error.
    pub docs: Vec<String>,

    /// Arguments for the error (for Sidecar compatibility, usually empty for errors).
    pub args: Vec<ErrorArg>,
}

/// An argument of an error (for Sidecar compatibility).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorArg {
    /// The argument name (camelCase).
    pub name: String,

    /// The resolved type name from the type registry.
    #[serde(rename = "type")]
    pub ty: String,

    /// The simplified type name (without generics).
    pub type_name: String,
}

/// A field of an error (with type ID).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorField {
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

/// Response for the `/pallets/{palletId}/errors/{errorItemId}` endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletErrorItemResponse {
    /// Block reference information.
    pub at: AtResponse,

    /// The pallet name (lowercase).
    pub pallet: String,

    /// The pallet index in the metadata.
    pub pallet_index: String,

    /// The error name (camelCase).
    pub error_item: String,

    /// Full metadata for the error (only when `metadata=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ErrorItemMetadata>,

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

/// Handler for `GET /pallets/{palletId}/errors`.
///
/// Returns the errors defined in the specified pallet.
///
/// # Query Parameters
///
/// - `at`: Block hash or number to query at. Defaults to the latest block.
/// - `onlyIds`: If `true`, only return error names without full metadata.
/// - `useRcBlock`: If `true`, resolve the block from the relay chain (Asset Hub only).
///
/// # Errors
///
/// - `400 Bad Request`: Invalid block parameter or unsupported `useRcBlock` usage.
/// - `404 Not Found`: Pallet not found in metadata.
/// - `500 Internal Server Error`: Unsupported metadata version.
/// - `503 Service Unavailable`: RPC connection lost.
pub async fn get_pallet_errors(
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

    // Fetch raw metadata via RPC to access all metadata versions (V9-V16)
    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let metadata = fetch_runtime_metadata(&state, &block_hash).await?;

    let response = extract_errors_from_metadata(
        &metadata,
        &pallet_id,
        at,
        params.only_ids,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Handler for `GET /pallets/{palletId}/errors/{errorItemId}`.
///
/// Returns a single error defined in the specified pallet.
///
/// # Query Parameters
///
/// - `at`: Block hash or number to query at. Defaults to the latest block.
/// - `metadata`: If `true`, include full metadata for the error.
/// - `useRcBlock`: If `true`, resolve the block from the relay chain (Asset Hub only).
///
/// # Errors
///
/// - `400 Bad Request`: Invalid block parameter or unsupported `useRcBlock` usage.
/// - `404 Not Found`: Pallet or error not found in metadata.
/// - `500 Internal Server Error`: Unsupported metadata version.
/// - `503 Service Unavailable`: RPC connection lost.
pub async fn get_pallet_error_item(
    State(state): State<AppState>,
    Path((pallet_id, error_id)): Path<(String, String)>,
    Query(params): Query<PalletItemQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_error_item_use_rc_block(state, pallet_id, error_id, params).await;
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

    // Fetch raw metadata via RPC to access all metadata versions (V9-V16)
    let block_hash = format!("{:#x}", client_at_block.block_hash());
    let metadata = fetch_runtime_metadata(&state, &block_hash).await?;

    let response = extract_error_item_from_metadata(
        &metadata,
        &pallet_id,
        &error_id,
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
///
/// When `useRcBlock=true`, this function:
/// 1. Resolves the relay chain block
/// 2. Finds ALL Asset Hub blocks contained in that RC block
/// 3. Returns an ARRAY of responses, one for each AH block
/// 4. Returns an empty array if no AH blocks are found
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

    // If no Asset Hub blocks found, return empty array (matching Sidecar behavior)
    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(Vec::<PalletsErrorsResponse>::new())).into_response());
    }

    // Process each Asset Hub block and collect results
    let mut results = Vec::new();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let rc_block_number = rc_resolved_block.number.to_string();

    for ah_block in &ah_blocks {
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
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        };

        let response =
            extract_errors_from_metadata(&metadata, &pallet_id, at, params.only_ids, rc_fields)?;

        results.push(response);
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

/// Handle requests with `useRcBlock=true` for Asset Hub chains (single error item).
///
/// When `useRcBlock=true`, this function:
/// 1. Resolves the relay chain block
/// 2. Finds ALL Asset Hub blocks contained in that RC block
/// 3. Returns an ARRAY of responses, one for each AH block
/// 4. Returns an empty array if no AH blocks are found
async fn handle_error_item_use_rc_block(
    state: AppState,
    pallet_id: String,
    error_id: String,
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

    // If no Asset Hub blocks found, return empty array (matching Sidecar behavior)
    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(Vec::<PalletErrorItemResponse>::new())).into_response());
    }

    // Process each Asset Hub block and collect results
    let mut results = Vec::new();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let rc_block_number = rc_resolved_block.number.to_string();

    for ah_block in &ah_blocks {
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
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        };

        let response = extract_error_item_from_metadata(
            &metadata,
            &pallet_id,
            &error_id,
            at,
            params.metadata,
            rc_fields,
        )?;

        results.push(response);
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

// ============================================================================
// Metadata Extraction
// ============================================================================

/// Extract errors from runtime metadata.
fn extract_errors_from_metadata(
    metadata: &RuntimeMetadata,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
    use RuntimeMetadata::*;
    match metadata {
        V9(meta) => extract_errors_v9(meta, pallet_id, at, only_ids, rc_fields),
        V10(meta) => extract_errors_v10(meta, pallet_id, at, only_ids, rc_fields),
        V11(meta) => extract_errors_v11(meta, pallet_id, at, only_ids, rc_fields),
        V12(meta) => extract_errors_v12(meta, pallet_id, at, only_ids, rc_fields),
        V13(meta) => extract_errors_v13(meta, pallet_id, at, only_ids, rc_fields),
        V14(meta) => extract_errors_v14(meta, pallet_id, at, only_ids, rc_fields),
        V15(meta) => extract_errors_v15(meta, pallet_id, at, only_ids, rc_fields),
        V16(meta) => extract_errors_v16(meta, pallet_id, at, only_ids, rc_fields),
        _ => Err(PalletError::UnsupportedMetadataVersion),
    }
}

/// Extract a single error from runtime metadata.
fn extract_error_item_from_metadata(
    metadata: &RuntimeMetadata,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
    use RuntimeMetadata::*;
    match metadata {
        V9(meta) => {
            extract_error_item_v9(meta, pallet_id, error_id, at, include_metadata, rc_fields)
        }
        V10(meta) => {
            extract_error_item_v10(meta, pallet_id, error_id, at, include_metadata, rc_fields)
        }
        V11(meta) => {
            extract_error_item_v11(meta, pallet_id, error_id, at, include_metadata, rc_fields)
        }
        V12(meta) => {
            extract_error_item_v12(meta, pallet_id, error_id, at, include_metadata, rc_fields)
        }
        V13(meta) => {
            extract_error_item_v13(meta, pallet_id, error_id, at, include_metadata, rc_fields)
        }
        V14(meta) => {
            extract_error_item_v14(meta, pallet_id, error_id, at, include_metadata, rc_fields)
        }
        V15(meta) => {
            extract_error_item_v15(meta, pallet_id, error_id, at, include_metadata, rc_fields)
        }
        V16(meta) => {
            extract_error_item_v16(meta, pallet_id, error_id, at, include_metadata, rc_fields)
        }
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

// ============================================================================
// V9 Metadata Extraction
// ============================================================================

fn extract_errors_v9(
    meta: &frame_metadata::v9::RuntimeMetadataV9,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
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
            .map(|(idx, m)| (m, idx as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        ErrorsItems::OnlyIds(
            errors
                .iter()
                .map(|e| extract_str(&e.name).to_string())
                .collect(),
        )
    } else {
        ErrorsItems::Full(
            errors
                .iter()
                .enumerate()
                .map(|(idx, e)| ErrorItemMetadata {
                    name: extract_str(&e.name).to_string(),
                    fields: vec![], // V9 errors don't have fields
                    index: idx.to_string(),
                    docs: extract_docs(&e.documentation),
                    args: vec![],
                })
                .collect(),
        )
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_error_item_v9(
    meta: &frame_metadata::v9::RuntimeMetadataV9,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
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
            .map(|(idx, m)| (m, idx as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    let error_id_lower = error_id.to_lowercase();
    let (idx, error) = errors
        .iter()
        .enumerate()
        .find(|(_, e)| extract_str(&e.name).to_lowercase() == error_id_lower)
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_name = extract_str(&error.name).to_string();

    let metadata = if include_metadata {
        Some(ErrorItemMetadata {
            name: error_name.clone(),
            fields: vec![],
            index: idx.to_string(),
            docs: extract_docs(&error.documentation),
            args: vec![],
        })
    } else {
        None
    };

    Ok(PalletErrorItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        error_item: error_name.to_lower_camel_case(),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V10 Metadata Extraction
// ============================================================================

fn extract_errors_v10(
    meta: &frame_metadata::v10::RuntimeMetadataV10,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
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
            .map(|(idx, m)| (m, idx as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        ErrorsItems::OnlyIds(
            errors
                .iter()
                .map(|e| extract_str(&e.name).to_string())
                .collect(),
        )
    } else {
        ErrorsItems::Full(
            errors
                .iter()
                .enumerate()
                .map(|(idx, e)| ErrorItemMetadata {
                    name: extract_str(&e.name).to_string(),
                    fields: vec![],
                    index: idx.to_string(),
                    docs: extract_docs(&e.documentation),
                    args: vec![],
                })
                .collect(),
        )
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_error_item_v10(
    meta: &frame_metadata::v10::RuntimeMetadataV10,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
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
            .map(|(idx, m)| (m, idx as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    let error_id_lower = error_id.to_lowercase();
    let (idx, error) = errors
        .iter()
        .enumerate()
        .find(|(_, e)| extract_str(&e.name).to_lowercase() == error_id_lower)
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_name = extract_str(&error.name).to_string();

    let metadata = if include_metadata {
        Some(ErrorItemMetadata {
            name: error_name.clone(),
            fields: vec![],
            index: idx.to_string(),
            docs: extract_docs(&error.documentation),
            args: vec![],
        })
    } else {
        None
    };

    Ok(PalletErrorItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        error_item: error_name.to_lower_camel_case(),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V11 Metadata Extraction
// ============================================================================

fn extract_errors_v11(
    meta: &frame_metadata::v11::RuntimeMetadataV11,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
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
            .map(|(idx, m)| (m, idx as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        ErrorsItems::OnlyIds(
            errors
                .iter()
                .map(|e| extract_str(&e.name).to_string())
                .collect(),
        )
    } else {
        ErrorsItems::Full(
            errors
                .iter()
                .enumerate()
                .map(|(idx, e)| ErrorItemMetadata {
                    name: extract_str(&e.name).to_string(),
                    fields: vec![],
                    index: idx.to_string(),
                    docs: extract_docs(&e.documentation),
                    args: vec![],
                })
                .collect(),
        )
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_error_item_v11(
    meta: &frame_metadata::v11::RuntimeMetadataV11,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
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
            .map(|(idx, m)| (m, idx as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str(&module.name).to_string();

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    let error_id_lower = error_id.to_lowercase();
    let (idx, error) = errors
        .iter()
        .enumerate()
        .find(|(_, e)| extract_str(&e.name).to_lowercase() == error_id_lower)
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_name = extract_str(&error.name).to_string();

    let metadata = if include_metadata {
        Some(ErrorItemMetadata {
            name: error_name.clone(),
            fields: vec![],
            index: idx.to_string(),
            docs: extract_docs(&error.documentation),
            args: vec![],
        })
    } else {
        None
    };

    Ok(PalletErrorItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        error_item: error_name.to_lower_camel_case(),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V12 Metadata Extraction
// ============================================================================

fn extract_errors_v12(
    meta: &frame_metadata::v12::RuntimeMetadataV12,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
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

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        ErrorsItems::OnlyIds(
            errors
                .iter()
                .map(|e| extract_str(&e.name).to_string())
                .collect(),
        )
    } else {
        ErrorsItems::Full(
            errors
                .iter()
                .enumerate()
                .map(|(idx, e)| ErrorItemMetadata {
                    name: extract_str(&e.name).to_string(),
                    fields: vec![],
                    index: idx.to_string(),
                    docs: extract_docs(&e.documentation),
                    args: vec![],
                })
                .collect(),
        )
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_error_item_v12(
    meta: &frame_metadata::v12::RuntimeMetadataV12,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
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

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    let error_id_lower = error_id.to_lowercase();
    let (idx, error) = errors
        .iter()
        .enumerate()
        .find(|(_, e)| extract_str(&e.name).to_lowercase() == error_id_lower)
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_name = extract_str(&error.name).to_string();

    let metadata = if include_metadata {
        Some(ErrorItemMetadata {
            name: error_name.clone(),
            fields: vec![],
            index: idx.to_string(),
            docs: extract_docs(&error.documentation),
            args: vec![],
        })
    } else {
        None
    };

    Ok(PalletErrorItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        error_item: error_name.to_lower_camel_case(),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V13 Metadata Extraction
// ============================================================================

fn extract_errors_v13(
    meta: &frame_metadata::v13::RuntimeMetadataV13,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
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

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: module_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        ErrorsItems::OnlyIds(
            errors
                .iter()
                .map(|e| extract_str(&e.name).to_string())
                .collect(),
        )
    } else {
        ErrorsItems::Full(
            errors
                .iter()
                .enumerate()
                .map(|(idx, e)| ErrorItemMetadata {
                    name: extract_str(&e.name).to_string(),
                    fields: vec![],
                    index: idx.to_string(),
                    docs: extract_docs(&e.documentation),
                    args: vec![],
                })
                .collect(),
        )
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_error_item_v13(
    meta: &frame_metadata::v13::RuntimeMetadataV13,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
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

    let errors = match &module.errors {
        DecodeDifferent::Decoded(errors) => errors,
        _ => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    let error_id_lower = error_id.to_lowercase();
    let (idx, error) = errors
        .iter()
        .enumerate()
        .find(|(_, e)| extract_str(&e.name).to_lowercase() == error_id_lower)
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_name = extract_str(&error.name).to_string();

    let metadata = if include_metadata {
        Some(ErrorItemMetadata {
            name: error_name.clone(),
            fields: vec![],
            index: idx.to_string(),
            docs: extract_docs(&error.documentation),
            args: vec![],
        })
    } else {
        None
    };

    Ok(PalletErrorItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        error_item: error_name.to_lower_camel_case(),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V14 Metadata Extraction
// ============================================================================

fn extract_errors_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v14(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get the error type from the pallet
    let error_type_id = match &pallet.error {
        Some(error) => error.ty.id,
        None => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    // Resolve the type to get variants (errors)
    let error_type = meta.types.resolve(error_type_id).ok_or_else(|| {
        PalletError::PalletNotFound(format!("Could not resolve error type for {}", pallet_id))
    })?;

    let variants = match &error_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        ErrorsItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
    } else {
        ErrorsItems::Full(
            variants
                .iter()
                .map(|v| {
                    let fields: Vec<ErrorField> = v
                        .fields
                        .iter()
                        .map(|f| {
                            let type_name = f.type_name.clone();
                            ErrorField {
                                name: f.name.clone().unwrap_or_default(),
                                ty: f.ty.id.to_string(),
                                type_name,
                                docs: f.docs.clone(),
                            }
                        })
                        .collect();

                    ErrorItemMetadata {
                        name: v.name.clone(),
                        fields,
                        index: v.index.to_string(),
                        docs: v.docs.clone(),
                        args: vec![],
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_error_item_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v14(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get the error type from the pallet
    let error_type_id = match &pallet.error {
        Some(error) => error.ty.id,
        None => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    // Resolve the type to get variants (errors)
    let error_type = meta.types.resolve(error_type_id).ok_or_else(|| {
        PalletError::PalletNotFound(format!("Could not resolve error type for {}", pallet_id))
    })?;

    let variants = match &error_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    let error_id_lower = error_id.to_lowercase();
    let variant = variants
        .iter()
        .find(|v| v.name.to_lowercase() == error_id_lower)
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_name = variant.name.clone();

    let metadata = if include_metadata {
        let fields: Vec<ErrorField> = variant
            .fields
            .iter()
            .map(|f| {
                let type_name = f.type_name.clone();
                ErrorField {
                    name: f.name.clone().unwrap_or_default(),
                    ty: f.ty.id.to_string(),
                    type_name,
                    docs: f.docs.clone(),
                }
            })
            .collect();

        Some(ErrorItemMetadata {
            name: error_name.clone(),
            fields,
            index: variant.index.to_string(),
            docs: variant.docs.clone(),
            args: vec![],
        })
    } else {
        None
    };

    Ok(PalletErrorItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        error_item: error_name.to_lower_camel_case(),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V15 Metadata Extraction
// ============================================================================

fn extract_errors_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v15(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get the error type from the pallet
    let error_type_id = match &pallet.error {
        Some(error) => error.ty.id,
        None => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    // Resolve the type to get variants (errors)
    let error_type = meta.types.resolve(error_type_id).ok_or_else(|| {
        PalletError::PalletNotFound(format!("Could not resolve error type for {}", pallet_id))
    })?;

    let variants = match &error_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        ErrorsItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
    } else {
        ErrorsItems::Full(
            variants
                .iter()
                .map(|v| {
                    let fields: Vec<ErrorField> = v
                        .fields
                        .iter()
                        .map(|f| {
                            let type_name = f.type_name.clone();
                            ErrorField {
                                name: f.name.clone().unwrap_or_default(),
                                ty: f.ty.id.to_string(),
                                type_name,
                                docs: f.docs.clone(),
                            }
                        })
                        .collect();

                    ErrorItemMetadata {
                        name: v.name.clone(),
                        fields,
                        index: v.index.to_string(),
                        docs: v.docs.clone(),
                        args: vec![],
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_error_item_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v15(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get the error type from the pallet
    let error_type_id = match &pallet.error {
        Some(error) => error.ty.id,
        None => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    // Resolve the type to get variants (errors)
    let error_type = meta.types.resolve(error_type_id).ok_or_else(|| {
        PalletError::PalletNotFound(format!("Could not resolve error type for {}", pallet_id))
    })?;

    let variants = match &error_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    let error_id_lower = error_id.to_lowercase();
    let variant = variants
        .iter()
        .find(|v| v.name.to_lowercase() == error_id_lower)
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_name = variant.name.clone();

    let metadata = if include_metadata {
        let fields: Vec<ErrorField> = variant
            .fields
            .iter()
            .map(|f| {
                let type_name = f.type_name.clone();
                ErrorField {
                    name: f.name.clone().unwrap_or_default(),
                    ty: f.ty.id.to_string(),
                    type_name,
                    docs: f.docs.clone(),
                }
            })
            .collect();

        Some(ErrorItemMetadata {
            name: error_name.clone(),
            fields,
            index: variant.index.to_string(),
            docs: variant.docs.clone(),
            args: vec![],
        })
    } else {
        None
    };

    Ok(PalletErrorItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        error_item: error_name.to_lower_camel_case(),
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// V16 Metadata Extraction
// ============================================================================

fn extract_errors_v16(
    meta: &frame_metadata::v16::RuntimeMetadataV16,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v16(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get the error type from the pallet
    let error_type_id = match &pallet.error {
        Some(error) => error.ty.id,
        None => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    // Resolve the type to get variants (errors)
    let error_type = meta.types.resolve(error_type_id).ok_or_else(|| {
        PalletError::PalletNotFound(format!("Could not resolve error type for {}", pallet_id))
    })?;

    let variants = match &error_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            return Ok(PalletsErrorsResponse {
                at,
                pallet: pallet_name.to_lowercase(),
                pallet_index: pallet_index.to_string(),
                items: if only_ids {
                    ErrorsItems::OnlyIds(vec![])
                } else {
                    ErrorsItems::Full(vec![])
                },
                rc_block_hash: rc_fields.rc_block_hash,
                rc_block_number: rc_fields.rc_block_number,
                ah_timestamp: rc_fields.ah_timestamp,
            });
        }
    };

    let items = if only_ids {
        ErrorsItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
    } else {
        ErrorsItems::Full(
            variants
                .iter()
                .map(|v| {
                    let fields: Vec<ErrorField> = v
                        .fields
                        .iter()
                        .map(|f| {
                            let type_name = f.type_name.clone();
                            ErrorField {
                                name: f.name.clone().unwrap_or_default(),
                                ty: f.ty.id.to_string(),
                                type_name,
                                docs: f.docs.clone(),
                            }
                        })
                        .collect();

                    ErrorItemMetadata {
                        name: v.name.clone(),
                        fields,
                        index: v.index.to_string(),
                        docs: v.docs.clone(),
                        args: vec![],
                    }
                })
                .collect(),
        )
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_error_item_v16(
    meta: &frame_metadata::v16::RuntimeMetadataV16,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v16(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get the error type from the pallet
    let error_type_id = match &pallet.error {
        Some(error) => error.ty.id,
        None => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    // Resolve the type to get variants (errors)
    let error_type = meta.types.resolve(error_type_id).ok_or_else(|| {
        PalletError::PalletNotFound(format!("Could not resolve error type for {}", pallet_id))
    })?;

    let variants = match &error_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => return Err(PalletError::ErrorItemNotFound(error_id.to_string())),
    };

    let error_id_lower = error_id.to_lowercase();
    let variant = variants
        .iter()
        .find(|v| v.name.to_lowercase() == error_id_lower)
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_name = variant.name.clone();

    let metadata = if include_metadata {
        let fields: Vec<ErrorField> = variant
            .fields
            .iter()
            .map(|f| {
                let type_name = f.type_name.clone();
                ErrorField {
                    name: f.name.clone().unwrap_or_default(),
                    ty: f.ty.id.to_string(),
                    type_name,
                    docs: f.docs.clone(),
                }
            })
            .collect();

        Some(ErrorItemMetadata {
            name: error_name.clone(),
            fields,
            index: variant.index.to_string(),
            docs: variant.docs.clone(),
            args: vec![],
        })
    } else {
        None
    };

    Ok(PalletErrorItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        error_item: error_name.to_lower_camel_case(),
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

    // ========================================================================
    // ErrorsItems Tests
    // ========================================================================

    #[test]
    fn test_errors_items_only_ids_serialization() {
        let items = ErrorsItems::OnlyIds(vec![
            "InsufficientBalance".to_string(),
            "InvalidOrigin".to_string(),
        ]);
        let json = serde_json::to_string(&items).expect("Failed to serialize ErrorsItems::OnlyIds");
        assert!(json.contains("InsufficientBalance"));
        assert!(json.contains("InvalidOrigin"));
        // OnlyIds should serialize as a flat array
        assert!(json.starts_with('['));
        assert!(json.ends_with(']'));
    }

    #[test]
    fn test_errors_items_full_serialization() {
        let items = ErrorsItems::Full(vec![ErrorItemMetadata {
            name: "InsufficientBalance".to_string(),
            fields: vec![],
            index: "0".to_string(),
            docs: vec!["Not enough balance".to_string()],
            args: vec![],
        }]);
        let json = serde_json::to_string(&items).expect("Failed to serialize ErrorsItems::Full");
        assert!(json.contains("\"name\":\"InsufficientBalance\""));
        assert!(json.contains("\"index\":\"0\""));
        assert!(json.contains("\"docs\":[\"Not enough balance\"]"));
    }

    #[test]
    fn test_errors_items_empty_only_ids() {
        let items = ErrorsItems::OnlyIds(vec![]);
        let json = serde_json::to_string(&items).expect("Failed to serialize empty OnlyIds");
        assert_eq!(json, "[]");
    }

    #[test]
    fn test_errors_items_empty_full() {
        let items = ErrorsItems::Full(vec![]);
        let json = serde_json::to_string(&items).expect("Failed to serialize empty Full");
        assert_eq!(json, "[]");
    }

    // ========================================================================
    // ErrorField Tests
    // ========================================================================

    #[test]
    fn test_error_field_serialization() {
        let field = ErrorField {
            name: "amount".to_string(),
            ty: "6".to_string(),
            type_name: Some("Balance".to_string()),
            docs: vec!["The amount that was attempted".to_string()],
        };
        let json = serde_json::to_string(&field).expect("Failed to serialize ErrorField");
        assert!(json.contains("\"name\":\"amount\""));
        assert!(json.contains("\"type\":\"6\""));
        assert!(json.contains("\"typeName\":\"Balance\""));
        assert!(json.contains("\"docs\":[\"The amount that was attempted\"]"));
    }

    #[test]
    fn test_error_field_without_type_name() {
        let field = ErrorField {
            name: "value".to_string(),
            ty: "10".to_string(),
            type_name: None,
            docs: vec![],
        };
        let json = serde_json::to_string(&field).expect("Failed to serialize ErrorField");
        assert!(json.contains("\"name\":\"value\""));
        assert!(json.contains("\"type\":\"10\""));
        // typeName should be omitted when None
        assert!(!json.contains("typeName"));
    }

    #[test]
    fn test_error_field_empty_name() {
        let field = ErrorField {
            name: "".to_string(),
            ty: "1".to_string(),
            type_name: None,
            docs: vec![],
        };
        let json = serde_json::to_string(&field).expect("Failed to serialize ErrorField");
        assert!(json.contains("\"name\":\"\""));
    }

    // ========================================================================
    // ErrorArg Tests
    // ========================================================================

    #[test]
    fn test_error_arg_serialization() {
        let arg = ErrorArg {
            name: "needed".to_string(),
            ty: "T::Balance".to_string(),
            type_name: "Balance".to_string(),
        };
        let json = serde_json::to_string(&arg).expect("Failed to serialize ErrorArg");
        assert!(json.contains("\"name\":\"needed\""));
        assert!(json.contains("\"type\":\"T::Balance\""));
        assert!(json.contains("\"typeName\":\"Balance\""));
    }

    #[test]
    fn test_error_arg_camel_case_name() {
        let arg = ErrorArg {
            name: "existentialDeposit".to_string(),
            ty: "u128".to_string(),
            type_name: "u128".to_string(),
        };
        let json = serde_json::to_string(&arg).expect("Failed to serialize ErrorArg");
        assert!(json.contains("\"name\":\"existentialDeposit\""));
    }

    // ========================================================================
    // ErrorItemMetadata Tests
    // ========================================================================

    #[test]
    fn test_error_item_metadata_serialization() {
        let metadata = ErrorItemMetadata {
            name: "InsufficientBalance".to_string(),
            fields: vec![],
            index: "0".to_string(),
            docs: vec![
                "Account balance is too low.".to_string(),
                "This error occurs when the account does not have enough funds.".to_string(),
            ],
            args: vec![],
        };
        let json = serde_json::to_string(&metadata).expect("Failed to serialize ErrorItemMetadata");
        assert!(json.contains("\"name\":\"InsufficientBalance\""));
        assert!(json.contains("\"index\":\"0\""));
        assert!(json.contains("\"docs\":["));
        assert!(json.contains("Account balance is too low."));
    }

    #[test]
    fn test_error_item_metadata_with_fields() {
        let metadata = ErrorItemMetadata {
            name: "InsufficientFunds".to_string(),
            fields: vec![
                ErrorField {
                    name: "needed".to_string(),
                    ty: "6".to_string(),
                    type_name: Some("Balance".to_string()),
                    docs: vec![],
                },
                ErrorField {
                    name: "have".to_string(),
                    ty: "6".to_string(),
                    type_name: Some("Balance".to_string()),
                    docs: vec![],
                },
            ],
            index: "1".to_string(),
            docs: vec!["Not enough funds".to_string()],
            args: vec![],
        };
        let json = serde_json::to_string(&metadata).expect("Failed to serialize ErrorItemMetadata");
        assert!(json.contains("\"name\":\"InsufficientFunds\""));
        assert!(json.contains("\"needed\""));
        assert!(json.contains("\"have\""));
        assert!(json.contains("\"fields\":["));
    }

    #[test]
    fn test_error_item_metadata_with_args() {
        let metadata = ErrorItemMetadata {
            name: "TooExpensive".to_string(),
            fields: vec![],
            index: "5".to_string(),
            docs: vec![],
            args: vec![
                ErrorArg {
                    name: "fee".to_string(),
                    ty: "u128".to_string(),
                    type_name: "Balance".to_string(),
                },
                ErrorArg {
                    name: "balance".to_string(),
                    ty: "u128".to_string(),
                    type_name: "Balance".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&metadata).expect("Failed to serialize ErrorItemMetadata");
        assert!(json.contains("\"args\":["));
        assert!(json.contains("\"fee\""));
        assert!(json.contains("\"balance\""));
    }

    #[test]
    fn test_error_item_metadata_empty_docs() {
        let metadata = ErrorItemMetadata {
            name: "Unknown".to_string(),
            fields: vec![],
            index: "99".to_string(),
            docs: vec![],
            args: vec![],
        };
        let json = serde_json::to_string(&metadata).expect("Failed to serialize ErrorItemMetadata");
        assert!(json.contains("\"docs\":[]"));
    }

    // ========================================================================
    // AtResponse Tests
    // ========================================================================

    #[test]
    fn test_at_response_serialization() {
        let at = AtResponse {
            hash: "0x1234567890abcdef".to_string(),
            height: "12345".to_string(),
        };
        let json = serde_json::to_string(&at).expect("Failed to serialize AtResponse");
        assert!(json.contains("\"hash\":\"0x1234567890abcdef\""));
        assert!(json.contains("\"height\":\"12345\""));
    }

    // ========================================================================
    // PalletsErrorsResponse Tests
    // ========================================================================

    #[test]
    fn test_pallets_errors_response_serialization() {
        let response = PalletsErrorsResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "1000".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "10".to_string(),
            items: ErrorsItems::OnlyIds(vec!["InsufficientBalance".to_string()]),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };
        let json =
            serde_json::to_string(&response).expect("Failed to serialize PalletsErrorsResponse");
        assert!(json.contains("\"pallet\":\"balances\""));
        assert!(json.contains("\"palletIndex\":\"10\""));
        assert!(json.contains("\"items\":["));
    }

    #[test]
    fn test_pallets_errors_response_with_rc_block_fields() {
        let response = PalletsErrorsResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "5000".to_string(),
            },
            pallet: "system".to_string(),
            pallet_index: "0".to_string(),
            items: ErrorsItems::Full(vec![]),
            rc_block_hash: Some("0xrelaychain".to_string()),
            rc_block_number: Some("10000".to_string()),
            ah_timestamp: Some("1700000000000".to_string()),
        };
        let json =
            serde_json::to_string(&response).expect("Failed to serialize PalletsErrorsResponse");
        assert!(json.contains("\"rcBlockHash\":\"0xrelaychain\""));
        assert!(json.contains("\"rcBlockNumber\":\"10000\""));
        assert!(json.contains("\"ahTimestamp\":\"1700000000000\""));
    }

    #[test]
    fn test_pallets_errors_response_rc_fields_omitted_when_none() {
        let response = PalletsErrorsResponse {
            at: AtResponse {
                hash: "0xdef456".to_string(),
                height: "2000".to_string(),
            },
            pallet: "staking".to_string(),
            pallet_index: "6".to_string(),
            items: ErrorsItems::OnlyIds(vec![]),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };
        let json =
            serde_json::to_string(&response).expect("Failed to serialize PalletsErrorsResponse");
        assert!(!json.contains("rcBlockHash"));
        assert!(!json.contains("rcBlockNumber"));
        assert!(!json.contains("ahTimestamp"));
    }

    // ========================================================================
    // PalletErrorItemResponse Tests
    // ========================================================================

    #[test]
    fn test_pallet_error_item_response_serialization() {
        let response = PalletErrorItemResponse {
            at: AtResponse {
                hash: "0x999".to_string(),
                height: "3000".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "10".to_string(),
            error_item: "insufficientBalance".to_string(),
            metadata: None,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };
        let json =
            serde_json::to_string(&response).expect("Failed to serialize PalletErrorItemResponse");
        assert!(json.contains("\"pallet\":\"balances\""));
        assert!(json.contains("\"errorItem\":\"insufficientBalance\""));
        assert!(!json.contains("\"metadata\""));
    }

    #[test]
    fn test_pallet_error_item_response_with_metadata() {
        let response = PalletErrorItemResponse {
            at: AtResponse {
                hash: "0xaaa".to_string(),
                height: "4000".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "10".to_string(),
            error_item: "insufficientBalance".to_string(),
            metadata: Some(ErrorItemMetadata {
                name: "InsufficientBalance".to_string(),
                fields: vec![],
                index: "0".to_string(),
                docs: vec!["Balance too low".to_string()],
                args: vec![],
            }),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };
        let json =
            serde_json::to_string(&response).expect("Failed to serialize PalletErrorItemResponse");
        assert!(json.contains("\"metadata\":{"));
        assert!(json.contains("\"name\":\"InsufficientBalance\""));
    }

    #[test]
    fn test_pallet_error_item_response_with_rc_block_fields() {
        let response = PalletErrorItemResponse {
            at: AtResponse {
                hash: "0xbbb".to_string(),
                height: "5000".to_string(),
            },
            pallet: "assets".to_string(),
            pallet_index: "50".to_string(),
            error_item: "unknown".to_string(),
            metadata: None,
            rc_block_hash: Some("0xrelayblock".to_string()),
            rc_block_number: Some("12345".to_string()),
            ah_timestamp: Some("1705000000000".to_string()),
        };
        let json =
            serde_json::to_string(&response).expect("Failed to serialize PalletErrorItemResponse");
        assert!(json.contains("\"rcBlockHash\":\"0xrelayblock\""));
        assert!(json.contains("\"rcBlockNumber\":\"12345\""));
        assert!(json.contains("\"ahTimestamp\":\"1705000000000\""));
    }

    // ========================================================================
    // camelCase Conversion Tests
    // ========================================================================

    #[test]
    fn test_error_name_to_lower_camel_case() {
        // Test the heck crate's ToLowerCamelCase behavior
        assert_eq!(
            "InsufficientBalance".to_lower_camel_case(),
            "insufficientBalance"
        );
        assert_eq!("VestingBalance".to_lower_camel_case(), "vestingBalance");
        assert_eq!("Liquidity".to_lower_camel_case(), "liquidity");
        assert_eq!(
            "ExistentialDeposit".to_lower_camel_case(),
            "existentialDeposit"
        );
        assert_eq!("TooManyReserves".to_lower_camel_case(), "tooManyReserves");
        assert_eq!("TooManyHolds".to_lower_camel_case(), "tooManyHolds");
        assert_eq!("TooManyFreezes".to_lower_camel_case(), "tooManyFreezes");
        assert_eq!("IssuanceEmpty".to_lower_camel_case(), "issuanceEmpty");
        assert_eq!("DeltaZero".to_lower_camel_case(), "deltaZero");
    }

    #[test]
    fn test_single_word_error_name() {
        // Single word should start lowercase
        assert_eq!("Unknown".to_lower_camel_case(), "unknown");
        assert_eq!("Overflow".to_lower_camel_case(), "overflow");
    }

    #[test]
    fn test_acronym_error_name() {
        // Test acronym handling - heck treats consecutive caps as separate words
        assert_eq!("BadXCM".to_lower_camel_case(), "badXcm");
        assert_eq!("XCMError".to_lower_camel_case(), "xcmError");
    }

    // ========================================================================
    // RcBlockFields Tests
    // ========================================================================

    #[test]
    fn test_rc_block_fields_default() {
        let fields = RcBlockFields::default();
        assert!(fields.rc_block_hash.is_none());
        assert!(fields.rc_block_number.is_none());
        assert!(fields.ah_timestamp.is_none());
    }

    #[test]
    fn test_rc_block_fields_with_values() {
        let fields = RcBlockFields {
            rc_block_hash: Some("0xhash".to_string()),
            rc_block_number: Some("123".to_string()),
            ah_timestamp: Some("456".to_string()),
        };
        assert_eq!(fields.rc_block_hash, Some("0xhash".to_string()));
        assert_eq!(fields.rc_block_number, Some("123".to_string()));
        assert_eq!(fields.ah_timestamp, Some("456".to_string()));
    }

    // ========================================================================
    // Full JSON Structure Tests
    // ========================================================================

    #[test]
    fn test_full_errors_response_json_structure() {
        // Test that the JSON structure matches Sidecar format
        let response = PalletsErrorsResponse {
            at: AtResponse {
                hash: "0x7b60ca0d8cd3a8a5f6a79ae42c46c5fa3e9c82f6fd9b731bf81fb8296a0cee89"
                    .to_string(),
                height: "28490503".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "10".to_string(),
            items: ErrorsItems::Full(vec![
                ErrorItemMetadata {
                    name: "VestingBalance".to_string(),
                    fields: vec![],
                    index: "0".to_string(),
                    docs: vec!["Vesting balance too high to send value.".to_string()],
                    args: vec![],
                },
                ErrorItemMetadata {
                    name: "LiquidityRestrictions".to_string(),
                    fields: vec![],
                    index: "1".to_string(),
                    docs: vec!["Account liquidity restrictions prevent withdrawal.".to_string()],
                    args: vec![],
                },
            ]),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json_value: serde_json::Value = serde_json::to_value(&response).unwrap();

        // Verify top-level structure
        assert!(json_value.get("at").is_some());
        assert!(json_value.get("pallet").is_some());
        assert!(json_value.get("palletIndex").is_some());
        assert!(json_value.get("items").is_some());

        // Verify at block
        assert_eq!(json_value["at"]["height"].as_str().unwrap(), "28490503");

        // Verify pallet info
        assert_eq!(json_value["pallet"].as_str().unwrap(), "balances");
        assert_eq!(json_value["palletIndex"].as_str().unwrap(), "10");

        // Verify items array
        let items = json_value["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"].as_str().unwrap(), "VestingBalance");
        assert_eq!(items[1]["name"].as_str().unwrap(), "LiquidityRestrictions");
    }

    #[test]
    fn test_error_item_response_json_structure() {
        let response = PalletErrorItemResponse {
            at: AtResponse {
                hash: "0xabc".to_string(),
                height: "1000".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "10".to_string(),
            error_item: "insufficientBalance".to_string(),
            metadata: Some(ErrorItemMetadata {
                name: "InsufficientBalance".to_string(),
                fields: vec![],
                index: "2".to_string(),
                docs: vec!["Balance is too low.".to_string()],
                args: vec![],
            }),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json_value: serde_json::Value = serde_json::to_value(&response).unwrap();

        // Verify structure
        assert_eq!(
            json_value["errorItem"].as_str().unwrap(),
            "insufficientBalance"
        );
        assert!(json_value.get("metadata").is_some());
        assert_eq!(
            json_value["metadata"]["name"].as_str().unwrap(),
            "InsufficientBalance"
        );
        assert_eq!(json_value["metadata"]["index"].as_str().unwrap(), "2");
    }
}
