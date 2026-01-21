//! Types for RC account-related handlers.

use crate::handlers::common::accounts::{BalanceQueryError, ProxyQueryError, StakingQueryError, StakingPayoutsQueryError, VestingQueryError};
use crate::utils;
use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

// ================================================================================================
// Query Parameters
// ================================================================================================

/// Query parameters for GET /rc/accounts/{accountId}/balance-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBalanceInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// Token symbol for chains with multiple tokens (defaults to native)
    #[serde(default)]
    pub token: Option<String>,

    /// When true, denominate balances using chain decimals
    #[serde(default)]
    pub denominated: bool,
}

// ================================================================================================
// Response Types
// ================================================================================================

/// Response for GET /rc/accounts/{accountId}/balance-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBalanceInfoResponse {
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
}

/// Block information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub hash: String,
    pub height: String,
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

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum RcBalanceInfoError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("The runtime does not include the balances pallet at this block")]
    BalancesPalletNotAvailable,

    #[error("Relay chain not available. This endpoint requires a relay chain connection.")]
    RelayChainNotAvailable,

    #[error("Balance query failed: {0}")]
    BalanceQueryFailed(#[from] BalanceQueryError),
}

impl IntoResponse for RcBalanceInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            RcBalanceInfoError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcBalanceInfoError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcBalanceInfoError::BalancesPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcBalanceInfoError::RelayChainNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcBalanceInfoError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            RcBalanceInfoError::BalanceQueryFailed(_) => {
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

/// Query parameters for GET /rc/accounts/{accountId}/proxy-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcProxyInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,
}

/// Response for GET /rc/accounts/{accountId}/proxy-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcProxyInfoResponse {
    pub at: BlockInfo,

    /// Array of delegated accounts with their proxy definitions
    pub delegated_accounts: Vec<ProxyDefinition>,

    /// The deposit held for the proxies
    pub deposit_held: String,
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
pub enum RcProxyInfoError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("The runtime does not include the proxy pallet at this block")]
    ProxyPalletNotAvailable,

    #[error("Relay chain not available. This endpoint requires a relay chain connection.")]
    RelayChainNotAvailable,

    #[error("Proxy query failed: {0}")]
    ProxyQueryFailed(#[from] ProxyQueryError),
}

impl IntoResponse for RcProxyInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            RcProxyInfoError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcProxyInfoError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcProxyInfoError::ProxyPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcProxyInfoError::RelayChainNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcProxyInfoError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            RcProxyInfoError::ProxyQueryFailed(_) => {
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
// Vesting Info Types
// ================================================================================================

/// Query parameters for GET /rc/accounts/{accountId}/vesting-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcVestingInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// When true, calculate and include vested amounts
    #[serde(default)]
    pub include_claimable: bool,
}

/// Response for GET /rc/accounts/{accountId}/vesting-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcVestingInfoResponse {
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
pub enum RcVestingInfoError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("The runtime does not include the vesting pallet at this block")]
    VestingPalletNotAvailable,

    #[error("Relay chain not available. This endpoint requires a relay chain connection.")]
    RelayChainNotAvailable,

    #[error("Vesting query failed: {0}")]
    VestingQueryFailed(#[from] VestingQueryError),
}

impl IntoResponse for RcVestingInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            RcVestingInfoError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcVestingInfoError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcVestingInfoError::VestingPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcVestingInfoError::RelayChainNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcVestingInfoError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            RcVestingInfoError::VestingQueryFailed(_) => {
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

/// Query parameters for GET /rc/accounts/{accountId}/staking-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcStakingInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,
}

/// Response for GET /rc/accounts/{accountId}/staking-info
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcStakingInfoResponse {
    pub at: BlockInfo,

    /// Controller account address
    pub controller: String,

    /// Where rewards are paid to
    pub reward_destination: RewardDestination,

    /// Number of slashing spans
    pub num_slashing_spans: u32,

    /// Nominations info (None if not a nominator)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nominations: Option<NominationsInfo>,

    /// Staking ledger
    pub staking: StakingLedger,
}

/// Reward destination
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RewardDestination {
    /// Simple variant (Staked, Stash, Controller, None)
    Simple(String),
    /// Account variant with specific address
    Account {
        account: String,
    },
}

/// Nominations info
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

/// Staking ledger
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

/// Unlocking chunk
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
pub enum RcStakingInfoError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("The runtime does not include the staking pallet at this block")]
    StakingPalletNotAvailable,

    #[error("The address is not a stash account")]
    NotAStashAccount,

    #[error("Relay chain not available. This endpoint requires a relay chain connection.")]
    RelayChainNotAvailable,

    #[error("Staking query failed: {0}")]
    StakingQueryFailed(StakingQueryError),
}

impl From<StakingQueryError> for RcStakingInfoError {
    fn from(err: StakingQueryError) -> Self {
        match err {
            StakingQueryError::StakingPalletNotAvailable => RcStakingInfoError::StakingPalletNotAvailable,
            StakingQueryError::NotAStashAccount => RcStakingInfoError::NotAStashAccount,
            other => RcStakingInfoError::StakingQueryFailed(other),
        }
    }
}

impl IntoResponse for RcStakingInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            RcStakingInfoError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcStakingInfoError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcStakingInfoError::StakingPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcStakingInfoError::NotAStashAccount => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcStakingInfoError::RelayChainNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcStakingInfoError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            RcStakingInfoError::StakingQueryFailed(_) => {
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

/// Query parameters for GET /rc/accounts/{accountId}/staking-payouts endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcStakingPayoutsQueryParams {
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
}

fn default_depth() -> u32 {
    1
}

fn default_unclaimed_only() -> bool {
    true
}

/// Response for GET /rc/accounts/{accountId}/staking-payouts
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcStakingPayoutsResponse {
    pub at: BlockInfo,

    /// Array of era payouts
    pub eras_payouts: Vec<EraPayouts>,
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
pub enum RcStakingPayoutsError {
    #[error("Invalid block parameter: {0}")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed: {0}")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("The runtime does not include the staking pallet at this block")]
    StakingPalletNotAvailable,

    #[error("No active era found")]
    NoActiveEra,

    #[error("Invalid era: requested era {0} is beyond history depth")]
    InvalidEra(u32),

    #[error("Depth must be greater than 0 and less than history depth")]
    InvalidDepth,

    #[error("Relay chain not available. This endpoint requires a relay chain connection.")]
    RelayChainNotAvailable,

    #[error("Staking payouts query failed: {0}")]
    StakingPayoutsQueryFailed(StakingPayoutsQueryError),
}

impl From<StakingPayoutsQueryError> for RcStakingPayoutsError {
    fn from(err: StakingPayoutsQueryError) -> Self {
        match err {
            StakingPayoutsQueryError::StakingPalletNotAvailable => RcStakingPayoutsError::StakingPalletNotAvailable,
            StakingPayoutsQueryError::NoActiveEra => RcStakingPayoutsError::NoActiveEra,
            StakingPayoutsQueryError::InvalidEra(era) => RcStakingPayoutsError::InvalidEra(era),
            StakingPayoutsQueryError::InvalidDepth => RcStakingPayoutsError::InvalidDepth,
            other => RcStakingPayoutsError::StakingPayoutsQueryFailed(other),
        }
    }
}

impl IntoResponse for RcStakingPayoutsError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            RcStakingPayoutsError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcStakingPayoutsError::InvalidAddress(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            RcStakingPayoutsError::StakingPalletNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcStakingPayoutsError::NoActiveEra => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcStakingPayoutsError::InvalidEra(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcStakingPayoutsError::InvalidDepth => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcStakingPayoutsError::RelayChainNotAvailable => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            RcStakingPayoutsError::BlockResolveFailed(_) => (StatusCode::NOT_FOUND, self.to_string()),
            RcStakingPayoutsError::StakingPayoutsQueryFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message
        }));
        (status, body).into_response()
    }
}
