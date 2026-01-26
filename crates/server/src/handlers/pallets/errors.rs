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
        return Ok(
            (StatusCode::OK, Json(Vec::<PalletErrorItemResponse>::new())).into_response(),
        );
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
