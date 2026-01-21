//! Handlers for pallets-related endpoints.
//!
//! These endpoints provide access to pallet metadata including
//! storage items, constants, dispatchables, errors, and events.

pub mod assets;
pub mod common;
pub mod consts;
pub mod storage;

pub use assets::pallets_assets_asset_info;
pub use consts::{get_pallet_const_item, get_pallets_consts};
pub use storage::get_pallets_storage;
