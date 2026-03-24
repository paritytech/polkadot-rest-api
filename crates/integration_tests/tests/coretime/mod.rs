// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for coretime endpoints.
//!
//! These tests verify the coretime endpoint behavior against a running API server.
//! The endpoints are only available on coretime chains (chains with the Broker pallet).
//!
//! Run with:
//!   API_URL=http://localhost:8080 cargo test --package integration_tests --test coretime
//!
//! For testing against a coretime chain:
//!   SAS_SUBSTRATE_URL=wss://kusama-coretime-rpc.polkadot.io cargo run --release &
//!   API_URL=http://localhost:8080 cargo test --package integration_tests --test coretime

// Submodules
mod info;
mod leases;
mod overview;
mod regions;
mod renewals;
mod reservations;

// Re-export common test helpers from the library for use in submodules.
// This provides a convenient way for tests to access these utilities via `super::*`.
pub use integration_tests::test_helpers::{init_tracing, is_coretime_chain, setup_client};
