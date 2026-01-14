//! Handlers for pallets-related endpoints.
//!
//! These endpoints provide access to pallet metadata including
//! storage items, constants, dispatchables, errors, and events.

pub mod storage;

pub use storage::get_pallets_storage;
