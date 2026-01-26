//! Handlers for pallet-related endpoints.
//!
//! This module contains all handlers for the `/pallets` API endpoints.

mod assets;
mod common;
mod consts;
mod dispatchables;
mod events;
mod foreign_assets;
mod nomination_pools;
mod pool_assets;
mod staking;

pub use assets::pallets_assets_asset_info;
pub use consts::{pallets_constant_item, pallets_constants};
pub use dispatchables::{get_pallet_dispatchable_item, get_pallets_dispatchables};
pub use events::{get_pallet_event_item, get_pallet_events};
pub use foreign_assets::pallets_foreign_assets;
pub use nomination_pools::{
    get_liquidity_pools, get_next_available_id, pallets_nomination_pools_info,
    pallets_nomination_pools_pool,
};
pub use pool_assets::pallets_pool_assets_asset_info;
pub use staking::{
    pallets_staking_progress, pallets_staking_validators, rc_pallets_staking_progress,
    rc_pallets_staking_validators,
};
