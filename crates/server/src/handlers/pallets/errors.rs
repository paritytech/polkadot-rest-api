// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for the `/pallets/{palletId}/errors` endpoint.
//!
//! This endpoint returns the errors defined in a pallet's metadata.
//! It supports querying at specific blocks and relay chain block resolution
//! for Asset Hub chains.
//!
//! # Sidecar Compatibility
//!
//! This endpoint aims to match the Sidecar `/pallets/{palletId}/errors` response format.
//!
//! # Implementation Notes
//!
//! This handler uses subxt's unified Metadata API which automatically normalizes
//! all metadata versions (V9-V16) into a single format. This eliminates the need
//! for version-specific extraction logic.

// Allow large error types - PalletError contains subxt::error::OnlineClientAtBlockError
// which is large by design. Boxing would add indirection without significant benefit.
#![allow(clippy::result_large_err)]

use crate::extractors::JsonQuery;
use crate::handlers::pallets::common::{
    AtResponse, PalletError, PalletItemQueryParams, PalletQueryParams, RcBlockFields,
    RcPalletItemQueryParams, RcPalletQueryParams, resolve_block_for_pallet, resolve_type_name,
};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use heck::ToLowerCamelCase;
use polkadot_rest_api_config::ChainType;
use scale_info::form::PortableForm;
use serde::Serialize;
use subxt::Metadata;

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

#[utoipa::path(
    get,
    path = "/v1/pallets/{palletId}/errors",
    tag = "pallets",
    summary = "Pallet errors",
    description = "Returns all errors defined in a pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("onlyIds" = Option<bool>, description = "Only return error names"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Pallet errors", body = Object),
        (status = 400, description = "Invalid pallet"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_pallet_errors(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    JsonQuery(params): JsonQuery<PalletQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, pallet_id, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    // Use subxt's metadata API - it normalizes all versions (V9-V16) automatically
    let metadata = resolved.client_at_block.metadata();

    let response = extract_errors_from_metadata(
        &metadata,
        &pallet_id,
        resolved.at,
        params.only_ids,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

#[utoipa::path(
    get,
    path = "/v1/pallets/{palletId}/errors/{errorItemId}",
    tag = "pallets",
    summary = "Pallet error details",
    description = "Returns metadata for a specific error in a pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("errorItemId" = String, Path, description = "Name of the error"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("metadata" = Option<bool>, description = "Include metadata"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Error details", body = Object),
        (status = 404, description = "Error not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_pallet_error_item(
    State(state): State<AppState>,
    Path((pallet_id, error_id)): Path<(String, String)>,
    JsonQuery(params): JsonQuery<PalletItemQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_error_item_use_rc_block(state, pallet_id, error_id, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    // Use subxt's metadata API - it normalizes all versions (V9-V16) automatically
    let metadata = resolved.client_at_block.metadata();

    let response = extract_error_item_from_metadata(
        &metadata,
        &pallet_id,
        &error_id,
        resolved.at,
        params.metadata,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Relay Chain Block Handlers
// ============================================================================

async fn handle_use_rc_block(
    state: AppState,
    pallet_id: String,
    params: PalletQueryParams,
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
        return Ok((StatusCode::OK, Json(Vec::<PalletsErrorsResponse>::new())).into_response());
    }

    let mut results = Vec::new();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let rc_block_number = rc_resolved_block.number.to_string();

    for ah_block in &ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = utils::fetch_block_timestamp(&client_at_block).await;

        let rc_fields = RcBlockFields {
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        };

        let metadata = client_at_block.metadata();

        let response =
            extract_errors_from_metadata(&metadata, &pallet_id, at, params.only_ids, rc_fields)?;

        results.push(response);
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

async fn handle_error_item_use_rc_block(
    state: AppState,
    pallet_id: String,
    error_id: String,
    params: PalletItemQueryParams,
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
        return Ok((StatusCode::OK, Json(Vec::<PalletErrorItemResponse>::new())).into_response());
    }

    let mut results = Vec::new();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let rc_block_number = rc_resolved_block.number.to_string();

    for ah_block in &ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = utils::fetch_block_timestamp(&client_at_block).await;

        let rc_fields = RcBlockFields {
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        };

        let metadata = client_at_block.metadata();

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
// Metadata Extraction (Unified for all metadata versions)
// ============================================================================

/// Find a pallet by name (case-insensitive) or index using subxt's metadata API.
///
/// Note: For modern metadata (V12+), call_index == event_index == error_index.
/// For older metadata, they may differ. We use error_index since this is
/// the errors endpoint.
fn find_pallet<'a>(
    metadata: &'a Metadata,
    pallet_id: &str,
) -> Option<subxt_metadata::PalletMetadata<'a>> {
    // First, try to parse as a numeric index
    if let Ok(index) = pallet_id.parse::<u8>() {
        // Use error_index since this is the errors endpoint
        return metadata
            .pallets()
            .find(|pallet| pallet.error_index() == index);
    }

    // Otherwise, search by name (case-insensitive)
    let pallet_id_lower = pallet_id.to_lowercase();
    metadata
        .pallets()
        .find(|pallet| pallet.name().to_lowercase() == pallet_id_lower)
}

/// Extract errors from subxt's unified Metadata.
fn extract_errors_from_metadata(
    metadata: &Metadata,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
    let pallet = find_pallet(metadata, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet_name = pallet.name().to_string();
    let pallet_index = pallet.error_index();

    let error_variants = pallet.error_variants();

    let items = match error_variants {
        Some(variants) => {
            if only_ids {
                ErrorsItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
            } else {
                ErrorsItems::Full(
                    variants
                        .iter()
                        .map(|v| variant_to_error_metadata(v, metadata.types()))
                        .collect(),
                )
            }
        }
        None => {
            if only_ids {
                ErrorsItems::OnlyIds(vec![])
            } else {
                ErrorsItems::Full(vec![])
            }
        }
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

/// Extract a single error from subxt's unified Metadata.
fn extract_error_item_from_metadata(
    metadata: &Metadata,
    pallet_id: &str,
    error_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletErrorItemResponse, PalletError> {
    let pallet = find_pallet(metadata, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet_name = pallet.name().to_string();
    let pallet_index = pallet.error_index();

    let error_variants = pallet
        .error_variants()
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_id_lower = error_id.to_lowercase();
    let error_variant = error_variants
        .iter()
        .find(|v| v.name.to_lowercase() == error_id_lower)
        .ok_or_else(|| PalletError::ErrorItemNotFound(error_id.to_string()))?;

    let error_name = error_variant.name.clone();

    let error_metadata = if include_metadata {
        Some(variant_to_error_metadata(error_variant, metadata.types()))
    } else {
        None
    };

    Ok(PalletErrorItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        error_item: error_name.to_lower_camel_case(),
        metadata: error_metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Convert a scale_info Variant to our ErrorItemMetadata format.
fn variant_to_error_metadata(
    variant: &scale_info::Variant<PortableForm>,
    types: &scale_info::PortableRegistry,
) -> ErrorItemMetadata {
    let fields: Vec<ErrorField> = variant
        .fields
        .iter()
        .map(|f| {
            let type_name = resolve_type_name(types, f.ty.id);
            ErrorField {
                name: f.name.clone().unwrap_or_default(),
                ty: f.ty.id.to_string(),
                type_name: Some(type_name.clone()),
                docs: f.docs.clone(),
            }
        })
        .collect();

    let args: Vec<ErrorArg> = variant
        .fields
        .iter()
        .map(|f| {
            let type_name = resolve_type_name(types, f.ty.id);
            let simplified_type_name = simplify_type_name(&type_name);
            ErrorArg {
                name: f.name.clone().unwrap_or_default().to_lower_camel_case(),
                ty: type_name.clone(),
                type_name: simplified_type_name,
            }
        })
        .collect();

    ErrorItemMetadata {
        name: variant.name.clone(),
        fields,
        index: variant.index.to_string(),
        docs: variant.docs.clone(),
        args,
    }
}

/// Simplify a type name by removing generics (for Sidecar compatibility).
fn simplify_type_name(type_name: &str) -> String {
    type_name
        .split('<')
        .next()
        .unwrap_or(type_name)
        .split("::")
        .last()
        .unwrap_or(type_name)
        .to_string()
}

// ============================================================================
// RC (Relay Chain) Handlers
// ============================================================================

/// Handler for GET `/rc/pallets/{palletId}/errors`
///
/// Returns errors from the relay chain's pallet metadata.
#[utoipa::path(
    get,
    path = "/v1/rc/pallets/{palletId}/errors",
    tag = "rc",
    summary = "RC pallet errors",
    description = "Returns all errors defined in a relay chain pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("onlyIds" = Option<bool>, description = "Only return error names")
    ),
    responses(
        (status = 200, description = "Relay chain pallet errors", body = Object),
        (status = 400, description = "Invalid pallet"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallet_errors(
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
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(&relay_rpc_client, &relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let response = extract_errors_from_metadata(
        &metadata,
        &pallet_id,
        at,
        params.only_ids,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Handler for GET `/rc/pallets/{palletId}/errors/{errorItemId}`
///
/// Returns a specific error from the relay chain's pallet metadata.
#[utoipa::path(
    get,
    path = "/v1/rc/pallets/{palletId}/errors/{errorItemId}",
    tag = "rc",
    summary = "RC pallet error details",
    description = "Returns metadata for a specific error in a relay chain pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("errorItemId" = String, Path, description = "Name of the error"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("metadata" = Option<bool>, description = "Include metadata")
    ),
    responses(
        (status = 200, description = "Relay chain error details", body = Object),
        (status = 404, description = "Error not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallet_error_item(
    State(state): State<AppState>,
    Path((pallet_id, error_id)): Path<(String, String)>,
    JsonQuery(params): JsonQuery<RcPalletItemQueryParams>,
) -> Result<Response, PalletError> {
    let relay_client = state.get_relay_chain_client().await?;
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_rpc = state.get_relay_chain_rpc().await?;

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(&relay_rpc_client, &relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

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

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simplify_type_name() {
        assert_eq!(simplify_type_name("Vec<u8>"), "Vec");
        assert_eq!(simplify_type_name("BoundedVec<u8, MaxLen>"), "BoundedVec");
        assert_eq!(simplify_type_name("sp_runtime::AccountId32"), "AccountId32");
        assert_eq!(simplify_type_name("u128"), "u128");
    }

    #[test]
    fn test_error_item_metadata_serialization() {
        let metadata = ErrorItemMetadata {
            name: "InsufficientBalance".to_string(),
            fields: vec![],
            index: "0".to_string(),
            docs: vec!["The account does not have enough balance.".to_string()],
            args: vec![],
        };

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"name\":\"InsufficientBalance\""));
        assert!(json.contains("\"index\":\"0\""));
    }

    #[test]
    fn test_errors_items_only_ids_serialization() {
        let items = ErrorsItems::OnlyIds(vec!["Error1".to_string(), "Error2".to_string()]);

        let json = serde_json::to_string(&items).unwrap();
        assert_eq!(json, r#"["Error1","Error2"]"#);
    }

    #[test]
    fn test_pallet_errors_response_serialization() {
        let response = PalletsErrorsResponse {
            at: AtResponse {
                hash: "0x123".to_string(),
                height: "100".to_string(),
            },
            pallet: "system".to_string(),
            pallet_index: "0".to_string(),
            items: ErrorsItems::OnlyIds(vec!["InvalidSpecName".to_string()]),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"pallet\":\"system\""));
        assert!(json.contains("\"palletIndex\":\"0\""));
    }

    #[test]
    fn test_error_field_serialization() {
        let field = ErrorField {
            name: "amount".to_string(),
            ty: "6".to_string(),
            type_name: Some("u128".to_string()),
            docs: vec![],
        };

        let json = serde_json::to_string(&field).unwrap();
        assert!(json.contains("\"name\":\"amount\""));
        assert!(json.contains("\"type\":\"6\""));
        assert!(json.contains("\"typeName\":\"u128\""));
    }

    #[test]
    fn test_error_arg_serialization() {
        let arg = ErrorArg {
            name: "accountId".to_string(),
            ty: "sp_runtime::AccountId32".to_string(),
            type_name: "AccountId32".to_string(),
        };

        let json = serde_json::to_string(&arg).unwrap();
        assert!(json.contains("\"name\":\"accountId\""));
        assert!(json.contains("\"type\":\"sp_runtime::AccountId32\""));
        assert!(json.contains("\"typeName\":\"AccountId32\""));
    }
}
