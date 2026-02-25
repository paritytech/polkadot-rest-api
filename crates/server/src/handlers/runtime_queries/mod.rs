// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime storage query functions.
//!
//! This module provides standalone functions for querying various pallet storage items.
//! Each submodule handles a specific pallet or group of related pallets.
//!
//! # Available Modules
//! - `asset_conversion` - AssetConversion pallet (liquidity pool queries)
//! - `assets` - Assets pallet (asset details, metadata, balances, approvals)
//! - `balances` - System/Balances/Proxy/Vesting pallets (account data, locks, proxies, vesting)
//! - `broker` - Broker pallet (leases, reservations, configuration, workloads)
//! - `coretime` - Coretime pallets (broker ID, core descriptors, on-demand config)
//! - `foreign_assets` - ForeignAssets pallet (XCM-based multi-location assets)
//! - `governance` - Referenda pallet (referendum count, etc.)
//! - `nomination_pools` - NominationPools pallet (bonded/reward pools)
//! - `parachain_system` - ParachainSystem/ParachainInfo pallets (relay block number, para ID)
//! - `pool_assets` - PoolAssets pallet (LP token details, metadata, balances, approvals)
//! - `referenda` - Referenda pallet (referendum status, ongoing referenda)
//! - `staking` - Staking pallet (ledger, nominations, rewards, validators, etc.)
//! - `system` - System pallet (events)

pub mod asset_conversion;
pub mod assets;
pub mod assets_common;
pub mod balances;
pub mod broker;
pub mod coretime;
pub mod foreign_assets;
pub mod governance;
pub mod nomination_pools;
pub mod parachain_system;
pub mod pool_assets;
pub mod referenda;
pub mod staking;
pub mod system;
