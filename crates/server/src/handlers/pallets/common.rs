//! Common types and utilities shared across pallet endpoints.
//!
//! This module provides shared error types, response types, and SCALE decode types
//! used by the pallet endpoints.

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

    #[error("Asset not found: {0}")]
    AssetNotFound(String),

    #[error("Nomination pool not found: {0}")]
    PoolNotFound(String),

    #[error("Pool asset not found: {0}")]
    PoolAssetNotFound(String),

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

    #[error("Failed to decode metadata")]
    MetadataDecodeFailed,

    #[error(
        "Could not find dispatchable item (\"{0}\") in metadata. dispatchable item names are expected to be in camel case, e.g. 'transfer'"
    )]
    DispatchableNotFound(String),

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
            PalletError::AssetNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::PoolNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::PoolAssetNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),

            // Metadata errors
            PalletError::ConstantNotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::ConstantItemNotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::MetadataFetchFailed => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            PalletError::MetadataDecodeFailed => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            PalletError::DispatchableNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
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
// Query Parameters
// ============================================================================

/// Query parameters for pallet metadata endpoints.
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

/// Query parameters for single item endpoints (e.g., `/pallets/{palletId}/consts/{constantId}`).
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
