// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime storage query functions.
//!
//! This module provides standalone functions for querying various pallet storage items.
//! Each submodule handles a specific pallet.
//!
//! # Available Modules
//! - `asset_conversion` - AssetConversion pallet (liquidity pool queries)
//! - `assets` - Assets pallet (asset details, metadata, balances, approvals)
//! - `balances` - System/Balances/Proxy/Vesting pallets (account data, locks, proxies, vesting)
//! - `foreign_assets` - ForeignAssets pallet (XCM-based multi-location assets)
//! - `governance` - Referenda pallet (referendum count, etc.)
//! - `pool_assets` - PoolAssets pallet (LP token details, metadata, balances, approvals)
//! - `staking` - Staking pallet (ledger, nominations, rewards, etc.)

pub mod asset_conversion;
pub mod assets;
pub mod balances;
pub mod foreign_assets;
pub mod governance;
pub mod pool_assets;
pub mod staking;
