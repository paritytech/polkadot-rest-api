// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common types and utilities shared across pallet endpoints.
//!
//! This module provides shared error types, response types, and SCALE decode types
//! used by the pallet endpoints.

use crate::state::RelayChainError;
use axum::{Json, http::StatusCode, response::IntoResponse};
use parity_scale_codec::Decode;
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

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
    #[error(transparent)]
    RelayChain(#[from] RelayChainError),

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

    #[error("Asset {asset_id} not found at block {block_number}")]
    AssetNotFoundAtBlock {
        asset_id: String,
        block_number: String,
    },

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

impl From<crate::utils::ResolveClientAtBlockError> for PalletError {
    fn from(err: crate::utils::ResolveClientAtBlockError) -> Self {
        match err {
            crate::utils::ResolveClientAtBlockError::ParseError(e) => {
                PalletError::InvalidBlockParam(e)
            }
            crate::utils::ResolveClientAtBlockError::SubxtError(e) => {
                PalletError::ClientAtBlockFailed(e)
            }
        }
    }
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
            PalletError::RelayChain(RelayChainError::NotConfigured) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PalletError::RelayChain(RelayChainError::ConnectionFailed(_)) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
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
            PalletError::AssetNotFoundAtBlock { .. } => (StatusCode::NOT_FOUND, self.to_string()),
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
// Block Resolution Utilities
// ============================================================================

/// Type alias for the Subxt client at a specific block.
pub type ClientAtBlock = subxt::client::OnlineClientAtBlock<subxt::SubstrateConfig>;

/// Result of resolving a block for pallet queries.
///
/// Named `ResolvedBlockContext` to distinguish from `utils::ResolvedBlock`.
pub struct ResolvedBlockContext {
    /// The Subxt client positioned at the resolved block.
    pub client_at_block: ClientAtBlock,
    /// The `at` response containing block hash and height.
    pub at: AtResponse,
}

/// Resolves the block from an optional `at` parameter.
///
/// If `at` is `None`, resolves to the current finalized block.
/// If `at` is `Some`, parses it as either a block hash or number.
///
/// # Arguments
/// * `client` - The Subxt online client
/// * `at` - Optional block identifier (hash or number as string)
///
/// # Returns
/// A `ResolvedBlockContext` containing the client at that block and the `AtResponse`.
pub async fn resolve_block_for_pallet(
    client: &subxt::OnlineClient<subxt::SubstrateConfig>,
    at: Option<&String>,
) -> Result<ResolvedBlockContext, PalletError> {
    let client_at_block = match at {
        None => client.at_current_block().await?,
        Some(at_str) => {
            let block_id = at_str.parse::<crate::utils::BlockId>()?;
            match block_id {
                crate::utils::BlockId::Hash(hash) => client.at_block(hash).await?,
                crate::utils::BlockId::Number(number) => client.at_block(number).await?,
            }
        }
    };

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    Ok(ResolvedBlockContext {
        client_at_block,
        at,
    })
}

// ============================================================================
// Query Parameters
// ============================================================================

/// Query parameters for pallet metadata endpoints (list endpoints).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

/// Query parameters for relay chain pallet list endpoints (e.g., `/rc/pallets/{palletId}/consts`).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RcPalletQueryParams {
    /// Block hash or number to query at. If not provided, uses the latest block.
    pub at: Option<String>,

    /// If `true`, only return the names of items without full metadata.
    #[serde(default)]
    pub only_ids: bool,
}

/// Query parameters for relay chain single item endpoints (e.g., `/rc/pallets/{palletId}/consts/{constantItemId}`).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RcPalletItemQueryParams {
    /// Block hash or number to query at. If not provided, uses the latest block.
    pub at: Option<String>,

    /// If `true`, include full metadata for the item.
    #[serde(default)]
    pub metadata: bool,
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
// Type Resolution Utilities
// ============================================================================

/// Resolves a type ID from the portable registry to a human-readable type name.
///
/// This function formats types to match Sidecar's output format:
/// - `Vec<u8>` → `"Bytes"`
/// - Arrays use no space: `[T;N]` not `[T; N]`
/// - Tuples use no space: `(T,U)` not `(T, U)`
/// - Composites and variants use the last path segment (e.g., `"MultiAddress"`)
/// - Simple enums are formatted as `{"_enum":["Variant1","Variant2"]}`
///
/// # Arguments
/// * `types` - The portable type registry from metadata
/// * `type_id` - The type ID to resolve
///
/// # Returns
/// A string representation of the type suitable for API responses.
pub fn resolve_type_name(types: &scale_info::PortableRegistry, type_id: u32) -> String {
    let Some(ty) = types.resolve(type_id) else {
        return type_id.to_string();
    };

    // Check for simple enums first (variants with no fields)
    if let scale_info::TypeDef::Variant(v) = &ty.type_def {
        let is_simple_enum = v.variants.iter().all(|var| var.fields.is_empty());
        if is_simple_enum {
            let variant_names: Vec<String> = v
                .variants
                .iter()
                .map(|var| format!("\"{}\"", var.name))
                .collect();
            return format!("{{\"_enum\":[{}]}}", variant_names.join(","));
        }
    }

    // If type has a path, use the last segment (type name)
    if !ty.path.segments.is_empty() {
        return ty.path.segments.last().unwrap().clone();
    }

    // Handle types without paths based on their definition
    match &ty.type_def {
        scale_info::TypeDef::Primitive(p) => format!("{:?}", p).to_lowercase(),
        scale_info::TypeDef::Compact(c) => {
            format!("Compact<{}>", resolve_type_name(types, c.type_param.id))
        }
        scale_info::TypeDef::Sequence(s) => {
            let inner = resolve_type_name(types, s.type_param.id);
            // Match Sidecar: Vec<u8> becomes "Bytes"
            if inner == "u8" {
                "Bytes".to_string()
            } else {
                format!("Vec<{}>", inner)
            }
        }
        scale_info::TypeDef::Array(a) => {
            // Match Sidecar: no space in array format [T;N]
            format!("[{};{}]", resolve_type_name(types, a.type_param.id), a.len)
        }
        scale_info::TypeDef::Tuple(t) => {
            if t.fields.is_empty() {
                "()".to_string()
            } else {
                let inner: Vec<String> = t
                    .fields
                    .iter()
                    .map(|f| resolve_type_name(types, f.id))
                    .collect();
                // Match Sidecar: no space in tuple format (T,U)
                format!("({})", inner.join(","))
            }
        }
        scale_info::TypeDef::BitSequence(_) => "BitSequence".to_string(),
        _ => type_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pallet_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<PalletQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_pallet_item_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<PalletItemQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_pallet_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<RcPalletQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_pallet_item_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<RcPalletItemQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
