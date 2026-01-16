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

// ================================================================================================
// Balance Info Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/balance-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// When true, treat 'at' as relay chain block identifier
    #[serde(default)]
    pub use_rc_block: bool,

    /// Token symbol for chains with multiple tokens (ORML). Defaults to native token.
    #[serde(default)]
    pub token: Option<String>,

    /// When true, denominate balances using chain decimals
    #[serde(default)]
    pub denominated: bool,
}

/// Response for GET /accounts/{accountId}/balance-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceInfoResponse {
    pub at: BlockInfo,

    /// Account nonce
    pub nonce: String,

    /// Token symbol
    pub token_symbol: String,

    /// Free balance (not equivalent to spendable balance)
    pub free: String,

    /// Reserved balance
    pub reserved: String,

    /// The amount that free may not drop below when withdrawing for anything except
    /// transaction fee payment (legacy field, may be string message for newer runtimes)
    pub misc_frozen: String,

    /// The amount that free may not drop below when withdrawing specifically for
    /// transaction fee payment (legacy field, may be string message for newer runtimes)
    pub fee_frozen: String,

    /// Frozen balance (newer runtimes, may be string message for older runtimes)
    pub frozen: String,

    /// Calculated transferable balance using: free - max(maybeED, frozen - reserved)
    pub transferable: String,

    /// Array of balance locks
    pub locks: Vec<BalanceLock>,

    // Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Balance lock information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceLock {
    /// Lock identifier
    pub id: String,

    /// Amount locked
    pub amount: String,

    /// Lock reasons (Fee = 0, Misc = 1, All = 2)
    pub reasons: String,
}

/// Decoded account data from storage
#[derive(Debug, Clone)]
pub struct DecodedAccountData {
    pub nonce: u32,
    pub free: u128,
    pub reserved: u128,
    pub misc_frozen: Option<u128>,
    pub fee_frozen: Option<u128>,
    pub frozen: Option<u128>,
}

/// Decoded balance lock from storage
#[derive(Debug, Clone)]
pub struct DecodedBalanceLock {
    pub id: String,
    pub amount: u128,
    pub reasons: String,
}

// ================================================================================================
// Balance Info Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum BalanceInfoError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("The runtime does not include the balances pallet at this block")]
    BalancesPalletNotAvailable,

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

    #[error("Invalid use of denominated parameter: this chain doesn't have valid decimals")]
    InvalidDenominatedParam,

    #[error("Invalid token: {0}")]
    InvalidToken(String),
}

impl IntoResponse for BalanceInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            BalanceInfoError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            BalanceInfoError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            BalanceInfoError::BalancesPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            BalanceInfoError::UseRcBlockNotSupported => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            BalanceInfoError::RelayChainNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            BalanceInfoError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            BalanceInfoError::InvalidDenominatedParam => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            BalanceInfoError::InvalidToken(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}

// ================================================================================================
// Account Convert Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/convert endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountConvertQueryParams {
    /// Cryptographic scheme: "ed25519", "sr25519", or "ecdsa" (default: "sr25519")
    #[serde(default = "default_scheme")]
    pub scheme: String,

    /// SS58 prefix number (default: 42)
    #[serde(default = "default_prefix")]
    pub prefix: u16,

    /// If true, treat the input as a public key (default: false)
    #[serde(default)]
    pub public_key: bool,
}

fn default_scheme() -> String {
    "sr25519".to_string()
}

fn default_prefix() -> u16 {
    42
}

/// Response for GET /accounts/{accountId}/convert
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountConvertResponse {
    /// SS58 prefix used for encoding
    pub ss58_prefix: u16,

    /// Network name corresponding to the SS58 prefix
    pub network: String,

    /// The SS58-encoded address
    pub address: String,

    /// The original AccountId (hex)
    pub account_id: String,

    /// The cryptographic scheme used
    pub scheme: String,

    /// Whether the input was treated as a public key
    pub public_key: bool,
}

// ================================================================================================
// Account Convert Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum AccountConvertError {
    #[error("The `accountId` parameter provided is not a valid hex value")]
    InvalidHexAccountId,

    #[error("The given `prefix` query parameter does not correspond to an existing network")]
    InvalidPrefix,

    #[error("The `scheme` query parameter provided can be one of the following three values: [ed25519, sr25519, ecdsa]")]
    InvalidScheme,

    #[error("Failed to encode address: {0}")]
    EncodingFailed(String),
}

impl IntoResponse for AccountConvertError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            AccountConvertError::InvalidHexAccountId => (StatusCode::BAD_REQUEST, self.to_string()),
            AccountConvertError::InvalidPrefix => (StatusCode::BAD_REQUEST, self.to_string()),
            AccountConvertError::InvalidScheme => (StatusCode::BAD_REQUEST, self.to_string()),
            AccountConvertError::EncodingFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}
