//! Common types and utilities shared across pallet endpoints.
//!
//! This module provides shared error types, response types, helper functions,
//! and SCALE decode types used by the pallet endpoints.

use crate::state::AppState;
use crate::utils::{self, BlockId, ResolvedBlock};
use axum::{Json, http::StatusCode, response::IntoResponse};
use config::ChainType;
use parity_scale_codec::Decode;
use serde::Serialize;
use serde_json::json;
use subxt::{Metadata, OnlineClient, SubstrateConfig, client::OnlineClientAtBlock};
use thiserror::Error;

/// Type alias for the subxt client at a specific block.
pub type ClientAtBlock = OnlineClientAtBlock<SubstrateConfig>;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Error)]
pub enum PalletError {
    // ========================================================================
    // Block/Client Errors
    // ========================================================================
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[from] subxt::error::OnlineClientAtBlockError),

    #[error("Bad staking block: {0}")]
    BadStakingBlock(String),

    // ========================================================================
    // Relay Chain Errors
    // ========================================================================
    #[error("Relay chain connection not configured")]
    RelayChainNotConfigured,

    #[error("RC block error: {0}")]
    RcBlockError(#[from] crate::utils::rc_block::RcBlockError),

    #[error("useRcBlock is only supported for Asset Hub chains")]
    UseRcBlockNotSupported,

    #[error("at parameter is required when useRcBlock=true")]
    AtParameterRequired,

    // ========================================================================
    // Storage Fetch Errors
    // ========================================================================
    #[error("Failed to fetch {pallet}::{entry} storage")]
    StorageFetchFailed {
        pallet: &'static str,
        entry: &'static str,
    },

    #[error("Fetch entry of {pallet}::{entry} storage failed with {error}")]
    StorageEntryFetchFailed {
        pallet: &'static str,
        entry: &'static str,
        error: String,
    },

    #[error("Failed to decode {pallet}::{entry} storage")]
    StorageDecodeFailed {
        pallet: &'static str,
        entry: &'static str,
    },

    #[error("Pallet not found: {0}")]
    PalletNotFound(String),

    #[error("Pallet '{0}' is not available on this chain")]
    PalletNotAvailable(&'static str),

    #[error(
        "The runtime does not include the module '{module}' at this block height: {block_height}"
    )]
    PalletNotAvailableAtBlock {
        module: String,
        block_height: String,
    },

    #[error("Asset not found: {0}")]
    AssetNotFound(String),

    #[error("Nomination pool not found: {0}")]
    PoolNotFound(String),

    #[error("Pool asset not found: {0}")]
    PoolAssetNotFound(String),

    #[error(
        "Could not find event item (\"{0}\") in metadata. Event item names are expected to be in PascalCase, e.g. 'Transfer'"
    )]
    EventNotFound(String),

    #[error("No queryable events items found for palletId \"{0}\"")]
    NoEventsInPallet(String),

    // ========================================================================
    // Metadata/Constant Errors
    // ========================================================================
    #[error("Constant {pallet}::{constant} not found in metadata")]
    ConstantNotFound {
        pallet: &'static str,
        constant: &'static str,
    },

    #[error("Constant item '{item}' not found in pallet '{pallet}'")]
    ConstantItemNotFound { pallet: String, item: String },

    #[error("Failed to fetch metadata")]
    MetadataFetchFailed,

    #[error("Failed to decode metadata: {0}")]
    MetadataDecodeFailed(String),

    #[error(
        "Could not find dispatchable item (\"{0}\") in metadata. dispatchable item names are expected to be in camel case, e.g. 'transfer'"
    )]
    DispatchableNotFound(String),

    #[error(
        "Could not find error item (\"{0}\") in metadata. Error item names are expected to be in PascalCase, e.g. 'InsufficientBalance'"
    )]
    ErrorItemNotFound(String),

    #[error(
        "Could not find storage item (\"{item}\") in pallet \"{pallet}\". Storage item names are expected to be in camelCase, e.g. 'account'"
    )]
    StorageItemNotFound { pallet: String, item: String },

    #[error("Unsupported metadata version")]
    UnsupportedMetadataVersion,

    // ========================================================================
    // Staking-Specific Errors
    // ========================================================================
    #[error("Chain '{0}' is not supported for staking progress queries")]
    UnsupportedChainForStaking(String),

    #[error("Active era not found at this block")]
    ActiveEraNotFound,

    #[error("No active or current era was found")]
    CurrentOrActiveEraNotFound,

    #[error("Era start session index not found in BondedEras for active era")]
    EraStartSessionNotFound,

    // ========================================================================
    // Timestamp Errors
    // ========================================================================
    #[error("Failed to fetch timestamp from Timestamp::Now storage")]
    TimestampFetchFailed,

    #[error("Failed to parse timestamp value")]
    TimestampParseFailed,
}

impl IntoResponse for PalletError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            // Block/Client errors
            PalletError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::BlockResolveFailed(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::BadStakingBlock(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::ClientAtBlockFailed(err) => {
                if crate::utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Service temporarily unavailable: {}", err),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }

            // Relay chain errors
            PalletError::RelayChainNotConfigured => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::RcBlockError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            PalletError::UseRcBlockNotSupported => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::AtParameterRequired => (StatusCode::BAD_REQUEST, self.to_string()),

            // Storage errors - NOT_FOUND for missing data, INTERNAL_SERVER_ERROR for decode failures
            PalletError::StorageFetchFailed { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::StorageEntryFetchFailed { .. } => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
            PalletError::StorageDecodeFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            PalletError::PalletNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::PalletNotAvailable(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::PalletNotAvailableAtBlock { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            PalletError::AssetNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::PoolNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::PoolAssetNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::EventNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::NoEventsInPallet(_) => (StatusCode::BAD_REQUEST, self.to_string()),

            // Metadata errors
            PalletError::MetadataFetchFailed => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            PalletError::MetadataDecodeFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            PalletError::ConstantNotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::ConstantItemNotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::DispatchableNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::ErrorItemNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::StorageItemNotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::UnsupportedMetadataVersion => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }

            // Staking-specific errors
            PalletError::UnsupportedChainForStaking(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PalletError::ActiveEraNotFound => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::CurrentOrActiveEraNotFound => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::EraStartSessionNotFound => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }

            // Timestamp errors
            PalletError::TimestampFetchFailed => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::TimestampParseFailed => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct AtResponse {
    pub hash: String,
    pub height: String,
}

/// Formats a 32-byte account ID to SS58 format.
pub fn format_account_id(account: &[u8; 32], ss58_prefix: u16) -> String {
    use sp_core::crypto::Ss58Codec;
    sp_core::sr25519::Public::from_raw(*account).to_ss58check_with_version(ss58_prefix.into())
}

// ============================================================================
// String Case Conversion Utilities
// ============================================================================

/// Convert first character to lowercase (matching Sidecar's camelCase behavior).
/// Used for pallet names: "Balances" -> "balances", "BlockWeights" -> "blockWeights"
pub fn to_lower_camel_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
    }
}

/// Convert snake_case to camelCase.
/// Example: "transfer_allow_death" -> "transferAllowDeath"
pub fn snake_to_camel(s: &str) -> String {
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

/// Convert camelCase to snake_case.
/// Example: "transferAllowDeath" -> "transfer_allow_death"
pub fn camel_to_snake(s: &str) -> String {
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
// Type Resolution Helpers
// ============================================================================

/// Resolve a type ID to its human-readable name for **events** args.
///
/// This function matches Sidecar's behavior for event type resolution:
/// - Primitives: Return name (`u128`, `bool`)
/// - Simple composites (AccountId32, H256, etc.): Return last path segment
/// - Simple enums (no associated data): Return `{"_enum":["Variant1","Variant2"]}`
/// - Complex enums (with associated data): Return `{"_enum":{"Variant":"Type",...}}`
/// - Complex composites (structs with named fields): Return `{"field":"Type",...}`
///
/// # Examples
/// - Type ID for `T::AccountId` -> `AccountId32`
/// - Type ID for `T::Balance` -> `u128`
/// - Simple enum `Status` -> `{"_enum":["Free","Reserved"]}`
/// - Complex enum `DispatchError` -> `{"_enum":{"Other":"Null","Module":"SpRuntimeModuleError",...}}`
/// - Composite `DispatchInfo` -> `{"weight":"SpWeightsWeightV2Weight","class":"DispatchClass",...}`
pub fn resolve_type_name_for_events(metadata: &Metadata, type_id: u32) -> String {
    resolve_type_internal(metadata, type_id, TypeResolutionMode::Events)
}

/// Resolve a type ID to its human-readable name for **dispatchables** args.type.
///
/// This function matches Sidecar's behavior for dispatchable type resolution:
/// - All types use PascalCase path names (e.g., `PalletBalancesAdjustmentDirection`)
/// - Primitives and well-known types use their standard names
///
/// # Examples
/// - Enum `AdjustmentDirection` -> `PalletBalancesAdjustmentDirection`
/// - Composite `AccountId32` -> `AccountId32`
/// - Primitive `u128` -> `u128`
pub fn resolve_type_name_for_dispatchables(metadata: &Metadata, type_id: u32) -> String {
    resolve_type_internal(metadata, type_id, TypeResolutionMode::Dispatchables)
}

/// Legacy function for backwards compatibility - uses events mode.
/// Prefer using `resolve_type_name_for_events` or `resolve_type_name_for_dispatchables`.
pub fn resolve_type_name(metadata: &Metadata, type_id: u32) -> String {
    resolve_type_name_for_events(metadata, type_id)
}

/// Mode for type resolution - determines how composite types are formatted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeResolutionMode {
    /// For events: expand composites and complex enums inline
    Events,
    /// For dispatchables: use PascalCase path names
    Dispatchables,
}

/// Internal type resolution function that handles both modes.
fn resolve_type_internal(metadata: &Metadata, type_id: u32, mode: TypeResolutionMode) -> String {
    let types = metadata.types();
    let Some(ty) = types.resolve(type_id) else {
        return type_id.to_string();
    };

    use scale_info::TypeDef;

    match &ty.type_def {
        TypeDef::Composite(composite) => {
            // Get the type path name
            let path_name = ty
                .path
                .segments
                .last()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Composite".to_string());

            // Well-known simple types that should just return their last path segment name
            // These are common Substrate types that Sidecar returns with just their short name
            let simple_composites = [
                "AccountId32",
                "H256",
                "H160",
                "H512",
                "MultiAddress",
                "PerU16",
                "Perbill",
                "Permill",
                "Percent",
                "FixedU128",
                "FixedI128",
                "Weight",
                "RuntimeCall",
            ];

            // Check if last segment is a well-known type (case-insensitive for robustness)
            if simple_composites
                .iter()
                .any(|&s| s.eq_ignore_ascii_case(&path_name))
            {
                return path_name;
            }

            match mode {
                TypeResolutionMode::Dispatchables => {
                    // For dispatchables, return PascalCase path name
                    if ty.path.segments.is_empty() {
                        path_name
                    } else {
                        path_to_pascal_case(
                            &ty.path
                                .segments
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>(),
                        )
                    }
                }
                TypeResolutionMode::Events => {
                    // For events, expand composite with named fields
                    if composite.fields.is_empty() {
                        path_name
                    } else if composite.fields.iter().all(|f| f.name.is_some()) {
                        // Named fields: {"field":"Type",...}
                        // Field names are converted to camelCase to match Sidecar
                        let field_strs: Vec<String> = composite
                            .fields
                            .iter()
                            .map(|f| {
                                let field_name = snake_to_camel(f.name.as_ref().unwrap());
                                let field_type =
                                    resolve_type_name_for_dispatchables(metadata, f.ty.id);
                                format!("\"{}\":\"{}\"", field_name, field_type)
                            })
                            .collect();
                        format!("{{{}}}", field_strs.join(","))
                    } else {
                        // Unnamed fields (tuple-like struct): just return the name
                        path_name
                    }
                }
            }
        }
        TypeDef::Variant(v) => {
            // Get the type path name
            let path_name = ty
                .path
                .segments
                .last()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Enum".to_string());

            // Well-known enum types that should just return their last path segment name
            // These are common Substrate types that Sidecar returns with just their short name
            let simple_enums = ["MultiAddress", "RuntimeCall", "Option", "Result"];

            // Check if last segment is a well-known type (case-insensitive for robustness)
            if simple_enums
                .iter()
                .any(|&s| s.eq_ignore_ascii_case(&path_name))
            {
                return path_name;
            }

            let is_simple_enum = v.variants.iter().all(|var| var.fields.is_empty());

            match mode {
                TypeResolutionMode::Dispatchables => {
                    // For dispatchables, always use PascalCase path name
                    if ty.path.segments.is_empty() {
                        "Enum".to_string()
                    } else {
                        path_to_pascal_case(
                            &ty.path
                                .segments
                                .iter()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>(),
                        )
                    }
                }
                TypeResolutionMode::Events => {
                    if is_simple_enum {
                        // Simple enums: {"_enum":["Variant1","Variant2"]}
                        let variant_names: Vec<String> = v
                            .variants
                            .iter()
                            .map(|var| format!("\"{}\"", var.name))
                            .collect();
                        format!("{{\"_enum\":[{}]}}", variant_names.join(","))
                    } else {
                        // Complex enums: {"_enum":{"Variant":"Type",...}}
                        let variant_strs: Vec<String> = v
                            .variants
                            .iter()
                            .map(|var| {
                                let var_type = if var.fields.is_empty() {
                                    "Null".to_string()
                                } else if var.fields.len() == 1 {
                                    // Single field: use its type
                                    resolve_type_name_for_dispatchables(
                                        metadata,
                                        var.fields[0].ty.id,
                                    )
                                } else {
                                    // Multiple fields: create a tuple or use composite name
                                    let field_types: Vec<String> = var
                                        .fields
                                        .iter()
                                        .map(|f| {
                                            resolve_type_name_for_dispatchables(metadata, f.ty.id)
                                        })
                                        .collect();
                                    format!("({})", field_types.join(","))
                                };
                                format!("\"{}\":\"{}\"", var.name, var_type)
                            })
                            .collect();
                        format!("{{\"_enum\":{{{}}}}}", variant_strs.join(","))
                    }
                }
            }
        }
        TypeDef::Sequence(seq) => {
            let inner = resolve_type_internal(metadata, seq.type_param.id, mode);
            // Vec<u8> is shown as "Bytes" to match Sidecar
            if inner == "u8" {
                "Bytes".to_string()
            } else {
                format!("Vec<{}>", inner)
            }
        }
        TypeDef::Array(arr) => {
            let inner = resolve_type_internal(metadata, arr.type_param.id, mode);
            format!("[{}; {}]", inner, arr.len)
        }
        TypeDef::Tuple(tuple) => {
            if tuple.fields.is_empty() {
                "()".to_string()
            } else {
                let fields: Vec<String> = tuple
                    .fields
                    .iter()
                    .map(|f| resolve_type_internal(metadata, f.id, mode))
                    .collect();
                // No spaces in tuple to match Sidecar: (Type1,Type2) not (Type1, Type2)
                format!("({})", fields.join(","))
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
            let inner = resolve_type_internal(metadata, compact.type_param.id, mode);
            format!("Compact<{}>", inner)
        }
        TypeDef::BitSequence(_) => "BitSequence".to_string(),
    }
}

/// Simplify a type name for dispatchables typeName field.
/// Converts `Vec<u8>` to `Bytes` and removes `T::` prefix.
pub fn simplify_type_name_for_dispatchables(type_name: &str) -> String {
    // First remove T:: prefix (including inside generics)
    let without_prefix = type_name.replace("T::", "");

    // Convert Vec<u8> to Bytes
    let normalized = without_prefix.replace("Vec<u8>", "Bytes");

    // Only strip <T> suffix specifically, not other generic parameters
    if normalized.ends_with("<T>") {
        normalized[..normalized.len() - 3].to_string()
    } else {
        normalized
    }
}

/// Convert a type path to PascalCase.
/// Skips "types" segment to match Sidecar behavior.
///
/// Example: `["pallet_balances", "types", "AdjustmentDirection"]` -> `"PalletBalancesAdjustmentDirection"`
pub fn path_to_pascal_case(segments: &[String]) -> String {
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

// ============================================================================
// Pallet Lookup Helpers
// ============================================================================

/// Find a pallet by ID (numeric index) or name (case-insensitive).
///
/// This is the standard pallet lookup pattern used by metadata endpoints.
/// It first tries to parse the ID as a numeric index, then falls back to name lookup.
pub fn find_pallet_by_id_or_name<'a>(
    metadata: &'a Metadata,
    pallet_id: &str,
) -> Result<subxt_metadata::PalletMetadata<'a>, PalletError> {
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

    pallet.ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))
}

// ============================================================================
// Block Resolution Helpers
// ============================================================================

/// Result of resolving a block for a pallet query.
pub struct ResolvedBlockContext {
    /// The subxt client at the resolved block.
    pub client_at_block: ClientAtBlock,
    /// The `at` response for the JSON output.
    pub at: AtResponse,
}

/// Resolves the block from the `at` query parameter and returns the client at that block.
///
/// This is the standard block resolution pattern used by most pallet handlers.
/// It handles both hash and number inputs, and falls back to the current block if `at` is `None`.
pub async fn resolve_block_for_pallet(
    client: &OnlineClient<SubstrateConfig>,
    at: Option<&String>,
) -> Result<ResolvedBlockContext, PalletError> {
    let client_at_block = match at {
        None => client.at_current_block().await?,
        Some(at_str) => {
            let block_id = at_str.parse::<BlockId>()?;
            match block_id {
                BlockId::Hash(hash) => client.at_block(hash).await?,
                BlockId::Number(number) => client.at_block(number).await?,
            }
        }
    };

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    Ok(ResolvedBlockContext { client_at_block, at })
}

/// Builds an `AtResponse` from a subxt client at block.
pub fn build_at_response(client_at_block: &ClientAtBlock) -> AtResponse {
    AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    }
}

// ============================================================================
// RC Block Helpers
// ============================================================================

/// Result of validating and resolving an RC block request.
pub struct RcBlockContext {
    /// The resolved relay chain block.
    pub rc_resolved_block: ResolvedBlock,
    /// The Asset Hub blocks found in the relay chain block.
    pub ah_blocks: Vec<crate::utils::rc_block::AhBlockInfo>,
}

/// Validates and resolves the relay chain block for `useRcBlock=true` requests.
///
/// This function:
/// 1. Validates the chain is an Asset Hub
/// 2. Validates the relay chain connection is configured
/// 3. Requires the `at` parameter to be set
/// 4. Resolves the relay chain block
/// 5. Finds the Asset Hub blocks in the relay chain block
pub async fn validate_and_resolve_rc_block(
    state: &AppState,
    at: Option<&String>,
) -> Result<RcBlockContext, PalletError> {
    // Validate this is an Asset Hub chain
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    // Validate relay chain connection is configured
    if state.get_relay_chain_client().is_none() {
        return Err(PalletError::RelayChainNotConfigured);
    }

    // Parse the relay chain block ID (required for useRcBlock)
    let rc_block_id = at
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<BlockId>()?;

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
    let ah_blocks =
        crate::utils::rc_block::find_ah_blocks_in_rc_block(state, &rc_resolved_block).await?;

    Ok(RcBlockContext {
        rc_resolved_block,
        ah_blocks,
    })
}

/// Builds `RcBlockFields` from a resolved relay chain block and optional timestamp.
pub fn build_rc_block_fields(rc_resolved_block: &ResolvedBlock, ah_timestamp: Option<String>) -> RcBlockFields {
    RcBlockFields {
        rc_block_hash: Some(rc_resolved_block.hash.clone()),
        rc_block_number: Some(rc_resolved_block.number.to_string()),
        ah_timestamp,
    }
}

// ============================================================================
// Query Parameters
// ============================================================================

/// Query parameters for pallet metadata endpoints (list endpoints).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletQueryParams {
    /// Block hash or number to query at. If not provided, uses the latest block.
    pub at: Option<String>,

    /// If `true`, only return the names of items without full metadata.
    #[serde(default)]
    pub only_ids: bool,

    /// If `true`, resolve the block from the relay chain (Asset Hub only).
    #[serde(default)]
    pub use_rc_block: bool,
}

/// Query parameters for single item endpoints (e.g., `/pallets/{palletId}/errors/{errorId}`).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletItemQueryParams {
    /// Block hash or number to query at. If not provided, uses the latest block.
    pub at: Option<String>,

    /// If `true`, include full metadata for the item.
    #[serde(default)]
    pub metadata: bool,

    /// If `true`, resolve the block from the relay chain (Asset Hub only).
    #[serde(default)]
    pub use_rc_block: bool,
}

// ============================================================================
// RC Block Fields
// ============================================================================

/// Fields to include in responses when `useRcBlock=true`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlockFields {
    /// Relay chain block hash (when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,

    /// Relay chain block number (when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,

    /// Asset Hub timestamp (when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// ============================================================================
// Deprecation Info
// ============================================================================

/// Deprecation information for an item.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum DeprecationInfo {
    /// Item is not deprecated.
    NotDeprecated(Option<()>),
    /// Item is deprecated without any additional info.
    Deprecated(serde_json::Value),
    /// Item is deprecated with additional info (since, note).
    DeprecatedWithInfo {
        /// The version since which this item is deprecated.
        #[serde(skip_serializing_if = "Option::is_none")]
        since: Option<String>,
        /// A note about the deprecation.
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

impl Default for DeprecationInfo {
    fn default() -> Self {
        DeprecationInfo::NotDeprecated(None)
    }
}

// ============================================================================
// Shared SCALE Decode Types (used by Assets and PoolAssets pallets)
// ============================================================================

/// Asset status enum used in both Assets and PoolAssets pallets.
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub enum AssetStatus {
    Live,
    Frozen,
    Destroying,
}

impl AssetStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetStatus::Live => "Live",
            AssetStatus::Frozen => "Frozen",
            AssetStatus::Destroying => "Destroying",
        }
    }
}

/// Asset details struct used in both Assets::Asset and PoolAssets::Asset storage.
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct AssetDetails {
    pub owner: [u8; 32],
    pub issuer: [u8; 32],
    pub admin: [u8; 32],
    pub freezer: [u8; 32],
    pub supply: u128,
    pub deposit: u128,
    pub min_balance: u128,
    pub is_sufficient: bool,
    pub accounts: u32,
    pub sufficients: u32,
    pub approvals: u32,
    pub status: AssetStatus,
}

/// Asset metadata struct used in both Assets::Metadata and PoolAssets::Metadata storage.
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct AssetMetadataStorage {
    pub deposit: u128,
    pub name: Vec<u8>,
    pub symbol: Vec<u8>,
    pub decimals: u8,
    pub is_frozen: bool,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_lower_camel_case() {
        assert_eq!(to_lower_camel_case("Balances"), "balances");
        assert_eq!(to_lower_camel_case("BlockWeights"), "blockWeights");
        assert_eq!(to_lower_camel_case("SS58Prefix"), "sS58Prefix");
        assert_eq!(to_lower_camel_case("existentialDeposit"), "existentialDeposit");
        assert_eq!(to_lower_camel_case(""), "");
        assert_eq!(to_lower_camel_case("A"), "a");
    }

    #[test]
    fn test_snake_to_camel() {
        assert_eq!(snake_to_camel("transfer_allow_death"), "transferAllowDeath");
        assert_eq!(snake_to_camel("set_balance"), "setBalance");
        assert_eq!(snake_to_camel("transfer"), "transfer");
        assert_eq!(snake_to_camel(""), "");
        assert_eq!(snake_to_camel("a_b_c"), "aBC");
    }

    #[test]
    fn test_camel_to_snake() {
        assert_eq!(camel_to_snake("transferAllowDeath"), "transfer_allow_death");
        assert_eq!(camel_to_snake("setBalance"), "set_balance");
        assert_eq!(camel_to_snake("transfer"), "transfer");
        assert_eq!(camel_to_snake(""), "");
        assert_eq!(camel_to_snake("ABC"), "a_b_c");
    }

    #[test]
    fn test_at_response_serialization() {
        let at = AtResponse {
            hash: "0xabc123".to_string(),
            height: "12345".to_string(),
        };
        let json = serde_json::to_string(&at).unwrap();
        assert!(json.contains("\"hash\":\"0xabc123\""));
        assert!(json.contains("\"height\":\"12345\""));
    }

    #[test]
    fn test_rc_block_fields_default() {
        let fields = RcBlockFields::default();
        assert!(fields.rc_block_hash.is_none());
        assert!(fields.rc_block_number.is_none());
        assert!(fields.ah_timestamp.is_none());
    }

    #[test]
    fn test_asset_status_as_str() {
        assert_eq!(AssetStatus::Live.as_str(), "Live");
        assert_eq!(AssetStatus::Frozen.as_str(), "Frozen");
        assert_eq!(AssetStatus::Destroying.as_str(), "Destroying");
    }
}
