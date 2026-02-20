// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime storage query functions.
//!
//! This module provides standalone functions for querying various pallet storage items.
//! Each submodule handles a specific pallet.
//!
//! # Available Modules
//! - `assets` - Assets pallet (asset details, metadata, balances, approvals)
//! - `balances` - System/Balances/Proxy/Vesting pallets (account data, locks, proxies, vesting)
//! - `pool_assets` - PoolAssets pallet (LP token details, metadata, balances, approvals)
//! - `staking` - Staking pallet (ledger, nominations, rewards, etc.)

pub mod assets;
pub mod balances;
pub mod pool_assets;
pub mod staking;
