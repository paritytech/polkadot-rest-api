// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common utilities for account-related handlers.

mod balance_info;
mod proxy_info;
mod staking_info;
mod staking_payouts;
mod vesting_info;

pub use balance_info::{
    BalanceQueryError, DecodedAccountData, DecodedBalanceLock, FormattedBalanceLock,
    FormattedBlockInfo, RawBalanceInfo, apply_denomination, calculate_transferable, format_balance,
    format_frozen_fields, format_locks, format_transferable, get_default_existential_deposit,
    get_default_token_decimals, get_default_token_symbol, query_balance_info,
};

pub use proxy_info::{DecodedProxyDefinition, ProxyQueryError, RawProxyInfo, query_proxy_info};

pub use staking_info::{
    ClaimStatus, DecodedNominationsInfo, DecodedRewardDestination, DecodedStakingLedger,
    DecodedUnlockingChunk, EraClaimStatus, RawStakingInfo, StakingQueryError, query_staking_info,
};

pub use staking_payouts::{
    RawEraPayouts, RawEraPayoutsData, RawStakingPayouts, RawValidatorPayout, StakingPayoutsParams,
    StakingPayoutsQueryError, query_staking_payouts,
};

pub use vesting_info::{
    DecodedVestingSchedule, RawVestingInfo, VestingQueryError, query_vesting_info,
};
