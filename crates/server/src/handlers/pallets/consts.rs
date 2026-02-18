// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for /pallets/{palletId}/consts endpoints.
//!
//! Provides access to runtime constants defined in pallets.
//! Uses Subxt's metadata API which normalizes all metadata versions internally.

#![allow(clippy::result_large_err)]

use crate::handlers::pallets::common::{
    AtResponse, PalletError, RcPalletItemQueryParams, RcPalletQueryParams,
};
use crate::state::AppState;
use crate::utils;
use crate::utils::format::to_camel_case;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use polkadot_rest_api_config::ChainType;
use serde::{Deserialize, Serialize};
use subxt::Metadata;
// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstantsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub only_ids: bool,
    #[serde(default)]
    pub use_rc_block: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstantItemQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub metadata: bool,
    #[serde(default)]
    pub use_rc_block: bool,
}

/// Deprecation info for a constant
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeprecationInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_deprecated: Option<()>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
}

impl DeprecationInfo {
    pub fn not_deprecated() -> Self {
        Self {
            not_deprecated: Some(()),
            deprecated: None,
        }
    }
}

/// Metadata for a single constant
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstantItemMetadata {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub value: String,
    pub docs: Vec<String>,
    pub deprecation_info: DeprecationInfo,
}

/// Items can be either full metadata or just names
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ConstantsItems {
    Full(Vec<ConstantItemMetadata>),
    OnlyIds(Vec<String>),
}

/// Response for /pallets/{palletId}/consts
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletConstantsResponse {
    pub at: AtResponse,
    pub pallet: String,
    pub pallet_index: String,
    pub items: ConstantsItems,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Response for /pallets/{palletId}/consts/{constantItemId}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletConstantItemResponse {
    pub at: AtResponse,
    pub pallet: String,
    pub pallet_index: String,
    #[serde(rename = "constantsItem")]
    pub constants_item: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ConstantItemMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// ============================================================================
// Internal Types
// ============================================================================

struct PalletConstantsInfo {
    name: String,
    index: u8,
    constants: Vec<ConstantItemMetadata>,
}

// ============================================================================
// Main Handlers
// ============================================================================

#[utoipa::path(
    get,
    path = "/v1/pallets/{palletId}/consts",
    tag = "pallets",
    summary = "Pallet constants",
    description = "Returns all constants defined in a pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("onlyIds" = Option<bool>, Query, description = "Only return constant names"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Pallet constants", body = Object),
        (status = 400, description = "Invalid pallet"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_constants(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<ConstantsQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_constants_use_rc_block(state, pallet_id, params).await;
    }

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block(&state, block_id).await?;

    // Get client at block - Subxt normalizes all metadata versions
    let client_at_block = state.client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_constants(&metadata, &pallet_id)?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let items = if params.only_ids {
        ConstantsItems::OnlyIds(
            pallet_info
                .constants
                .iter()
                .map(|c| c.name.clone())
                .collect(),
        )
    } else {
        ConstantsItems::Full(pallet_info.constants)
    };

    Ok((
        StatusCode::OK,
        Json(PalletConstantsResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            items,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/v1/pallets/{palletId}/consts/{constantItemId}",
    tag = "pallets",
    summary = "Pallet constant value",
    description = "Returns the value and metadata of a specific constant in a pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("constantItemId" = String, Path, description = "Name of the constant"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("metadata" = Option<bool>, Query, description = "Include metadata"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Constant value", body = Object),
        (status = 400, description = "Invalid parameters"),
        (status = 404, description = "Constant not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_constant_item(
    State(state): State<AppState>,
    Path((pallet_id, constant_item_id)): Path<(String, String)>,
    Query(params): Query<ConstantItemQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_constant_item_use_rc_block(state, pallet_id, constant_item_id, params).await;
    }

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block(&state, block_id).await?;

    // Get client at block - Subxt normalizes all metadata versions
    let client_at_block = state.client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_constants(&metadata, &pallet_id)?;

    let constant = pallet_info
        .constants
        .iter()
        .find(|c| c.name.to_lowercase() == constant_item_id.to_lowercase())
        .ok_or_else(|| PalletError::ConstantItemNotFound {
            pallet: pallet_info.name.clone(),
            item: constant_item_id.clone(),
        })?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let metadata_field = if params.metadata {
        Some(constant.clone())
    } else {
        None
    };

    Ok((
        StatusCode::OK,
        Json(PalletConstantItemResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            constants_item: to_camel_case(&constant_item_id),
            metadata: metadata_field,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

// ============================================================================
// RC Block Handlers
// ============================================================================

async fn handle_constants_use_rc_block(
    state: AppState,
    pallet_id: String,
    params: ConstantsQueryParams,
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

    // Return empty array when no AH blocks found
    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(Vec::<PalletConstantsResponse>::new())).into_response());
    }

    // Process ALL AH blocks and return array of responses
    let mut responses = Vec::new();
    for ah_block in &ah_blocks {
        // Get client at block for this AH block
        let client_at_block = state.client.at_block(ah_block.number).await?;
        let metadata = client_at_block.metadata();

        let pallet_info = extract_pallet_constants(&metadata, &pallet_id)?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = utils::fetch_block_timestamp(&client_at_block).await;

        let items = if params.only_ids {
            ConstantsItems::OnlyIds(
                pallet_info
                    .constants
                    .iter()
                    .map(|c| c.name.clone())
                    .collect(),
            )
        } else {
            ConstantsItems::Full(pallet_info.constants)
        };

        responses.push(PalletConstantsResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            items,
            rc_block_hash: Some(rc_resolved_block.hash.clone()),
            rc_block_number: Some(rc_resolved_block.number.to_string()),
            ah_timestamp,
        });
    }

    Ok((StatusCode::OK, Json(responses)).into_response())
}

async fn handle_constant_item_use_rc_block(
    state: AppState,
    pallet_id: String,
    constant_item_id: String,
    params: ConstantItemQueryParams,
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

    // Return empty array when no AH blocks found
    if ah_blocks.is_empty() {
        return Ok((
            StatusCode::OK,
            Json(Vec::<PalletConstantItemResponse>::new()),
        )
            .into_response());
    }

    // Process ALL AH blocks in the RC block and return array
    let mut responses = Vec::new();
    for ah_block in &ah_blocks {
        // Get client at block for this AH block
        let client_at_block = state.client.at_block(ah_block.number).await?;
        let metadata = client_at_block.metadata();

        let pallet_info = extract_pallet_constants(&metadata, &pallet_id)?;

        let constant = pallet_info
            .constants
            .iter()
            .find(|c| c.name.to_lowercase() == constant_item_id.to_lowercase())
            .ok_or_else(|| PalletError::ConstantItemNotFound {
                pallet: pallet_info.name.clone(),
                item: constant_item_id.clone(),
            })?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = utils::fetch_block_timestamp(&client_at_block).await;

        let metadata_field = if params.metadata {
            Some(constant.clone())
        } else {
            None
        };

        responses.push(PalletConstantItemResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            constants_item: to_camel_case(&constant_item_id),
            metadata: metadata_field,
            rc_block_hash: Some(rc_resolved_block.hash.clone()),
            rc_block_number: Some(rc_resolved_block.number.to_string()),
            ah_timestamp,
        });
    }

    Ok((StatusCode::OK, Json(responses)).into_response())
}

// ============================================================================
// Metadata Extraction - Using Subxt's normalized metadata API
// ============================================================================

/// Extract pallet constants using Subxt's metadata API.
/// Subxt normalizes all metadata versions (V9-V15+) into a unified format.
fn extract_pallet_constants(
    metadata: &Metadata,
    pallet_id: &str,
) -> Result<PalletConstantsInfo, PalletError> {
    // Try to find pallet by index first, then by name (case-insensitive)
    let pallet = if let Ok(index) = pallet_id.parse::<u8>() {
        metadata.pallets().find(|p| p.call_index() == index)
    } else {
        // Try exact match first, then case-insensitive match
        metadata.pallet_by_name(pallet_id).or_else(|| {
            let pallet_id_lower = pallet_id.to_lowercase();
            metadata
                .pallets()
                .find(|p| p.name().to_lowercase() == pallet_id_lower)
        })
    };

    let pallet = pallet.ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let constants: Vec<ConstantItemMetadata> = pallet
        .constants()
        .map(|c| ConstantItemMetadata {
            name: c.name().to_string(),
            ty: c.ty().to_string(),
            value: format!("0x{}", hex::encode(c.value())),
            docs: c.docs().iter().map(|s| s.to_string()).collect(),
            deprecation_info: DeprecationInfo::not_deprecated(),
        })
        .collect();

    Ok(PalletConstantsInfo {
        name: pallet.name().to_string(),
        index: pallet.call_index(),
        constants,
    })
}

// ============================================================================
// RC (Relay Chain) Handlers
// ============================================================================

/// Handler for GET `/rc/pallets/{palletId}/consts`
///
/// Returns constants from the relay chain's pallet metadata.
#[utoipa::path(
    get,
    path = "/v1/rc/pallets/{palletId}/consts",
    tag = "rc",
    summary = "RC pallet constants",
    description = "Returns all constants defined in a relay chain pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("onlyIds" = Option<bool>, Query, description = "Only return constant names")
    ),
    responses(
        (status = 200, description = "Relay chain pallet constants", body = Object),
        (status = 400, description = "Invalid pallet"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallets_constants(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<RcPalletQueryParams>,
) -> Result<Response, PalletError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(PalletError::RelayChainNotConfigured)?;

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_constants(&metadata, &pallet_id)?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let items = if params.only_ids {
        ConstantsItems::OnlyIds(
            pallet_info
                .constants
                .iter()
                .map(|c| c.name.clone())
                .collect(),
        )
    } else {
        ConstantsItems::Full(pallet_info.constants)
    };

    Ok((
        StatusCode::OK,
        Json(PalletConstantsResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            items,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

/// Handler for GET `/rc/pallets/{palletId}/consts/{constantItemId}`
///
/// Returns a specific constant from the relay chain's pallet metadata.
#[utoipa::path(
    get,
    path = "/v1/rc/pallets/{palletId}/consts/{constantItemId}",
    tag = "rc",
    summary = "RC pallet constant value",
    description = "Returns the value and metadata of a specific constant from a relay chain pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("constantItemId" = String, Path, description = "Name of the constant"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("metadata" = Option<bool>, Query, description = "Include metadata")
    ),
    responses(
        (status = 200, description = "Relay chain constant value", body = Object),
        (status = 404, description = "Constant not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallets_constant_item(
    State(state): State<AppState>,
    Path((pallet_id, constant_item_id)): Path<(String, String)>,
    Query(params): Query<RcPalletItemQueryParams>,
) -> Result<Response, PalletError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(PalletError::RelayChainNotConfigured)?;

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_constants(&metadata, &pallet_id)?;

    let constant = pallet_info
        .constants
        .iter()
        .find(|c| c.name.to_lowercase() == constant_item_id.to_lowercase())
        .ok_or_else(|| PalletError::ConstantItemNotFound {
            pallet: pallet_info.name.clone(),
            item: constant_item_id.clone(),
        })?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let metadata_field = if params.metadata {
        Some(constant.clone())
    } else {
        None
    };

    Ok((
        StatusCode::OK,
        Json(PalletConstantItemResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            constants_item: to_camel_case(&constant_item_id),
            metadata: metadata_field,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}
// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("BlockWeights"), "blockWeights");
        assert_eq!(to_camel_case("SS58Prefix"), "sS58Prefix");
        assert_eq!(to_camel_case("existentialDeposit"), "existentialDeposit");
        assert_eq!(to_camel_case(""), "");
        assert_eq!(to_camel_case("A"), "a");
    }

    #[test]
    fn test_deprecation_info_not_deprecated() {
        let info = DeprecationInfo::not_deprecated();
        assert!(info.not_deprecated.is_some());
        assert!(info.deprecated.is_none());
    }

    #[test]
    fn test_constants_query_params_defaults() {
        let json = r#"{"at": "123"}"#;
        let params: ConstantsQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("123".to_string()));
        assert!(!params.only_ids);
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_constant_item_query_params_defaults() {
        let json = r#"{"at": "456"}"#;
        let params: ConstantItemQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("456".to_string()));
        assert!(!params.metadata);
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_constant_item_metadata_serialization() {
        let metadata = ConstantItemMetadata {
            name: "BlockWeights".to_string(),
            ty: "123".to_string(),
            value: "0x1234".to_string(),
            docs: vec!["Test doc".to_string()],
            deprecation_info: DeprecationInfo::not_deprecated(),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"name\":\"BlockWeights\""));
        assert!(json.contains("\"type\":\"123\""));
        assert!(json.contains("\"value\":\"0x1234\""));
    }

    #[test]
    fn test_pallet_constants_response_serialization() {
        let response = PalletConstantsResponse {
            at: AtResponse {
                hash: "0xabc".to_string(),
                height: "100".to_string(),
            },
            pallet: "system".to_string(),
            pallet_index: "0".to_string(),
            items: ConstantsItems::OnlyIds(vec!["BlockWeights".to_string()]),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"pallet\":\"system\""));
        assert!(json.contains("\"palletIndex\":\"0\""));
        assert!(!json.contains("rcBlockHash"));
    }

    #[test]
    fn test_pallet_constant_item_response_serialization() {
        let response = PalletConstantItemResponse {
            at: AtResponse {
                hash: "0xdef".to_string(),
                height: "200".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "5".to_string(),
            constants_item: "existentialDeposit".to_string(),
            metadata: None,
            rc_block_hash: Some("0xrc123".to_string()),
            rc_block_number: Some("1000".to_string()),
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"constantsItem\":\"existentialDeposit\""));
        assert!(json.contains("\"rcBlockHash\":\"0xrc123\""));
        assert!(json.contains("\"rcBlockNumber\":\"1000\""));
    }

    #[test]
    fn test_constants_items_full_serialization() {
        let items = ConstantsItems::Full(vec![ConstantItemMetadata {
            name: "Test".to_string(),
            ty: "1".to_string(),
            value: "0x00".to_string(),
            docs: vec![],
            deprecation_info: DeprecationInfo::not_deprecated(),
        }]);

        let json = serde_json::to_string(&items).unwrap();
        assert!(json.contains("\"name\":\"Test\""));
    }

    #[test]
    fn test_constants_items_only_ids_serialization() {
        let items = ConstantsItems::OnlyIds(vec!["Item1".to_string(), "Item2".to_string()]);

        let json = serde_json::to_string(&items).unwrap();
        assert_eq!(json, r#"["Item1","Item2"]"#);
    }
}
