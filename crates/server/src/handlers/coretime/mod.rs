// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handlers for coretime-related endpoints.
//!
//! This module provides endpoints for querying coretime data from the Broker pallet,
//! which is available on coretime chains (parachains that run the Broker pallet).
//!
//! For relay chains, limited coretime information is available from the Coretime pallet.
//!
//! # Module Organization
//!
//! - `common` - Shared types, error handling, and utility functions
//! - `info` - GET /coretime/info endpoint
//! - `leases` - GET /coretime/leases endpoint (also exports `fetch_leases`)
//! - `overview` - GET /coretime/overview endpoint
//! - `regions` - GET /coretime/regions endpoint (also exports `fetch_regions`)
//! - `renewals` - GET /coretime/renewals endpoint
//! - `reservations` - GET /coretime/reservations endpoint (also exports `fetch_reservations`)

pub mod common;
pub mod info;
pub mod leases;
pub mod overview;
pub mod regions;
pub mod renewals;
pub mod reservations;

pub use info::coretime_info;
pub use leases::coretime_leases;
pub use overview::coretime_overview;
pub use regions::coretime_regions;
pub use renewals::coretime_renewals;
pub use reservations::coretime_reservations;
