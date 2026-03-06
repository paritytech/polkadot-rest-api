// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common test helpers, macros, and assertion utilities.
//!
//! This module provides reusable utilities to reduce boilerplate in integration tests.

use crate::client::TestClient;
use crate::constants::API_READY_TIMEOUT_SECONDS;
use anyhow::Result;
use serde_json::Value;
use std::env;

// ============================================================================
// Test Setup Helpers
// ============================================================================

/// Initialize tracing for tests. Safe to call multiple times.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

/// Create a test client and wait for the API to be ready.
pub async fn setup_client() -> Result<TestClient> {
    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;
    Ok(client)
}

/// Create a test client with a custom API URL.
pub async fn setup_client_with_url(api_url: &str) -> Result<TestClient> {
    let client = TestClient::new(api_url.to_string());
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;
    Ok(client)
}

// ============================================================================
// Chain Detection Helpers
// ============================================================================

/// Check if the connected chain has the Broker pallet (is a coretime chain).
pub async fn is_coretime_chain(client: &TestClient) -> bool {
    has_pallet(client, "Broker").await
}

/// Check if the connected chain has a specific pallet.
pub async fn has_pallet(client: &TestClient, pallet_name: &str) -> bool {
    if let Ok((status, json)) = client.get_json("/v1/capabilities").await {
        if status.is_success() {
            if let Some(pallets) = json["pallets"].as_array() {
                return pallets.iter().any(|p| p.as_str() == Some(pallet_name));
            }
        }
    }
    false
}

/// Check if the connected chain is a relay chain (has ParaInclusion pallet).
pub async fn is_relay_chain(client: &TestClient) -> bool {
    has_pallet(client, "ParaInclusion").await
}

/// Check if the connected chain is a parachain (not a relay chain).
pub async fn is_parachain(client: &TestClient) -> bool {
    !is_relay_chain(client).await
}

// ============================================================================
// Response Structure Assertions
// ============================================================================

/// Assert that the response has a valid `at` field with hash and height.
pub fn assert_valid_at_field(json: &Value) -> Result<()> {
    let at = json
        .get("at")
        .ok_or_else(|| anyhow::anyhow!("Response should have 'at' field"))?;

    anyhow::ensure!(at.get("hash").is_some(), "'at' should have 'hash' field");
    anyhow::ensure!(
        at.get("height").is_some(),
        "'at' should have 'height' field"
    );
    anyhow::ensure!(at["hash"].is_string(), "'at.hash' should be a string");
    anyhow::ensure!(at["height"].is_string(), "'at.height' should be a string");

    let hash = at["hash"].as_str().unwrap();
    anyhow::ensure!(
        hash.starts_with("0x"),
        "'at.hash' should be a hex string starting with 0x"
    );

    Ok(())
}

/// Assert that a field exists and is an array.
pub fn assert_array_field<'a>(json: &'a Value, field_name: &str) -> Result<&'a Vec<Value>> {
    let field = json
        .get(field_name)
        .ok_or_else(|| anyhow::anyhow!("Response should have '{}' field", field_name))?;

    field
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("'{}' should be an array", field_name))
}

/// Assert that a field exists and is a string.
pub fn assert_string_field<'a>(json: &'a Value, field_name: &str) -> Result<&'a str> {
    let field = json
        .get(field_name)
        .ok_or_else(|| anyhow::anyhow!("Response should have '{}' field", field_name))?;

    field
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("'{}' should be a string", field_name))
}

/// Assert that a field exists and is a number.
pub fn assert_number_field(json: &Value, field_name: &str) -> Result<f64> {
    let field = json
        .get(field_name)
        .ok_or_else(|| anyhow::anyhow!("Response should have '{}' field", field_name))?;

    field
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("'{}' should be a number", field_name))
}

/// Assert that a hex string is valid (starts with 0x).
pub fn assert_hex_string(value: &str, field_name: &str) -> Result<()> {
    anyhow::ensure!(
        value.starts_with("0x"),
        "'{}' should be a hex string starting with 0x, got: {}",
        field_name,
        value
    );
    Ok(())
}

// ============================================================================
// Error Response Assertions
// ============================================================================

/// Assert that an endpoint returns a specific HTTP status code.
pub async fn assert_status(
    client: &TestClient,
    endpoint: &str,
    expected_status: u16,
) -> Result<()> {
    let response = client.get(endpoint).await?;
    anyhow::ensure!(
        response.status.as_u16() == expected_status,
        "Expected status {} for {}, got {}",
        expected_status,
        endpoint,
        response.status
    );
    Ok(())
}

/// Assert that an endpoint returns 400 Bad Request.
pub async fn assert_bad_request(client: &TestClient, endpoint: &str) -> Result<()> {
    assert_status(client, endpoint, 400).await
}

/// Assert that an endpoint returns 404 Not Found.
pub async fn assert_not_found(client: &TestClient, endpoint: &str) -> Result<()> {
    assert_status(client, endpoint, 404).await
}

/// Assert that an endpoint returns either 400 or 404.
pub async fn assert_client_error(client: &TestClient, endpoint: &str) -> Result<()> {
    let response = client.get(endpoint).await?;
    let status = response.status.as_u16();
    anyhow::ensure!(
        status == 400 || status == 404,
        "Expected 400 or 404 for {}, got {}",
        endpoint,
        status
    );
    Ok(())
}

// ============================================================================
// Test Skip Helpers
// ============================================================================

/// Print a skip message and return Ok(()). Use in tests that should be skipped.
pub fn skip_test(reason: &str) -> Result<()> {
    println!("Skipping test: {}", reason);
    Ok(())
}

/// Macro to skip a test if a condition is not met.
#[macro_export]
macro_rules! skip_if {
    ($condition:expr, $reason:expr) => {
        if $condition {
            println!("Skipping test: {}", $reason);
            return Ok(());
        }
    };
}

/// Macro to skip a test if the chain doesn't have a required pallet.
#[macro_export]
macro_rules! require_pallet {
    ($client:expr, $pallet:expr) => {
        if !$crate::test_helpers::has_pallet($client, $pallet).await {
            println!(
                "Skipping test: {} pallet not found on connected chain",
                $pallet
            );
            return Ok(());
        }
    };
}

/// Macro to skip a test if not connected to a coretime chain.
#[macro_export]
macro_rules! require_coretime_chain {
    ($client:expr) => {
        if !$crate::test_helpers::is_coretime_chain($client).await {
            println!("Skipping test: Not a coretime chain (Broker pallet not found)");
            return Ok(());
        }
    };
}

/// Macro to skip a test if not connected to a relay chain.
#[macro_export]
macro_rules! require_relay_chain {
    ($client:expr) => {
        if !$crate::test_helpers::is_relay_chain($client).await {
            println!("Skipping test: Not a relay chain (ParaInclusion pallet not found)");
            return Ok(());
        }
    };
}

// ============================================================================
// Block Query Helpers
// ============================================================================

/// Get the latest block height from an endpoint response.
pub fn get_block_height(json: &Value) -> Result<u64> {
    let height_str = json["at"]["height"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Could not get block height from response"))?;

    height_str
        .parse()
        .map_err(|e| anyhow::anyhow!("Could not parse block height '{}': {}", height_str, e))
}

/// Get the block hash from an endpoint response.
pub fn get_block_hash(json: &Value) -> Result<&str> {
    json["at"]["hash"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Could not get block hash from response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_assert_valid_at_field() {
        let valid = json!({
            "at": {
                "hash": "0x1234567890abcdef",
                "height": "1000000"
            }
        });
        assert!(assert_valid_at_field(&valid).is_ok());

        let missing_at = json!({});
        assert!(assert_valid_at_field(&missing_at).is_err());

        let missing_hash = json!({
            "at": {
                "height": "1000000"
            }
        });
        assert!(assert_valid_at_field(&missing_hash).is_err());

        let invalid_hash = json!({
            "at": {
                "hash": "not-hex",
                "height": "1000000"
            }
        });
        assert!(assert_valid_at_field(&invalid_hash).is_err());
    }

    #[test]
    fn test_assert_array_field() {
        let json = json!({
            "items": [1, 2, 3]
        });
        assert!(assert_array_field(&json, "items").is_ok());
        assert!(assert_array_field(&json, "missing").is_err());
    }

    #[test]
    fn test_get_block_height() {
        let json = json!({
            "at": {
                "hash": "0xabc",
                "height": "12345"
            }
        });
        assert_eq!(get_block_height(&json).unwrap(), 12345);
    }
}
