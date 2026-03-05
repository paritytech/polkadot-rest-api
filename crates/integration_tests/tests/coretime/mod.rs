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

use anyhow::Result;
use integration_tests::{client::TestClient, constants::API_READY_TIMEOUT_SECONDS};
use std::env;

// Submodules
mod info;
mod leases;
mod overview;
mod regions;
mod renewals;
mod reservations;

// ============================================================================
// Shared Test Helpers
// ============================================================================

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

pub async fn setup_client() -> Result<TestClient> {
    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;
    Ok(client)
}

/// Check if the connected chain is a coretime chain (has Broker pallet)
pub async fn is_coretime_chain(client: &TestClient) -> bool {
    if let Ok((status, json)) = client.get_json("/v1/capabilities").await {
        if status.is_success() {
            if let Some(pallets) = json["pallets"].as_array() {
                return pallets.iter().any(|p| p.as_str() == Some("Broker"));
            }
        }
    }
    false
}
