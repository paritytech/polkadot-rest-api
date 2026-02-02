//! Handlers for coretime-related endpoints.
//!
//! This module provides endpoints for querying coretime data from the Broker pallet,
//! which is available on coretime chains (parachains that run the Broker pallet).

pub mod common;
pub mod leases;
pub mod renewals;
pub mod reservations;

pub use leases::coretime_leases;
pub use renewals::coretime_renewals;
pub use reservations::coretime_reservations;
