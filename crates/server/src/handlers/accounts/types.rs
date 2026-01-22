//! Types for account-related handlers.

use super::utils::AddressValidationError;
use crate::handlers::common::accounts::StakingPayoutsQueryError;
use crate::utils::{self, RcBlockError};
use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt::error::{OnlineClientAtBlockError, StorageError};
use thiserror::Error;

// ================================================================================================
// Error Response Helpers
// ================================================================================================

/// Creates a JSON error response with the given status code and message.
fn error_response(status: StatusCode, message: String) -> axum::response::Response {
    let body = Json(json!({ "error": message }));
    (status, body).into_response()
}

/// Macro to implement IntoResponse for error types with status code mapping.
///
/// Usage:
/// ```ignore
/// impl_error_response!(MyError,
///     InvalidBlockParam(_) => BAD_REQUEST,
///     InvalidAddress(_) => BAD_REQUEST,
///     BlockResolveFailed(_) => NOT_FOUND,
///     _ => INTERNAL_SERVER_ERROR
/// );
/// ```
macro_rules! impl_error_response {
    ($error_type:ty, $($variant:pat => $status:ident),+ $(,)?) => {
        impl IntoResponse for $error_type {
            fn into_response(self) -> axum::response::Response {
                let status = match &self {
                    $($variant => StatusCode::$status,)+
                };
                error_response(status, self.to_string())
            }
        }
    };
}

// ================================================================================================
// Query Parameters
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/asset-balances endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetBalancesQueryParams {
    /// Optional Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// Optional When true, treat 'at' as relay chain block identifier
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
// Unified Accounts Error Type
// ================================================================================================

#[derive(Debug, Error)]
pub enum AccountsError {
    // ---- Common errors ----
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(#[from] AddressValidationError),

    #[error("Invalid delegate address: {0}")]
    InvalidDelegateAddress(String),

    #[error("The runtime does not include the {0} pallet at this block")]
    PalletNotAvailable(String),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(#[from] StorageError),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[from] OnlineClientAtBlockError),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),

    // ---- Relay chain errors ----
    #[error("useRcBlock is only supported on Asset Hub chains")]
    UseRcBlockNotSupported,

    #[error("Relay chain not configured for this Asset Hub")]
    RelayChainNotConfigured,

    #[error("Relay chain not available. This endpoint requires a relay chain connection.")]
    RelayChainNotAvailable,

    #[error("Relay chain block mapping failed: {0}")]
    RcBlockMappingFailed(#[from] RcBlockError),

    // ---- Balance-specific errors ----
    #[error("Invalid use of denominated parameter: this chain doesn't have valid decimals")]
    InvalidDenominatedParam,

    #[error("Invalid token: {0}")]
    InvalidToken(String),

    #[error("Balance query failed: {0}")]
    BalanceQueryFailed(#[from] crate::handlers::common::accounts::BalanceQueryError),

    // ---- Proxy-specific errors ----
    #[error("Proxy query failed: {0}")]
    ProxyQueryFailed(#[from] crate::handlers::common::accounts::ProxyQueryError),

    // ---- Staking-specific errors ----
    #[error("Staking query failed: {0}")]
    StakingQueryFailed(#[from] crate::handlers::common::accounts::StakingQueryError),

    #[error("Staking payouts query failed: {0}")]
    StakingPayoutsQueryFailed(StakingPayoutsQueryError),

    #[error("Invalid era: requested era {0} is beyond history depth")]
    InvalidEra(u32),

    #[error("Depth must be greater than 0 and less than history depth")]
    InvalidDepth,

    #[error("No active era found")]
    NoActiveEra,

    #[error("The address is not a stash account")]
    NotAStashAccount,

    // ---- Vesting-specific errors ----
    #[error("Vesting query failed: {0}")]
    VestingQueryFailed(#[from] crate::handlers::common::accounts::VestingQueryError),

    // ---- Account convert errors ----
    #[error("The `accountId` parameter provided is not a valid hex value")]
    InvalidHexAccountId,

    #[error("The given `prefix` query parameter does not correspond to an existing network")]
    InvalidPrefix,

    #[error("The `scheme` query parameter provided can be one of the following three values: [ed25519, sr25519, ecdsa]")]
    InvalidScheme,

    #[error("Failed to encode address: {0}")]
    EncodingFailed(String),

    // ---- Account compare errors ----
    #[error("Please limit the amount of address parameters to 30")]
    TooManyAddresses,

    #[error("At least one address is required")]
    NoAddresses,

    // ---- Generic internal error ----
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl From<StakingPayoutsQueryError> for AccountsError {
    fn from(err: StakingPayoutsQueryError) -> Self {
        match err {
            StakingPayoutsQueryError::StakingPalletNotAvailable => {
                AccountsError::PalletNotAvailable("Staking".to_string())
            }
            StakingPayoutsQueryError::NoActiveEra => AccountsError::NoActiveEra,
            StakingPayoutsQueryError::InvalidEra(era) => AccountsError::InvalidEra(era),
            StakingPayoutsQueryError::InvalidDepth => AccountsError::InvalidDepth,
            other => AccountsError::StakingPayoutsQueryFailed(other),
        }
    }
}

impl_error_response!(AccountsError,
    AccountsError::InvalidBlockParam(_) => BAD_REQUEST,
    AccountsError::InvalidAddress(_) => BAD_REQUEST,
    AccountsError::InvalidDelegateAddress(_) => BAD_REQUEST,
    AccountsError::PalletNotAvailable(_) => BAD_REQUEST,
    AccountsError::UseRcBlockNotSupported => BAD_REQUEST,
    AccountsError::RelayChainNotAvailable => BAD_REQUEST,
    AccountsError::BlockResolveFailed(_) => NOT_FOUND,
    AccountsError::InvalidDenominatedParam => BAD_REQUEST,
    AccountsError::InvalidToken(_) => BAD_REQUEST,
    AccountsError::InvalidEra(_) => BAD_REQUEST,
    AccountsError::InvalidDepth => BAD_REQUEST,
    AccountsError::NoActiveEra => BAD_REQUEST,
    AccountsError::NotAStashAccount => BAD_REQUEST,
    AccountsError::InvalidHexAccountId => BAD_REQUEST,
    AccountsError::InvalidPrefix => BAD_REQUEST,
    AccountsError::InvalidScheme => BAD_REQUEST,
    AccountsError::TooManyAddresses => BAD_REQUEST,
    AccountsError::NoAddresses => BAD_REQUEST,
    _ => INTERNAL_SERVER_ERROR
);

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

    /// When true, include claimed rewards in the response
    #[serde(default)]
    pub include_claimed_rewards: bool,
}

/// Response for GET /accounts/{accountId}/staking-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingInfoResponse {
    pub at: BlockInfo,

    /// Controller address 
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

    /// Total amount being unlocked
    pub unlocking: String,

    /// Claimed rewards per era (only when includeClaimedRewards=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_rewards: Option<Vec<ClaimedReward>>,
}

/// Claimed reward status for a specific era
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimedReward {
    /// Era index
    pub era: String,

    /// Claim status ("claimed" or "unclaimed")
    pub status: String,
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
}

/// Response for GET /accounts/{accountId}/vesting-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VestingInfoResponse {
    pub at: BlockInfo,

    /// Array of vesting schedules (empty array if no vesting)
    pub vesting: Vec<VestingSchedule>,

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
// Account Validate Types
// ================================================================================================

/// Query parameters for GET /accounts/{accountId}/validate endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountValidateQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,
}

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

