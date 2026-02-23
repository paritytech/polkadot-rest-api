// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Types for RC account-related handlers.

use crate::state::SubstrateLegacyRpc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use subxt::{OnlineClient, SubstrateConfig};
use subxt_rpcs::RpcClient;
use utoipa::ToSchema;

// Re-export shared types from accounts module
pub use crate::handlers::accounts::{
    AccountsError, BalanceLock, BlockInfo, ClaimedReward, EraPayouts, EraPayoutsData,
    NominationsInfo, ProxyDefinition, RewardDestination, StakingLedger, UnlockingChunk,
    ValidatorPayout, VestingSchedule,
};

/// Type alias for relay chain access tuple (client, rpc_client, legacy_rpc)
pub type RelayChainAccess = (
    Arc<OnlineClient<SubstrateConfig>>,
    Arc<RpcClient>,
    Arc<SubstrateLegacyRpc>,
);

/// Type alias for relay chain access with spec name (client, rpc_client, legacy_rpc, spec_name)
pub type RelayChainAccessWithSpec = (
    Arc<OnlineClient<SubstrateConfig>>,
    Arc<RpcClient>,
    Arc<SubstrateLegacyRpc>,
    String,
);

// ================================================================================================
// Balance Info Types (RC-specific)
// ================================================================================================

/// Query parameters for GET /rc/accounts/{accountId}/balance-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

/// Response for GET /rc/accounts/{accountId}/balance-info
#[derive(Debug, Serialize, ToSchema)]
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

// ================================================================================================
// Proxy Info Types (RC-specific)
// ================================================================================================

/// Query parameters for GET /rc/accounts/{accountId}/proxy-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RcProxyInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,
}

/// Response for GET /rc/accounts/{accountId}/proxy-info
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RcProxyInfoResponse {
    pub at: BlockInfo,

    /// Array of delegated accounts with their proxy definitions
    pub delegated_accounts: Vec<ProxyDefinition>,

    /// The deposit held for the proxies
    pub deposit_held: String,
}

// ================================================================================================
// Vesting Info Types (RC-specific)
// ================================================================================================

/// Query parameters for GET /rc/accounts/{accountId}/vesting-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RcVestingInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,
}

/// Response for GET /rc/accounts/{accountId}/vesting-info
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RcVestingInfoResponse {
    pub at: BlockInfo,

    /// Array of vesting schedules (empty array if no vesting)
    pub vesting: Vec<VestingSchedule>,
}

// ================================================================================================
// Staking Info Types (RC-specific)
// ================================================================================================

/// Query parameters for GET /rc/accounts/{accountId}/staking-info endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RcStakingInfoQueryParams {
    /// Block identifier (hash or height) - defaults to latest finalized
    #[serde(default)]
    pub at: Option<String>,

    /// When true, include claimed rewards in the response
    #[serde(default)]
    pub include_claimed_rewards: bool,
}

/// Response for GET /rc/accounts/{accountId}/staking-info
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RcStakingInfoResponse {
    pub at: BlockInfo,

    /// Controller account address
    pub controller: String,

    /// Where rewards are paid to
    pub reward_destination: RewardDestination,

    /// Number of slashing spans
    pub num_slashing_spans: String,

    /// Nominations info (null if not a nominator)
    pub nominations: Option<NominationsInfo>,

    /// Staking ledger
    pub staking: StakingLedger,
}

// ================================================================================================
// Staking Payouts Types (RC-specific)
// ================================================================================================

/// Query parameters for GET /rc/accounts/{accountId}/staking-payouts endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RcStakingPayoutsResponse {
    pub at: BlockInfo,

    /// Array of era payouts
    pub eras_payouts: Vec<EraPayouts>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rc_balance_info_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "123", "unknownField": true}"#;
        let result: Result<RcBalanceInfoQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_proxy_info_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "123", "unknownField": true}"#;
        let result: Result<RcProxyInfoQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_vesting_info_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "123", "unknownField": true}"#;
        let result: Result<RcVestingInfoQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_staking_info_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "123", "unknownField": true}"#;
        let result: Result<RcStakingInfoQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_staking_payouts_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "123", "unknownField": true}"#;
        let result: Result<RcStakingPayoutsQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
