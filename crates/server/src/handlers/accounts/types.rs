//! Types for account-related handlers.

use crate::utils::{self, RcBlockError};
use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt_historic::error::{OnlineClientAtBlockError, StorageError};
use thiserror::Error;

// ================================================================================================
// Query Parameters
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/asset-balances endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetBalancesQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// When true, treat 'at' as relay chain block identifier
    #[serde(default)]
    pub use_rc_block: bool,

    /// Optional list of asset IDs to query (queries all if omitted)
    #[serde(default)]
    pub assets: Vec<u32>,
}

// ================================================================================================
// Response Types
// ================================================================================================

/// Response for GET /accounts/{accountId}/asset-balances
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetBalancesResponse {
    pub at: BlockInfo,
    pub assets: Vec<AssetBalance>,

    // Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Block information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub hash: String,
    pub height: String,
}

/// Asset balance information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetBalance {
    pub asset_id: u32,
    /// Balance as string (u128 serialized as decimal string)
    pub balance: String,
    pub is_frozen: bool,
    pub is_sufficient: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedAssetBalance {
    /// Balance as string (u128 serialized as decimal string)
    pub balance: String,
    pub is_frozen: bool,
    pub is_sufficient: bool,
}

// ================================================================================================
// Asset Approvals Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/asset-approvals endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetApprovalQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// When true, treat 'at' as relay chain block identifier
    #[serde(default)]
    pub use_rc_block: bool,

    /// The asset ID to query approval for (required)
    pub asset_id: u32,

    /// The delegate address with spending approval (required)
    pub delegate: String,
}

/// Response for GET /accounts/{accountId}/asset-approvals
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetApprovalResponse {
    pub at: BlockInfo,

    /// The approved amount (null if approval doesn't exist)
    pub amount: Option<String>,

    /// The deposit associated with the approval (null if approval doesn't exist)
    pub deposit: Option<String>,

    // Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Decoded asset approval data
#[derive(Debug, Clone)]
pub struct DecodedAssetApproval {
    pub amount: u128,
    pub deposit: u128,
}

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum AssetBalancesError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("The runtime does not include the assets pallet at this block")]
    AssetsPalletNotAvailable,

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(#[from] StorageError),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[from] OnlineClientAtBlockError),

    #[error("useRcBlock is only supported on Asset Hub chains")]
    UseRcBlockNotSupported,

    #[error("Relay chain not configured for this Asset Hub")]
    RelayChainNotConfigured,

    #[error("Relay chain block mapping failed: {0}")]
    RcBlockMappingFailed(#[from] RcBlockError),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Failed to fetch storage entry")]
    StorageEntryFailed(#[from] subxt_historic::error::StorageEntryIsNotAPlainValue),
}

impl IntoResponse for AssetBalancesError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            AssetBalancesError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AssetBalancesError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AssetBalancesError::AssetsPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            AssetBalancesError::UseRcBlockNotSupported => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            AssetBalancesError::RelayChainNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            AssetBalancesError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}

// ================================================================================================
// Asset Approval Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum AssetApprovalError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("Invalid delegate address: {0}")]
    InvalidDelegateAddress(String),

    #[error("The runtime does not include the assets pallet at this block")]
    AssetsPalletNotAvailable,

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(#[from] StorageError),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[from] OnlineClientAtBlockError),

    #[error("useRcBlock is only supported on Asset Hub chains")]
    UseRcBlockNotSupported,

    #[error("Relay chain not configured for this Asset Hub")]
    RelayChainNotConfigured,

    #[error("Relay chain block mapping failed: {0}")]
    RcBlockMappingFailed(#[from] RcBlockError),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Failed to fetch storage entry")]
    StorageEntryFailed(#[from] subxt_historic::error::StorageEntryIsNotAPlainValue),
}

impl IntoResponse for AssetApprovalError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            AssetApprovalError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AssetApprovalError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AssetApprovalError::InvalidDelegateAddress(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            AssetApprovalError::AssetsPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            AssetApprovalError::UseRcBlockNotSupported => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            AssetApprovalError::RelayChainNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            AssetApprovalError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}
