//! Handler for the `/pallets/{palletId}/dispatchables` endpoint.
//!
//! This endpoint returns the dispatchables (extrinsics/calls) defined in a pallet's metadata.
//! It supports querying at specific blocks and relay chain block resolution
//! for Asset Hub chains.
//!
//! # Sidecar Compatibility
//!
//! This endpoint aims to match the Sidecar `/pallets/{palletId}/dispatchables` response format.
//! Uses Subxt's metadata API which normalizes all metadata versions internally.

// Allow large error types - PalletError contains subxt::error::OnlineClientAtBlockError
// which is large by design. Boxing would add indirection without significant benefit.
#![allow(clippy::result_large_err)]

use crate::handlers::pallets::common::{
    AtResponse, PalletError, PalletItemQueryParams, PalletQueryParams,
};
use crate::state::AppState;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use crate::utils::{self, fetch_block_timestamp};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use serde::Serialize;
use subxt::Metadata;

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert first character to lowercase (for pallet names like "Balances" -> "balances")
fn to_lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
    }
}

/// Convert snake_case to camelCase (for dispatchable names like "transfer_allow_death" -> "transferAllowDeath")
fn snake_to_camel(s: &str) -> String {
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

/// Convert camelCase to snake_case (for looking up dispatchables by user input)
fn camel_to_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }
    result
}

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
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
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
/// - `503 Service Unavailable`: RPC connection lost.
pub async fn get_pallets_dispatchables(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<PalletQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, pallet_id, params).await;
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

    let pallet_info = extract_pallet_dispatchables(&metadata, &pallet_id)?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let items = if params.only_ids {
        DispatchablesItems::OnlyIds(
            pallet_info
                .dispatchables
                .iter()
                .map(|d| d.name.clone())
                .collect(),
        )
    } else {
        DispatchablesItems::Full(pallet_info.dispatchables)
    };

    Ok((
        StatusCode::OK,
        Json(PalletsDispatchablesResponse {
            at,
            pallet: to_lower_first(&pallet_info.name),
            pallet_index: pallet_info.index.to_string(),
            items,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
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

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block(&state, block_id).await?;

    // Get client at block - Subxt normalizes all metadata versions
    let client_at_block = state.client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_dispatchables(&metadata, &pallet_id)?;

    // Convert camelCase input to snake_case for lookup (Sidecar accepts both)
    let dispatchable_id_snake = camel_to_snake(&dispatchable_id);
    let dispatchable = pallet_info
        .dispatchables
        .iter()
        .find(|d| d.name.to_lowercase() == dispatchable_id_snake.to_lowercase())
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.clone()))?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let metadata_field = if params.metadata {
        Some(dispatchable.clone())
    } else {
        None
    };

    Ok((
        StatusCode::OK,
        Json(PalletDispatchableItemResponse {
            at,
            pallet: to_lower_first(&pallet_info.name),
            pallet_index: pallet_info.index.to_string(),
            dispatchable_item: snake_to_camel(&dispatchable.name),
            metadata: metadata_field,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
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
                pallet: to_lower_first(&pallet_id),
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

    // Get client at block - Subxt normalizes all metadata versions
    let client_at_block = state.client.at_block(ah_block.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_dispatchables(&metadata, &pallet_id)?;

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    // Fetch timestamp using Subxt's dynamic storage API
    let ah_timestamp = fetch_block_timestamp(&client_at_block).await;

    let items = if params.only_ids {
        DispatchablesItems::OnlyIds(
            pallet_info
                .dispatchables
                .iter()
                .map(|d| d.name.clone())
                .collect(),
        )
    } else {
        DispatchablesItems::Full(pallet_info.dispatchables)
    };

    Ok((
        StatusCode::OK,
        Json(PalletsDispatchablesResponse {
            at,
            pallet: to_lower_first(&pallet_info.name),
            pallet_index: pallet_info.index.to_string(),
            items,
            rc_block_hash: Some(rc_resolved_block.hash),
            rc_block_number: Some(rc_resolved_block.number.to_string()),
            ah_timestamp,
        }),
    )
        .into_response())
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

    // Get client at block - Subxt normalizes all metadata versions
    let client_at_block = state.client.at_block(ah_block.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_dispatchables(&metadata, &pallet_id)?;

    // Convert camelCase input to snake_case for lookup (Sidecar accepts both)
    let dispatchable_id_snake = camel_to_snake(&dispatchable_id);
    let dispatchable = pallet_info
        .dispatchables
        .iter()
        .find(|d| d.name.to_lowercase() == dispatchable_id_snake.to_lowercase())
        .ok_or_else(|| PalletError::DispatchableNotFound(dispatchable_id.clone()))?;

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    // Fetch timestamp using Subxt's dynamic storage API
    let ah_timestamp = fetch_block_timestamp(&client_at_block).await;

    let metadata_field = if params.metadata {
        Some(dispatchable.clone())
    } else {
        None
    };

    Ok((
        StatusCode::OK,
        Json(PalletDispatchableItemResponse {
            at,
            pallet: to_lower_first(&pallet_info.name),
            pallet_index: pallet_info.index.to_string(),
            dispatchable_item: snake_to_camel(&dispatchable.name),
            metadata: metadata_field,
            rc_block_hash: Some(rc_resolved_block.hash),
            rc_block_number: Some(rc_resolved_block.number.to_string()),
            ah_timestamp,
        }),
    )
        .into_response())
}

// ============================================================================
// Internal Types
// ============================================================================

struct PalletDispatchablesInfo {
    name: String,
    index: u8,
    dispatchables: Vec<DispatchableItemMetadata>,
}

// ============================================================================
// Metadata Extraction - Using Subxt's normalized metadata API
// ============================================================================

/// Extract pallet dispatchables using Subxt's metadata API.
/// Subxt normalizes all metadata versions (V9-V16) into a unified format.
fn extract_pallet_dispatchables(
    metadata: &Metadata,
    pallet_id: &str,
) -> Result<PalletDispatchablesInfo, PalletError> {
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

    // Get call variants from the pallet (if available)
    let dispatchables: Vec<DispatchableItemMetadata> = match pallet.call_variants() {
        Some(variants) => variants
            .iter()
            .map(|variant| {
                let fields: Vec<DispatchableField> = variant
                    .fields
                    .iter()
                    .map(|f| DispatchableField {
                        name: f.name.clone().unwrap_or_default(),
                        ty: f.ty.id.to_string(),
                        type_name: f.type_name.clone(),
                        docs: f.docs.clone(),
                    })
                    .collect();

                let args: Vec<DispatchableArg> = variant
                    .fields
                    .iter()
                    .map(|f| {
                        let type_name = f.type_name.clone().unwrap_or_else(|| f.ty.id.to_string());
                        DispatchableArg {
                            name: snake_to_camel(&f.name.clone().unwrap_or_default()),
                            ty: resolve_type_name(metadata, f.ty.id),
                            type_name: simplify_type_name(&type_name),
                        }
                    })
                    .collect();

                DispatchableItemMetadata {
                    name: variant.name.clone(),
                    fields,
                    index: variant.index.to_string(),
                    docs: variant.docs.clone(),
                    args,
                }
            })
            .collect(),
        None => vec![],
    };

    Ok(PalletDispatchablesInfo {
        name: pallet.name().to_string(),
        index: pallet.call_index(),
        dispatchables,
    })
}

/// Resolve a type ID to its human-readable name using the type registry.
fn resolve_type_name(metadata: &Metadata, type_id: u32) -> String {
    let types = metadata.types();
    let Some(ty) = types.resolve(type_id) else {
        return type_id.to_string();
    };

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
            // Use full path for pallet types, short name for others
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
            let inner = resolve_type_name(metadata, seq.type_param.id);
            format!("Vec<{}>", inner)
        }
        TypeDef::Array(arr) => {
            let inner = resolve_type_name(metadata, arr.type_param.id);
            format!("[{}; {}]", inner, arr.len)
        }
        TypeDef::Tuple(tuple) => {
            if tuple.fields.is_empty() {
                "()".to_string()
            } else {
                let fields: Vec<String> = tuple
                    .fields
                    .iter()
                    .map(|f| resolve_type_name(metadata, f.id))
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
            let inner = resolve_type_name(metadata, compact.type_param.id);
            format!("Compact<{}>", inner)
        }
        TypeDef::BitSequence(_) => "BitSequence".to_string(),
    }
}

/// Convert a type path to PascalCase (e.g., ["pallet_balances", "AdjustmentDirection"] -> "PalletBalancesAdjustmentDirection")
/// Skips "types" segment to match Sidecar behavior
fn path_to_pascal_case(segments: &[String]) -> String {
    segments
        .iter()
        // Skip "types" segment to match Sidecar behavior
        .filter(|segment| segment.as_str() != "types")
        .flat_map(|segment| {
            segment
                .split('_')
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Simplify a type name for args[] by removing T:: prefix and stripping <T> suffix only.
/// Preserves other generic parameters.
/// e.g., "AccountIdLookupOf<T>" -> "AccountIdLookupOf"
///       "Vec<T::AccountId>" -> "Vec<AccountId>"
///       "T::Balance" -> "Balance"
fn simplify_type_name(type_name: &str) -> String {
    // First remove T:: prefix (including inside generics)
    let without_prefix = type_name.replace("T::", "");

    // Only strip <T> suffix specifically, not other generic parameters
    if without_prefix.ends_with("<T>") {
        without_prefix[..without_prefix.len() - 3].to_string()
    } else {
        without_prefix
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_lower_first() {
        assert_eq!(to_lower_first("Balances"), "balances");
        assert_eq!(to_lower_first("System"), "system");
        assert_eq!(to_lower_first("transferAllowDeath"), "transferAllowDeath");
        assert_eq!(to_lower_first(""), "");
        assert_eq!(to_lower_first("A"), "a");
    }

    #[test]
    fn test_snake_to_camel() {
        assert_eq!(snake_to_camel("transfer_allow_death"), "transferAllowDeath");
        assert_eq!(snake_to_camel("set_balance"), "setBalance");
        assert_eq!(snake_to_camel("transfer"), "transfer");
        assert_eq!(snake_to_camel(""), "");
    }

    #[test]
    fn test_camel_to_snake() {
        assert_eq!(camel_to_snake("transferAllowDeath"), "transfer_allow_death");
        assert_eq!(camel_to_snake("setBalance"), "set_balance");
        assert_eq!(camel_to_snake("transfer"), "transfer");
        assert_eq!(camel_to_snake(""), "");
    }

    #[test]
    fn test_simplify_type_name() {
        // T:: prefix is removed, and only <T> suffix is stripped (not other generics)
        assert_eq!(
            simplify_type_name("MultiAddress<AccountId32, ()>"),
            "MultiAddress<AccountId32, ()>"
        );
        assert_eq!(simplify_type_name("u128"), "u128");
        assert_eq!(simplify_type_name("Vec<u8>"), "Vec<u8>");
        assert_eq!(simplify_type_name("Vec<T::AccountId>"), "Vec<AccountId>");
        assert_eq!(simplify_type_name("T::Balance"), "Balance");
        assert_eq!(
            simplify_type_name("AccountIdLookupOf<T>"),
            "AccountIdLookupOf"
        );
    }

    #[test]
    fn test_path_to_pascal_case() {
        let segments = vec![
            "pallet_balances".to_string(),
            "AdjustmentDirection".to_string(),
        ];
        assert_eq!(
            path_to_pascal_case(&segments),
            "PalletBalancesAdjustmentDirection"
        );
    }

    #[test]
    fn test_dispatchable_item_metadata_serialization() {
        let metadata = DispatchableItemMetadata {
            name: "transfer_allow_death".to_string(),
            fields: vec![DispatchableField {
                name: "dest".to_string(),
                ty: "123".to_string(),
                type_name: Some("MultiAddress".to_string()),
                docs: vec![],
            }],
            index: "0".to_string(),
            docs: vec!["Transfer some balance".to_string()],
            args: vec![DispatchableArg {
                name: "dest".to_string(),
                ty: "MultiAddress".to_string(),
                type_name: "MultiAddress".to_string(),
            }],
        };

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"name\":\"transfer_allow_death\""));
        assert!(json.contains("\"index\":\"0\""));
    }

    #[test]
    fn test_pallets_dispatchables_response_serialization() {
        let response = PalletsDispatchablesResponse {
            at: AtResponse {
                hash: "0xabc".to_string(),
                height: "100".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "5".to_string(),
            items: DispatchablesItems::OnlyIds(vec!["transfer_allow_death".to_string()]),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"pallet\":\"balances\""));
        assert!(json.contains("\"palletIndex\":\"5\""));
        assert!(!json.contains("rcBlockHash"));
    }

    #[test]
    fn test_pallet_dispatchable_item_response_serialization() {
        let response = PalletDispatchableItemResponse {
            at: AtResponse {
                hash: "0xdef".to_string(),
                height: "200".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "5".to_string(),
            dispatchable_item: "transferAllowDeath".to_string(),
            metadata: None,
            rc_block_hash: Some("0xrc123".to_string()),
            rc_block_number: Some("1000".to_string()),
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"dispatchableItem\":\"transferAllowDeath\""));
        assert!(json.contains("\"rcBlockHash\":\"0xrc123\""));
        assert!(json.contains("\"rcBlockNumber\":\"1000\""));
    }

    #[test]
    fn test_dispatchables_items_full_serialization() {
        let items = DispatchablesItems::Full(vec![DispatchableItemMetadata {
            name: "test".to_string(),
            fields: vec![],
            index: "0".to_string(),
            docs: vec![],
            args: vec![],
        }]);

        let json = serde_json::to_string(&items).unwrap();
        assert!(json.contains("\"name\":\"test\""));
    }

    #[test]
    fn test_dispatchables_items_only_ids_serialization() {
        let items =
            DispatchablesItems::OnlyIds(vec!["transfer".to_string(), "set_balance".to_string()]);

        let json = serde_json::to_string(&items).unwrap();
        assert_eq!(json, r#"["transfer","set_balance"]"#);
    }
}
