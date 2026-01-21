//! Types for account-related handlers.

use crate::handlers::common::accounts::StakingPayoutsQueryError;
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

    #[error("Balance query failed: {0}")]
    BalanceQueryFailed(#[from] crate::handlers::common::accounts::BalanceQueryError),
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
// Pool Asset Balances Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/pool-asset-balances endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolAssetBalancesQueryParams {
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

/// Response for GET /accounts/{accountId}/pool-asset-balances
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolAssetBalancesResponse {
    pub at: BlockInfo,
    pub pool_assets: Vec<PoolAssetBalance>,

    // Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Pool asset balance information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolAssetBalance {
    pub asset_id: u32,
    /// Balance as string (u128 serialized as decimal string)
    pub balance: String,
    pub is_frozen: bool,
    pub is_sufficient: bool,
}

// ================================================================================================
// Pool Asset Balances Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum PoolAssetBalancesError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("The runtime does not include the pool assets pallet at this block")]
    PoolAssetsPalletNotAvailable,

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

impl IntoResponse for PoolAssetBalancesError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            PoolAssetBalancesError::InvalidBlockParam(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PoolAssetBalancesError::InvalidAddress(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PoolAssetBalancesError::PoolAssetsPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PoolAssetBalancesError::UseRcBlockNotSupported => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PoolAssetBalancesError::RelayChainNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            PoolAssetBalancesError::BlockResolveFailed(_) => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}

// ================================================================================================
// Pool Asset Approvals Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/pool-asset-approvals endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolAssetApprovalQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// When true, treat 'at' as relay chain block identifier
    #[serde(default)]
    pub use_rc_block: bool,

    /// The pool asset ID to query approval for (required)
    pub asset_id: u32,

    /// The delegate address with spending approval (required)
    pub delegate: String,
}

/// Response for GET /accounts/{accountId}/pool-asset-approvals
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolAssetApprovalResponse {
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

/// Decoded pool asset approval data
#[derive(Debug, Clone)]
pub struct DecodedPoolAssetApproval {
    pub amount: u128,
    pub deposit: u128,
}

// ================================================================================================
// Pool Asset Approval Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum PoolAssetApprovalError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("Invalid delegate address: {0}")]
    InvalidDelegateAddress(String),

    #[error("The runtime does not include the pool assets pallet at this block")]
    PoolAssetsPalletNotAvailable,

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

impl IntoResponse for PoolAssetApprovalError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            PoolAssetApprovalError::InvalidBlockParam(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PoolAssetApprovalError::InvalidAddress(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PoolAssetApprovalError::InvalidDelegateAddress(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PoolAssetApprovalError::PoolAssetsPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PoolAssetApprovalError::UseRcBlockNotSupported => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PoolAssetApprovalError::RelayChainNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            PoolAssetApprovalError::BlockResolveFailed(_) => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
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

// ================================================================================================
// Proxy Info Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/proxy-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// When true, treat 'at' as relay chain block identifier
    #[serde(default)]
    pub use_rc_block: bool,
}

/// Response for GET /accounts/{accountId}/proxy-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyInfoResponse {
    pub at: BlockInfo,

    /// Array of delegated accounts with their proxy definitions
    pub delegated_accounts: Vec<ProxyDefinition>,

    /// The deposit held for the proxies
    pub deposit_held: String,

    // Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// A proxy definition containing the delegate, proxy type, and delay
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyDefinition {
    /// The delegate address that can act on behalf of the account
    pub delegate: String,

    /// The type of proxy (e.g., "Any", "Staking", "Governance", etc.)
    pub proxy_type: String,

    /// The announcement delay in blocks
    pub delay: String,
}

// ================================================================================================
// Proxy Info Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum ProxyInfoError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("useRcBlock is only supported on Asset Hub chains")]
    UseRcBlockNotSupported,

    #[error("Relay chain not configured for this Asset Hub")]
    RelayChainNotConfigured,

    #[error("Relay chain block mapping failed: {0}")]
    RcBlockMappingFailed(#[from] RcBlockError),

    #[error("Proxy query failed: {0}")]
    ProxyQueryFailed(#[from] crate::handlers::common::accounts::ProxyQueryError),
}

impl IntoResponse for ProxyInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            ProxyInfoError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ProxyInfoError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ProxyInfoError::UseRcBlockNotSupported => (StatusCode::BAD_REQUEST, self.to_string()),
            ProxyInfoError::RelayChainNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            ProxyInfoError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            ProxyInfoError::RcBlockMappingFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            ProxyInfoError::ProxyQueryFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}

// ================================================================================================
// Staking Info Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/staking-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// When true, treat 'at' as relay chain block identifier
    #[serde(default)]
    pub use_rc_block: bool,
}

/// Response for GET /accounts/{accountId}/staking-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingInfoResponse {
    pub at: BlockInfo,

    /// Controller address (may be same as stash after controller deprecation)
    pub controller: String,

    /// Reward destination configuration
    pub reward_destination: RewardDestination,

    /// Number of slashing spans
    pub num_slashing_spans: u32,

    /// Nomination info (null if not a nominator)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nominations: Option<NominationsInfo>,

    /// Staking ledger information
    pub staking: StakingLedger,

    // Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Reward destination - can be "Staked", "Stash", "Controller", or { "account": "..." }
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RewardDestination {
    /// Simple variant without account (Staked, Stash, Controller, None)
    Simple(String),
    /// Account variant with specific address
    Account { account: String },
}

/// Nominations information for a nominator
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NominationsInfo {
    /// List of validator addresses being nominated
    pub targets: Vec<String>,

    /// Era in which nomination was submitted
    pub submitted_in: String,

    /// Whether nominations are suppressed
    pub suppressed: bool,
}

/// Staking ledger information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingLedger {
    /// Stash account address
    pub stash: String,

    /// Total locked balance (active + unlocking)
    pub total: String,

    /// Active staked balance
    pub active: String,

    /// Unlocking chunks
    pub unlocking: Vec<UnlockingChunk>,
}

/// An unlocking chunk representing funds that are being unbonded
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockingChunk {
    /// Amount being unlocked
    pub value: String,

    /// Era when funds become available
    pub era: String,
}

// ================================================================================================
// Staking Info Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum StakingInfoError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("useRcBlock is only supported on Asset Hub chains")]
    UseRcBlockNotSupported,

    #[error("Relay chain not configured for this Asset Hub")]
    RelayChainNotConfigured,

    #[error("Relay chain block mapping failed: {0}")]
    RcBlockMappingFailed(#[from] RcBlockError),

    #[error("Staking query failed: {0}")]
    StakingQueryFailed(#[from] crate::handlers::common::accounts::StakingQueryError),
}

impl IntoResponse for StakingInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            StakingInfoError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            StakingInfoError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            StakingInfoError::UseRcBlockNotSupported => (StatusCode::BAD_REQUEST, self.to_string()),
            StakingInfoError::RelayChainNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            StakingInfoError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            StakingInfoError::RcBlockMappingFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            StakingInfoError::StakingQueryFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}

// ================================================================================================
// Staking Payouts Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/staking-payouts endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingPayoutsQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// Number of eras to query. Must be less than HISTORY_DEPTH. Defaults to 1.
    #[serde(default = "default_depth")]
    pub depth: u32,

    /// The era to query at. Defaults to active_era - 1.
    #[serde(default)]
    pub era: Option<u32>,

    /// Only show unclaimed rewards. Defaults to true.
    #[serde(default = "default_unclaimed_only")]
    pub unclaimed_only: bool,

    /// When true, treat 'at' as relay chain block identifier
    #[serde(default)]
    pub use_rc_block: bool,
}

fn default_depth() -> u32 {
    1
}

fn default_unclaimed_only() -> bool {
    true
}

/// Response for GET /accounts/{accountId}/staking-payouts
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingPayoutsResponse {
    pub at: BlockInfo,

    /// Array of era payouts
    pub eras_payouts: Vec<EraPayouts>,

    // Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Payouts for a single era - can be either actual payouts or an error message
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum EraPayouts {
    /// Successful payout data for an era
    Payouts(EraPayoutsData),
    /// Error message when payouts cannot be calculated
    Message { message: String },
}

/// Actual payout data for an era
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EraPayoutsData {
    /// Era index
    pub era: u32,

    /// Total reward points for the era
    pub total_era_reward_points: String,

    /// Total payout for the era
    pub total_era_payout: String,

    /// Individual payouts for validators nominated
    pub payouts: Vec<ValidatorPayout>,
}

/// Payout information for a single validator
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorPayout {
    /// Validator stash account ID
    pub validator_id: String,

    /// Calculated payout amount for the nominator
    pub nominator_staking_payout: String,

    /// Whether the reward has been claimed
    pub claimed: bool,

    /// Validator's reward points for this era
    pub total_validator_reward_points: String,

    /// Validator's commission (as parts per billion, 0-1000000000)
    pub validator_commission: String,

    /// Total stake behind this validator
    pub total_validator_exposure: String,

    /// Nominator's stake behind this validator
    pub nominator_exposure: String,
}

// ================================================================================================
// Staking Payouts Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum StakingPayoutsError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("The runtime does not include the staking pallet at this block")]
    StakingPalletNotAvailable,

    #[error("useRcBlock is only supported on Asset Hub chains")]
    UseRcBlockNotSupported,

    #[error("Relay chain not configured for this Asset Hub")]
    RelayChainNotConfigured,

    #[error("Relay chain block mapping failed: {0}")]
    RcBlockMappingFailed(#[from] RcBlockError),

    #[error("Invalid era: requested era {0} is beyond history depth")]
    InvalidEra(u32),

    #[error("Depth must be greater than 0 and less than history depth")]
    InvalidDepth,

    #[error("No active era found")]
    NoActiveEra,

    #[error("Staking payouts query failed: {0}")]
    StakingPayoutsQueryFailed(StakingPayoutsQueryError),
}

impl From<StakingPayoutsQueryError> for StakingPayoutsError {
    fn from(err: StakingPayoutsQueryError) -> Self {
        match err {
            StakingPayoutsQueryError::StakingPalletNotAvailable => StakingPayoutsError::StakingPalletNotAvailable,
            StakingPayoutsQueryError::NoActiveEra => StakingPayoutsError::NoActiveEra,
            StakingPayoutsQueryError::InvalidEra(era) => StakingPayoutsError::InvalidEra(era),
            StakingPayoutsQueryError::InvalidDepth => StakingPayoutsError::InvalidDepth,
            other => StakingPayoutsError::StakingPayoutsQueryFailed(other),
        }
    }
}

impl IntoResponse for StakingPayoutsError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            StakingPayoutsError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            StakingPayoutsError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            StakingPayoutsError::StakingPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            StakingPayoutsError::UseRcBlockNotSupported => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            StakingPayoutsError::RelayChainNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            StakingPayoutsError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            StakingPayoutsError::InvalidEra(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            StakingPayoutsError::InvalidDepth => (StatusCode::BAD_REQUEST, self.to_string()),
            StakingPayoutsError::NoActiveEra => (StatusCode::BAD_REQUEST, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}

// ================================================================================================
// Vesting Info Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/vesting-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VestingInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// When true, treat 'at' as relay chain block identifier
    #[serde(default)]
    pub use_rc_block: bool,

    /// When true, calculate and include vested amounts
    #[serde(default)]
    pub include_claimable: bool,
}

/// Response for GET /accounts/{accountId}/vesting-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VestingInfoResponse {
    pub at: BlockInfo,

    /// Array of vesting schedules (empty array if no vesting)
    pub vesting: Vec<VestingSchedule>,

    /// Total vested across all schedules (only when includeClaimable=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vested_balance: Option<String>,

    /// Total locked across all schedules (only when includeClaimable=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vesting_total: Option<String>,

    /// Actual claimable amount now (only when includeClaimable=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vested_claimable: Option<String>,

    /// Block number used for calculations (only when includeClaimable=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number_for_calculation: Option<String>,

    /// Source of block number for calculations: "relay" or "self" (only when includeClaimable=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number_source: Option<String>,

    // Only present when useRcBlock=true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// A vesting schedule
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VestingSchedule {
    /// Total tokens locked at start of vesting
    pub locked: String,

    /// Tokens unlocked per block
    pub per_block: String,

    /// Block when vesting begins
    pub starting_block: String,

    /// Amount vested (only when includeClaimable=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vested: Option<String>,
}

// ================================================================================================
// Vesting Info Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum VestingInfoError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("useRcBlock is only supported on Asset Hub chains")]
    UseRcBlockNotSupported,

    #[error("Relay chain not configured for this Asset Hub")]
    RelayChainNotConfigured,

    #[error("Relay chain block mapping failed: {0}")]
    RcBlockMappingFailed(#[from] RcBlockError),

    #[error("Vesting query failed: {0}")]
    VestingQueryFailed(#[from] crate::handlers::common::accounts::VestingQueryError),
}

impl IntoResponse for VestingInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            VestingInfoError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            VestingInfoError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            VestingInfoError::UseRcBlockNotSupported => (StatusCode::BAD_REQUEST, self.to_string()),
            VestingInfoError::RelayChainNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            VestingInfoError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            VestingInfoError::RcBlockMappingFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            VestingInfoError::VestingQueryFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}

// ================================================================================================
// Account Compare Types
// ================================================================================================

/// Query parameters for GET /accounts/compare endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountCompareQueryParams {
    /// Comma-separated list of SS58 addresses to compare (max 30)
    pub addresses: String,
}

/// Response for GET /accounts/compare
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountCompareResponse {
    /// Whether all addresses have the same underlying public key
    pub are_equal: bool,

    /// Details for each address
    pub addresses: Vec<AddressDetails>,
}

/// Details about a single address
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressDetails {
    /// The original SS58 format address
    pub ss58_format: String,

    /// The SS58 prefix (null if invalid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ss58_prefix: Option<u16>,

    /// The network name for the prefix (null if invalid/unknown)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// The public key in hex format (null if invalid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
}

// ================================================================================================
// Account Compare Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum AccountCompareError {
    #[error("Please limit the amount of address parameters to 30")]
    TooManyAddresses,

    #[error("At least one address is required")]
    NoAddresses,
}

impl IntoResponse for AccountCompareError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            AccountCompareError::TooManyAddresses => (StatusCode::BAD_REQUEST, self.to_string()),
            AccountCompareError::NoAddresses => (StatusCode::BAD_REQUEST, self.to_string()),
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}

// ================================================================================================
// Account Validate Types
// ================================================================================================

/// Response for GET /accounts/{accountId}/validate
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountValidateResponse {
    /// Whether the address is valid
    pub is_valid: bool,

    /// The SS58 prefix (null if invalid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ss58_prefix: Option<String>,

    /// The network name for the prefix (null if invalid/unknown)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// The account ID in hex format (null if invalid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

// ================================================================================================
// Account Validate Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum AccountValidateError {
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl IntoResponse for AccountValidateError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            AccountValidateError::InternalError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}
