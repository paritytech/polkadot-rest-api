//! Handler for the `/pallets/{palletId}/consts` endpoint.
//!
//! This endpoint returns the constants defined in a pallet's metadata.
//! It supports querying at specific blocks and relay chain block resolution
//! for Asset Hub chains.
//!
//! # Sidecar Compatibility
//!
//! This endpoint aims to match the Sidecar `/pallets/{palletId}/consts` response format.

use crate::handlers::pallets::common::{
    AtResponse, DeprecationInfo, PalletError, PalletItemQueryParams, PalletQueryParams,
    RcBlockFields, find_pallet_v14, find_pallet_v15, find_pallet_v16,
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
use frame_metadata::{RuntimeMetadata, decode_different::DecodeDifferent};
use parity_scale_codec::Decode;
use serde::Serialize;

// ============================================================================
// Response Types
// ============================================================================

/// Response for the `/pallets/{palletId}/consts` endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsConstsResponse {
    /// Block reference information.
    pub at: AtResponse,

    /// The pallet name (lowercase).
    pub pallet: String,

    /// The pallet index in the metadata.
    pub pallet_index: String,

    /// The list of constants (full metadata or just names).
    pub items: ConstsItems,

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

/// Constants items - either full metadata or just names.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ConstsItems {
    /// Full constant metadata.
    Full(Vec<ConstItemMetadata>),

    /// Only constant names (when `onlyIds=true`).
    OnlyIds(Vec<String>),
}

/// Metadata for a single constant.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstItemMetadata {
    /// The constant name.
    pub name: String,

    /// The type ID in the type registry.
    #[serde(rename = "type")]
    pub ty: String,

    /// The SCALE-encoded value as a hex string.
    pub value: String,

    /// Documentation for the constant.
    pub docs: Vec<String>,

    /// Deprecation information (V15+ metadata only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecation_info: Option<DeprecationInfo>,
}

/// Response for the `/pallets/{palletId}/consts/{constantId}` endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletConstItemResponse {
    /// Block reference information.
    pub at: AtResponse,

    /// The pallet name (lowercase).
    pub pallet: String,

    /// The pallet index in the metadata.
    pub pallet_index: String,

    /// The constant name (camelCase).
    pub constants_item: String,

    /// Full metadata for the constant (only when `metadata=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ConstItemMetadata>,

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

/// Handler for `GET /pallets/{palletId}/consts`.
///
/// Returns the constants defined in the specified pallet.
///
/// # Query Parameters
///
/// - `at`: Block hash or number to query at. Defaults to the latest block.
/// - `onlyIds`: If `true`, only return constant names without full metadata.
/// - `useRcBlock`: If `true`, resolve the block from the relay chain (Asset Hub only).
///
/// # Errors
///
/// - `400 Bad Request`: Invalid block parameter or unsupported `useRcBlock` usage.
/// - `404 Not Found`: Pallet not found in metadata.
/// - `500 Internal Server Error`: Unsupported metadata version.
/// - `503 Service Unavailable`: RPC connection lost.
pub async fn get_pallets_consts(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<PalletQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, pallet_id, params).await;
    }

    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    let resolved_block = utils::resolve_block(&state, block_id).await?;
    let client_at_block = state.client.at(resolved_block.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: resolved_block.hash.clone(),
        height: resolved_block.number.to_string(),
    };

    let response = extract_consts_from_metadata(
        metadata,
        &pallet_id,
        at,
        params.only_ids,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Handler for `GET /pallets/{palletId}/consts/{constantId}`.
///
/// Returns a single constant defined in the specified pallet.
///
/// # Query Parameters
///
/// - `at`: Block hash or number to query at. Defaults to the latest block.
/// - `metadata`: If `true`, include full metadata for the constant.
/// - `useRcBlock`: If `true`, resolve the block from the relay chain (Asset Hub only).
///
/// # Errors
///
/// - `400 Bad Request`: Invalid block parameter or unsupported `useRcBlock` usage.
/// - `404 Not Found`: Pallet or constant not found in metadata.
/// - `500 Internal Server Error`: Unsupported metadata version.
/// - `503 Service Unavailable`: RPC connection lost.
pub async fn get_pallet_const_item(
    State(state): State<AppState>,
    Path((pallet_id, constant_id)): Path<(String, String)>,
    Query(params): Query<PalletItemQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_const_item_use_rc_block(state, pallet_id, constant_id, params).await;
    }

    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    let resolved_block = utils::resolve_block(&state, block_id).await?;
    let client_at_block = state.client.at(resolved_block.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: resolved_block.hash.clone(),
        height: resolved_block.number.to_string(),
    };

    let response = extract_const_item_from_metadata(
        metadata,
        &pallet_id,
        &constant_id,
        at,
        params.metadata,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Relay Chain Block Handler
// ============================================================================

/// Handle requests with `useRcBlock=true` for Asset Hub chains.
///
/// This resolves the specified relay chain block and finds the corresponding
/// Asset Hub block(s) included in it.
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
            Json(PalletsConstsResponse {
                at,
                pallet: pallet_id.to_lowercase(),
                pallet_index: "0".to_string(),
                items: ConstsItems::Full(vec![]),
                rc_block_hash: Some(rc_resolved_block.hash),
                rc_block_number: Some(rc_resolved_block.number.to_string()),
                ah_timestamp: None,
            }),
        )
            .into_response());
    }

    // Use the first Asset Hub block
    let ah_block = &ah_blocks[0];
    let client_at_block = state.client.at(ah_block.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    // Try to get the Asset Hub timestamp
    let mut ah_timestamp = None;
    if let Ok(timestamp_entry) = client_at_block.storage().entry("Timestamp", "Now")
        && let Ok(Some(timestamp)) = timestamp_entry.fetch(()).await
    {
        let timestamp_bytes = timestamp.into_bytes();
        let mut cursor = &timestamp_bytes[..];
        if let Ok(timestamp_value) = u64::decode(&mut cursor) {
            ah_timestamp = Some(timestamp_value.to_string());
        }
    }

    let rc_fields = RcBlockFields {
        rc_block_hash: Some(rc_resolved_block.hash),
        rc_block_number: Some(rc_resolved_block.number.to_string()),
        ah_timestamp,
    };

    let response =
        extract_consts_from_metadata(metadata, &pallet_id, at, params.only_ids, rc_fields)?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Handle requests with `useRcBlock=true` for Asset Hub chains (single constant item).
async fn handle_const_item_use_rc_block(
    state: AppState,
    pallet_id: String,
    constant_id: String,
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
        return Err(PalletError::ConstantNotFound(constant_id));
    }

    // Use the first Asset Hub block
    let ah_block = &ah_blocks[0];
    let client_at_block = state.client.at(ah_block.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    // Try to get the Asset Hub timestamp
    let mut ah_timestamp = None;
    if let Ok(timestamp_entry) = client_at_block.storage().entry("Timestamp", "Now")
        && let Ok(Some(timestamp)) = timestamp_entry.fetch(()).await
    {
        let timestamp_bytes = timestamp.into_bytes();
        let mut cursor = &timestamp_bytes[..];
        if let Ok(timestamp_value) = u64::decode(&mut cursor) {
            ah_timestamp = Some(timestamp_value.to_string());
        }
    }

    let rc_fields = RcBlockFields {
        rc_block_hash: Some(rc_resolved_block.hash),
        rc_block_number: Some(rc_resolved_block.number.to_string()),
        ah_timestamp,
    };

    let response = extract_const_item_from_metadata(
        metadata,
        &pallet_id,
        &constant_id,
        at,
        params.metadata,
        rc_fields,
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Metadata Extraction
// ============================================================================

/// Extract constants from runtime metadata.
///
/// Dispatches to version-specific extraction functions based on metadata version.
fn extract_consts_from_metadata(
    metadata: &RuntimeMetadata,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsConstsResponse, PalletError> {
    use RuntimeMetadata::*;
    match metadata {
        V9(meta) => extract_consts_v9(meta, pallet_id, at, only_ids, rc_fields),
        V10(meta) => extract_consts_v10(meta, pallet_id, at, only_ids, rc_fields),
        V11(meta) => extract_consts_v11(meta, pallet_id, at, only_ids, rc_fields),
        V12(meta) => extract_consts_v12(meta, pallet_id, at, only_ids, rc_fields),
        V13(meta) => extract_consts_v13(meta, pallet_id, at, only_ids, rc_fields),
        V14(meta) => extract_consts_v14(meta, pallet_id, at, only_ids, rc_fields),
        V15(meta) => extract_consts_v15(meta, pallet_id, at, only_ids, rc_fields),
        V16(meta) => extract_consts_v16(meta, pallet_id, at, only_ids, rc_fields),
        _ => Err(PalletError::UnsupportedMetadataVersion),
    }
}

/// Extract a single constant from runtime metadata.
///
/// Dispatches to version-specific extraction functions based on metadata version.
fn extract_const_item_from_metadata(
    metadata: &RuntimeMetadata,
    pallet_id: &str,
    constant_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletConstItemResponse, PalletError> {
    use RuntimeMetadata::*;
    match metadata {
        V9(meta) => extract_const_item_v9(
            meta,
            pallet_id,
            constant_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V10(meta) => extract_const_item_v10(
            meta,
            pallet_id,
            constant_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V11(meta) => extract_const_item_v11(
            meta,
            pallet_id,
            constant_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V12(meta) => extract_const_item_v12(
            meta,
            pallet_id,
            constant_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V13(meta) => extract_const_item_v13(
            meta,
            pallet_id,
            constant_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V14(meta) => extract_const_item_v14(
            meta,
            pallet_id,
            constant_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V15(meta) => extract_const_item_v15(
            meta,
            pallet_id,
            constant_id,
            at,
            include_metadata,
            rc_fields,
        ),
        V16(meta) => extract_const_item_v16(
            meta,
            pallet_id,
            constant_id,
            at,
            include_metadata,
            rc_fields,
        ),
        _ => Err(PalletError::UnsupportedMetadataVersion),
    }
}

/// Helper to extract a string from a DecodeDifferent type used in V9-V13 metadata.
fn extract_str_const<'a>(dd: &'a DecodeDifferent<&'static str, String>) -> &'a str {
    match dd {
        DecodeDifferent::Decoded(s) => s.as_str(),
        DecodeDifferent::Encode(s) => s,
    }
}

/// Helper to extract bytes from DecodeDifferent<DefaultByteGetter, Vec<u8>> used in V9-V13 constants.
fn extract_const_bytes<G>(value: &DecodeDifferent<G, Vec<u8>>) -> String {
    match value {
        DecodeDifferent::Decoded(v) => format!("0x{}", hex::encode(v)),
        DecodeDifferent::Encode(_) => "0x".to_string(),
    }
}

/// Helper to extract docs from DecodeDifferent used in V9-V13 metadata.
fn extract_const_docs(docs: &DecodeDifferent<&'static [&'static str], Vec<String>>) -> Vec<String> {
    match docs {
        DecodeDifferent::Decoded(v) => v.clone(),
        DecodeDifferent::Encode(s) => s.iter().map(|x| x.to_string()).collect(),
    }
}

/// Extracts constants from V9 metadata.
///
/// V9 metadata uses `DecodeDifferent` for module names and constant names/values.
/// The pallet index is derived from array position in V9.
fn extract_consts_v9(
    meta: &frame_metadata::v9::RuntimeMetadataV9,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsConstsResponse, PalletError> {
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
            .find(|(_, m)| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|(i, m)| (m, i as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();

    // constants in V9 is DFnA<ModuleConstantMetadata> = DecodeDifferent<FnEncode<&'static [T]>, Vec<T>>
    let DecodeDifferent::Decoded(constants) = &module.constants else {
        // Fallback for non-decoded case
        return Ok(PalletsConstsResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: module_index.to_string(),
            items: if only_ids {
                ConstsItems::OnlyIds(vec![])
            } else {
                ConstsItems::Full(vec![])
            },
            rc_block_hash: rc_fields.rc_block_hash,
            rc_block_number: rc_fields.rc_block_number,
            ah_timestamp: rc_fields.ah_timestamp,
        });
    };

    let items = if only_ids {
        ConstsItems::OnlyIds(
            constants
                .iter()
                .map(|c| extract_str_const(&c.name).to_string())
                .collect(),
        )
    } else {
        ConstsItems::Full(
            constants
                .iter()
                .map(|c| ConstItemMetadata {
                    name: extract_str_const(&c.name).to_string(),
                    ty: extract_str_const(&c.ty).to_string(),
                    value: extract_const_bytes(&c.value),
                    docs: extract_const_docs(&c.documentation),
                    deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
                })
                .collect(),
        )
    };

    Ok(PalletsConstsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extracts constants from V10 metadata.
///
/// V10 is structurally similar to V9 for constants.
/// The pallet index is derived from array position.
fn extract_consts_v10(
    meta: &frame_metadata::v10::RuntimeMetadataV10,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsConstsResponse, PalletError> {
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
            .find(|(_, m)| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|(i, m)| (m, i as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();

    let DecodeDifferent::Decoded(constants) = &module.constants else {
        return Ok(PalletsConstsResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: module_index.to_string(),
            items: if only_ids {
                ConstsItems::OnlyIds(vec![])
            } else {
                ConstsItems::Full(vec![])
            },
            rc_block_hash: rc_fields.rc_block_hash,
            rc_block_number: rc_fields.rc_block_number,
            ah_timestamp: rc_fields.ah_timestamp,
        });
    };

    let items = if only_ids {
        ConstsItems::OnlyIds(
            constants
                .iter()
                .map(|c| extract_str_const(&c.name).to_string())
                .collect(),
        )
    } else {
        ConstsItems::Full(
            constants
                .iter()
                .map(|c| ConstItemMetadata {
                    name: extract_str_const(&c.name).to_string(),
                    ty: extract_str_const(&c.ty).to_string(),
                    value: extract_const_bytes(&c.value),
                    docs: extract_const_docs(&c.documentation),
                    deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
                })
                .collect(),
        )
    };

    Ok(PalletsConstsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extracts constants from V11 metadata.
///
/// V11 is structurally similar to V9/V10 for constants.
/// The pallet index is derived from array position.
fn extract_consts_v11(
    meta: &frame_metadata::v11::RuntimeMetadataV11,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsConstsResponse, PalletError> {
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
            .find(|(_, m)| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|(i, m)| (m, i as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();

    let DecodeDifferent::Decoded(constants) = &module.constants else {
        return Ok(PalletsConstsResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: module_index.to_string(),
            items: if only_ids {
                ConstsItems::OnlyIds(vec![])
            } else {
                ConstsItems::Full(vec![])
            },
            rc_block_hash: rc_fields.rc_block_hash,
            rc_block_number: rc_fields.rc_block_number,
            ah_timestamp: rc_fields.ah_timestamp,
        });
    };

    let items = if only_ids {
        ConstsItems::OnlyIds(
            constants
                .iter()
                .map(|c| extract_str_const(&c.name).to_string())
                .collect(),
        )
    } else {
        ConstsItems::Full(
            constants
                .iter()
                .map(|c| ConstItemMetadata {
                    name: extract_str_const(&c.name).to_string(),
                    ty: extract_str_const(&c.ty).to_string(),
                    value: extract_const_bytes(&c.value),
                    docs: extract_const_docs(&c.documentation),
                    deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
                })
                .collect(),
        )
    };

    Ok(PalletsConstsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extracts constants from V12 metadata.
///
/// V12 introduces an explicit `index` field on modules.
/// Constants use `ModuleConstantMetadata` with explicit index.
fn extract_consts_v12(
    meta: &frame_metadata::v12::RuntimeMetadataV12,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsConstsResponse, PalletError> {
    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    // V12 has explicit .index field on modules
    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules
            .iter()
            .find(|m| m.index == idx)
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .find(|m| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();

    let DecodeDifferent::Decoded(constants) = &module.constants else {
        return Ok(PalletsConstsResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: module_index.to_string(),
            items: if only_ids {
                ConstsItems::OnlyIds(vec![])
            } else {
                ConstsItems::Full(vec![])
            },
            rc_block_hash: rc_fields.rc_block_hash,
            rc_block_number: rc_fields.rc_block_number,
            ah_timestamp: rc_fields.ah_timestamp,
        });
    };

    let items = if only_ids {
        ConstsItems::OnlyIds(
            constants
                .iter()
                .map(|c| extract_str_const(&c.name).to_string())
                .collect(),
        )
    } else {
        ConstsItems::Full(
            constants
                .iter()
                .map(|c| ConstItemMetadata {
                    name: extract_str_const(&c.name).to_string(),
                    ty: extract_str_const(&c.ty).to_string(),
                    value: extract_const_bytes(&c.value),
                    docs: extract_const_docs(&c.documentation),
                    deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
                })
                .collect(),
        )
    };

    Ok(PalletsConstsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extracts constants from V13 metadata.
///
/// V13 is structurally similar to V12, with explicit pallet index.
fn extract_consts_v13(
    meta: &frame_metadata::v13::RuntimeMetadataV13,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsConstsResponse, PalletError> {
    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    // V13 has explicit .index field on modules (same as V12)
    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules
            .iter()
            .find(|m| m.index == idx)
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .find(|m| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();

    let DecodeDifferent::Decoded(constants) = &module.constants else {
        return Ok(PalletsConstsResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: module_index.to_string(),
            items: if only_ids {
                ConstsItems::OnlyIds(vec![])
            } else {
                ConstsItems::Full(vec![])
            },
            rc_block_hash: rc_fields.rc_block_hash,
            rc_block_number: rc_fields.rc_block_number,
            ah_timestamp: rc_fields.ah_timestamp,
        });
    };

    let items = if only_ids {
        ConstsItems::OnlyIds(
            constants
                .iter()
                .map(|c| extract_str_const(&c.name).to_string())
                .collect(),
        )
    } else {
        ConstsItems::Full(
            constants
                .iter()
                .map(|c| ConstItemMetadata {
                    name: extract_str_const(&c.name).to_string(),
                    ty: extract_str_const(&c.ty).to_string(),
                    value: extract_const_bytes(&c.value),
                    docs: extract_const_docs(&c.documentation),
                    deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
                })
                .collect(),
        )
    };

    Ok(PalletsConstsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_consts_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsConstsResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v14(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let items = if only_ids {
        ConstsItems::OnlyIds(pallet.constants.iter().map(|c| c.name.clone()).collect())
    } else {
        ConstsItems::Full(
            pallet
                .constants
                .iter()
                .map(|c| ConstItemMetadata {
                    name: c.name.clone(),
                    ty: c.ty.id.to_string(),
                    value: format!("0x{}", hex::encode(&c.value)),
                    docs: c.docs.clone(),
                    // V14 doesn't have deprecation info, but we include it for consistency
                    deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
                })
                .collect(),
        )
    };

    Ok(PalletsConstsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract constants from V15 metadata.
fn extract_consts_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsConstsResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v15(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let items = if only_ids {
        ConstsItems::OnlyIds(pallet.constants.iter().map(|c| c.name.clone()).collect())
    } else {
        ConstsItems::Full(
            pallet
                .constants
                .iter()
                .map(|c| ConstItemMetadata {
                    name: c.name.clone(),
                    ty: c.ty.id.to_string(),
                    value: format!("0x{}", hex::encode(&c.value)),
                    docs: c.docs.clone(),
                    // V15 has deprecation info but currently we default to not deprecated
                    // TODO: Extract actual deprecation info from V15 metadata when available
                    deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
                })
                .collect(),
        )
    };

    Ok(PalletsConstsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract constants from V16 metadata.
fn extract_consts_v16(
    meta: &frame_metadata::v16::RuntimeMetadataV16,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsConstsResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v16(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let items = if only_ids {
        ConstsItems::OnlyIds(pallet.constants.iter().map(|c| c.name.clone()).collect())
    } else {
        ConstsItems::Full(
            pallet
                .constants
                .iter()
                .map(|c| ConstItemMetadata {
                    name: c.name.clone(),
                    ty: c.ty.id.to_string(),
                    value: format!("0x{}", hex::encode(&c.value)),
                    docs: c.docs.clone(),
                    deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
                })
                .collect(),
        )
    };

    Ok(PalletsConstsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// Single Constant Extraction Functions
// ============================================================================

/// Convert a constant name to camelCase for response field.
fn to_camel_case(name: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for (i, c) in name.chars().enumerate() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else if i == 0 {
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Extract a single constant from V9 metadata.
fn extract_const_item_v9(
    meta: &frame_metadata::v9::RuntimeMetadataV9,
    pallet_id: &str,
    constant_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletConstItemResponse, PalletError> {
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
            .find(|(_, m)| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|(i, m)| (m, i as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();
    let constant_id_lower = constant_id.to_lowercase();

    let DecodeDifferent::Decoded(constants) = &module.constants else {
        return Err(PalletError::ConstantNotFound(constant_id.to_string()));
    };

    let constant = constants
        .iter()
        .find(|c| extract_str_const(&c.name).to_lowercase() == constant_id_lower)
        .ok_or_else(|| PalletError::ConstantNotFound(constant_id.to_string()))?;

    let const_name = extract_str_const(&constant.name).to_string();
    let constants_item = to_camel_case(&const_name);

    let metadata = if include_metadata {
        Some(ConstItemMetadata {
            name: const_name,
            ty: extract_str_const(&constant.ty).to_string(),
            value: extract_const_bytes(&constant.value),
            docs: extract_const_docs(&constant.documentation),
            deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
        })
    } else {
        None
    };

    Ok(PalletConstItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        constants_item,
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract a single constant from V10 metadata.
fn extract_const_item_v10(
    meta: &frame_metadata::v10::RuntimeMetadataV10,
    pallet_id: &str,
    constant_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletConstItemResponse, PalletError> {
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
            .find(|(_, m)| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|(i, m)| (m, i as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();
    let constant_id_lower = constant_id.to_lowercase();

    let DecodeDifferent::Decoded(constants) = &module.constants else {
        return Err(PalletError::ConstantNotFound(constant_id.to_string()));
    };

    let constant = constants
        .iter()
        .find(|c| extract_str_const(&c.name).to_lowercase() == constant_id_lower)
        .ok_or_else(|| PalletError::ConstantNotFound(constant_id.to_string()))?;

    let const_name = extract_str_const(&constant.name).to_string();
    let constants_item = to_camel_case(&const_name);

    let metadata = if include_metadata {
        Some(ConstItemMetadata {
            name: const_name,
            ty: extract_str_const(&constant.ty).to_string(),
            value: extract_const_bytes(&constant.value),
            docs: extract_const_docs(&constant.documentation),
            deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
        })
    } else {
        None
    };

    Ok(PalletConstItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        constants_item,
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract a single constant from V11 metadata.
fn extract_const_item_v11(
    meta: &frame_metadata::v11::RuntimeMetadataV11,
    pallet_id: &str,
    constant_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletConstItemResponse, PalletError> {
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
            .find(|(_, m)| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|(i, m)| (m, i as u8))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();
    let constant_id_lower = constant_id.to_lowercase();

    let DecodeDifferent::Decoded(constants) = &module.constants else {
        return Err(PalletError::ConstantNotFound(constant_id.to_string()));
    };

    let constant = constants
        .iter()
        .find(|c| extract_str_const(&c.name).to_lowercase() == constant_id_lower)
        .ok_or_else(|| PalletError::ConstantNotFound(constant_id.to_string()))?;

    let const_name = extract_str_const(&constant.name).to_string();
    let constants_item = to_camel_case(&const_name);

    let metadata = if include_metadata {
        Some(ConstItemMetadata {
            name: const_name,
            ty: extract_str_const(&constant.ty).to_string(),
            value: extract_const_bytes(&constant.value),
            docs: extract_const_docs(&constant.documentation),
            deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
        })
    } else {
        None
    };

    Ok(PalletConstItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        constants_item,
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract a single constant from V12 metadata.
fn extract_const_item_v12(
    meta: &frame_metadata::v12::RuntimeMetadataV12,
    pallet_id: &str,
    constant_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletConstItemResponse, PalletError> {
    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules
            .iter()
            .find(|m| m.index == idx)
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .find(|m| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();
    let constant_id_lower = constant_id.to_lowercase();

    let DecodeDifferent::Decoded(constants) = &module.constants else {
        return Err(PalletError::ConstantNotFound(constant_id.to_string()));
    };

    let constant = constants
        .iter()
        .find(|c| extract_str_const(&c.name).to_lowercase() == constant_id_lower)
        .ok_or_else(|| PalletError::ConstantNotFound(constant_id.to_string()))?;

    let const_name = extract_str_const(&constant.name).to_string();
    let constants_item = to_camel_case(&const_name);

    let metadata = if include_metadata {
        Some(ConstItemMetadata {
            name: const_name,
            ty: extract_str_const(&constant.ty).to_string(),
            value: extract_const_bytes(&constant.value),
            docs: extract_const_docs(&constant.documentation),
            deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
        })
    } else {
        None
    };

    Ok(PalletConstItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        constants_item,
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract a single constant from V13 metadata.
fn extract_const_item_v13(
    meta: &frame_metadata::v13::RuntimeMetadataV13,
    pallet_id: &str,
    constant_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletConstItemResponse, PalletError> {
    let DecodeDifferent::Decoded(modules) = &meta.modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let (module, module_index) = if let Ok(idx) = pallet_id.parse::<u8>() {
        modules
            .iter()
            .find(|m| m.index == idx)
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    } else {
        modules
            .iter()
            .find(|m| extract_str_const(&m.name).eq_ignore_ascii_case(pallet_id))
            .map(|m| (m, m.index))
            .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?
    };

    let pallet_name = extract_str_const(&module.name).to_string();
    let constant_id_lower = constant_id.to_lowercase();

    let DecodeDifferent::Decoded(constants) = &module.constants else {
        return Err(PalletError::ConstantNotFound(constant_id.to_string()));
    };

    let constant = constants
        .iter()
        .find(|c| extract_str_const(&c.name).to_lowercase() == constant_id_lower)
        .ok_or_else(|| PalletError::ConstantNotFound(constant_id.to_string()))?;

    let const_name = extract_str_const(&constant.name).to_string();
    let constants_item = to_camel_case(&const_name);

    let metadata = if include_metadata {
        Some(ConstItemMetadata {
            name: const_name,
            ty: extract_str_const(&constant.ty).to_string(),
            value: extract_const_bytes(&constant.value),
            docs: extract_const_docs(&constant.documentation),
            deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
        })
    } else {
        None
    };

    Ok(PalletConstItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: module_index.to_string(),
        constants_item,
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract a single constant from V14 metadata.
fn extract_const_item_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    constant_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletConstItemResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v14(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let constant_id_lower = constant_id.to_lowercase();
    let constant = pallet
        .constants
        .iter()
        .find(|c| c.name.to_lowercase() == constant_id_lower)
        .ok_or_else(|| PalletError::ConstantNotFound(constant_id.to_string()))?;

    let constants_item = to_camel_case(&constant.name);

    let metadata = if include_metadata {
        Some(ConstItemMetadata {
            name: constant.name.clone(),
            ty: constant.ty.id.to_string(),
            value: format!("0x{}", hex::encode(&constant.value)),
            docs: constant.docs.clone(),
            deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
        })
    } else {
        None
    };

    Ok(PalletConstItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        constants_item,
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract a single constant from V15 metadata.
fn extract_const_item_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    constant_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletConstItemResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v15(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let constant_id_lower = constant_id.to_lowercase();
    let constant = pallet
        .constants
        .iter()
        .find(|c| c.name.to_lowercase() == constant_id_lower)
        .ok_or_else(|| PalletError::ConstantNotFound(constant_id.to_string()))?;

    let constants_item = to_camel_case(&constant.name);

    let metadata = if include_metadata {
        Some(ConstItemMetadata {
            name: constant.name.clone(),
            ty: constant.ty.id.to_string(),
            value: format!("0x{}", hex::encode(&constant.value)),
            docs: constant.docs.clone(),
            deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
        })
    } else {
        None
    };

    Ok(PalletConstItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        constants_item,
        metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract a single constant from V16 metadata.
fn extract_const_item_v16(
    meta: &frame_metadata::v16::RuntimeMetadataV16,
    pallet_id: &str,
    constant_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletConstItemResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v16(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let constant_id_lower = constant_id.to_lowercase();
    let constant = pallet
        .constants
        .iter()
        .find(|c| c.name.to_lowercase() == constant_id_lower)
        .ok_or_else(|| PalletError::ConstantNotFound(constant_id.to_string()))?;

    let constants_item = to_camel_case(&constant.name);

    let metadata = if include_metadata {
        Some(ConstItemMetadata {
            name: constant.name.clone(),
            ty: constant.ty.id.to_string(),
            value: format!("0x{}", hex::encode(&constant.value)),
            docs: constant.docs.clone(),
            deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
        })
    } else {
        None
    };

    Ok(PalletConstItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        constants_item,
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
    fn test_consts_items_serialization_full() {
        let items = ConstsItems::Full(vec![ConstItemMetadata {
            name: "TestConst".to_string(),
            ty: "1".to_string(),
            value: "0x01020304".to_string(),
            docs: vec!["Test documentation".to_string()],
            deprecation_info: Some(DeprecationInfo::NotDeprecated(None)),
        }]);

        let json = serde_json::to_string(&items).unwrap();
        assert!(json.contains("TestConst"));
        assert!(json.contains("0x01020304"));
    }

    #[test]
    fn test_consts_items_serialization_only_ids() {
        let items = ConstsItems::OnlyIds(vec!["Const1".to_string(), "Const2".to_string()]);

        let json = serde_json::to_string(&items).unwrap();
        assert!(json.contains("Const1"));
        assert!(json.contains("Const2"));
        // Should be a simple array, not objects
        assert!(!json.contains("name"));
    }

    #[test]
    fn test_response_serialization() {
        let response = PalletsConstsResponse {
            at: AtResponse {
                hash: "0xabc".to_string(),
                height: "100".to_string(),
            },
            pallet: "system".to_string(),
            pallet_index: "0".to_string(),
            items: ConstsItems::OnlyIds(vec!["BlockLength".to_string()]),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"pallet\":\"system\""));
        assert!(json.contains("\"palletIndex\":\"0\""));
        // rc_block fields should not appear when None
        assert!(!json.contains("rcBlockHash"));
    }

    #[test]
    fn test_response_with_rc_block_fields() {
        let response = PalletsConstsResponse {
            at: AtResponse {
                hash: "0xabc".to_string(),
                height: "100".to_string(),
            },
            pallet: "system".to_string(),
            pallet_index: "0".to_string(),
            items: ConstsItems::OnlyIds(vec![]),
            rc_block_hash: Some("0xdef".to_string()),
            rc_block_number: Some("200".to_string()),
            ah_timestamp: Some("1234567890".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"rcBlockHash\":\"0xdef\""));
        assert!(json.contains("\"rcBlockNumber\":\"200\""));
        assert!(json.contains("\"ahTimestamp\":\"1234567890\""));
    }
}
