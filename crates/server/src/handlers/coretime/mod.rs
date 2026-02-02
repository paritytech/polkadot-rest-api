//! Handlers for coretime-related endpoints.
//!
//! This module provides endpoints for querying coretime data from the Broker pallet,
//! which is available on coretime chains (parachains that run the Broker pallet).
//!
//! For relay chains, limited coretime information is available from the Coretime pallet.

pub mod common;
pub mod info;
pub mod leases;
pub mod reservations;

pub use info::coretime_info;
pub use leases::coretime_leases;
pub use reservations::coretime_reservations;
