// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the /v1/coretime/regions endpoint.

use super::{init_tracing, is_coretime_chain, setup_client};
use anyhow::Result;

// ============================================================================
// Regions Response Structure Tests
// ============================================================================

/// Test that the coretime/regions endpoint returns valid JSON with correct structure
#[tokio::test]
async fn test_coretime_regions_response_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    tracing::info!("Testing /v1/coretime/regions endpoint structure");
    let (status, json) = client.get_json("/v1/coretime/regions").await?;

    assert!(
        status.is_success(),
        "Coretime regions endpoint should return success status, got {}",
        status
    );

    // Verify response has required fields
    assert!(json.get("at").is_some(), "Response should have 'at' field");
    assert!(
        json.get("regions").is_some(),
        "Response should have 'regions' field"
    );

    // Verify 'at' structure
    let at = &json["at"];
    assert!(
        at.get("hash").is_some(),
        "Response 'at' should have 'hash' field"
    );
    assert!(
        at.get("height").is_some(),
        "Response 'at' should have 'height' field"
    );
    assert!(at["hash"].is_string(), "'at.hash' should be a string");
    assert!(at["height"].is_string(), "'at.height' should be a string");

    // Verify hash format (should start with 0x)
    let hash = at["hash"].as_str().unwrap();
    assert!(
        hash.starts_with("0x"),
        "'at.hash' should be a hex string starting with 0x"
    );

    // Verify regions is an array
    assert!(json["regions"].is_array(), "'regions' should be an array");

    println!("ok: Coretime regions response structure test passed");
    Ok(())
}

/// Test region item structure when regions are present
#[tokio::test]
async fn test_coretime_regions_item_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/regions").await?;
    assert!(status.is_success());

    let regions = json["regions"].as_array().unwrap();

    if regions.is_empty() {
        println!("Skipping item structure test: No regions present on chain");
        return Ok(());
    }

    // Check first region item structure
    let region = &regions[0];

    // Required fields
    assert!(
        region.get("core").is_some(),
        "Region should have 'core' field"
    );
    assert!(
        region.get("begin").is_some(),
        "Region should have 'begin' field"
    );
    assert!(
        region.get("mask").is_some(),
        "Region should have 'mask' field"
    );

    assert!(region["core"].is_number(), "'core' should be a number");
    assert!(region["begin"].is_number(), "'begin' should be a number");
    assert!(region["mask"].is_string(), "'mask' should be a string");

    // Validate mask is a valid hex string (should start with 0x)
    let mask = region["mask"].as_str().unwrap();
    assert!(
        mask.starts_with("0x"),
        "'mask' should be a hex string starting with 0x"
    );

    // CoreMask is 80 bits = 10 bytes = 20 hex chars + "0x" prefix
    assert_eq!(
        mask.len(),
        22,
        "'mask' should be 22 characters (0x + 20 hex digits for 10 bytes)"
    );

    // Optional fields: end, owner, paid
    // If present, check their types
    if let Some(end) = region.get("end") {
        if !end.is_null() {
            assert!(end.is_number(), "'end' should be a number when present");
        }
    }

    if let Some(owner) = region.get("owner") {
        if !owner.is_null() {
            assert!(owner.is_string(), "'owner' should be a string when present");
            let owner_str = owner.as_str().unwrap();
            // Owner is an SS58-encoded address (base58 string, typically 47-48 chars)
            // This matches substrate-api-sidecar behavior which uses .toString() on AccountId
            assert!(
                !owner_str.is_empty() && owner_str.chars().all(|c| c.is_alphanumeric()),
                "'owner' should be a valid SS58 address string, got: {}",
                owner_str
            );
        }
    }

    if let Some(paid) = region.get("paid") {
        if !paid.is_null() {
            assert!(paid.is_string(), "'paid' should be a string when present");
            // paid should be a numeric string
            let paid_str = paid.as_str().unwrap();
            assert!(
                paid_str.parse::<u128>().is_ok(),
                "'paid' should be a numeric string"
            );
        }
    }

    println!(
        "ok: Coretime regions item structure test passed ({} regions found)",
        regions.len()
    );
    Ok(())
}

// ============================================================================
// Regions Query Parameter Tests
// ============================================================================

/// Test the 'at' query parameter with a block number for regions
#[tokio::test]
async fn test_coretime_regions_at_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to find a valid block number
    let (_, latest_json) = client.get_json("/v1/coretime/regions").await?;
    let latest_height: u64 = latest_json["at"]["height"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    // Query a slightly older block (if available)
    let query_height = if latest_height > 10 {
        latest_height - 5
    } else {
        latest_height
    };

    let (status, json) = client
        .get_json(&format!("/v1/coretime/regions?at={}", query_height))
        .await?;

    assert!(
        status.is_success(),
        "Should succeed with valid block number, got {}",
        status
    );

    // Verify the response is at the requested block
    let response_height: u64 = json["at"]["height"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        response_height, query_height,
        "Response should be at the requested block height"
    );

    println!("ok: Coretime regions 'at' block number test passed");
    Ok(())
}

/// Test the 'at' query parameter with a block hash for regions
#[tokio::test]
async fn test_coretime_regions_at_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to get a valid block hash
    let (_, latest_json) = client.get_json("/v1/coretime/regions").await?;
    let block_hash = latest_json["at"]["hash"].as_str().unwrap();

    let (status, json) = client
        .get_json(&format!("/v1/coretime/regions?at={}", block_hash))
        .await?;

    assert!(
        status.is_success(),
        "Should succeed with valid block hash, got {}",
        status
    );

    // Verify the response has the same hash
    let response_hash = json["at"]["hash"].as_str().unwrap();
    assert_eq!(
        response_hash, block_hash,
        "Response should be at the requested block hash"
    );

    println!("ok: Coretime regions 'at' block hash test passed");
    Ok(())
}

// ============================================================================
// Regions Error Handling Tests
// ============================================================================

/// Test error response for invalid block parameter for regions
#[tokio::test]
async fn test_coretime_regions_invalid_block_param() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    let response = client.get("/v1/coretime/regions?at=invalid-block").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for invalid block parameter"
    );

    println!("ok: Coretime regions invalid block parameter test passed");
    Ok(())
}

/// Test error response for non-existent block (very high block number) for regions
#[tokio::test]
async fn test_coretime_regions_nonexistent_block() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/regions?at=999999999").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for non-existent block, got {}",
        response.status
    );

    let json = response.json()?;
    assert!(
        json["message"]
            .as_str()
            .map(|m| m.to_lowercase().contains("block"))
            .unwrap_or(false),
        "Error message should mention block: {:?}",
        json
    );

    println!("ok: Coretime regions non-existent block test passed");
    Ok(())
}

/// Test error response for very large block numbers that cause RPC errors for regions
#[tokio::test]
async fn test_coretime_regions_very_large_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    let response = client.get("/v1/coretime/regions?at=999999999999").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for very large block number, got {}",
        response.status
    );

    println!("ok: Coretime regions very large block number test passed");
    Ok(())
}

/// Test error response for invalid block hash format for regions
#[tokio::test]
async fn test_coretime_regions_invalid_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    // Invalid hex string (not 32 bytes)
    let response = client.get("/v1/coretime/regions?at=0xabc123").await?;

    assert!(
        response.status.as_u16() == 400 || response.status.as_u16() == 404,
        "Should return 400 or 404 for invalid block hash format, got {}",
        response.status
    );

    println!("ok: Coretime regions invalid block hash test passed");
    Ok(())
}

// ============================================================================
// Regions Consistency Tests
// ============================================================================

/// Test that multiple requests return consistent data for regions
#[tokio::test]
async fn test_coretime_regions_consistency() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // Get the latest block hash to ensure we're querying the same block
    let (_, first_response) = client.get_json("/v1/coretime/regions").await?;
    let block_hash = first_response["at"]["hash"].as_str().unwrap();

    // Query the same block multiple times
    for i in 0..3 {
        let (status, json) = client
            .get_json(&format!("/v1/coretime/regions?at={}", block_hash))
            .await?;

        assert!(status.is_success(), "Request {} should succeed", i + 1);

        assert_eq!(
            json["at"]["hash"].as_str().unwrap(),
            block_hash,
            "Request {} should return same block hash",
            i + 1
        );

        assert_eq!(
            json["regions"].as_array().map(|a| a.len()),
            first_response["regions"].as_array().map(|a| a.len()),
            "Request {} should return same number of regions",
            i + 1
        );
    }

    println!("ok: Coretime regions consistency test passed");
    Ok(())
}

/// Test that regions are sorted by core ID
#[tokio::test]
async fn test_coretime_regions_sorting() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/regions").await?;
    assert!(status.is_success());

    let regions = json["regions"].as_array().unwrap();

    if regions.len() < 2 {
        println!("Skipping sorting test: Need at least 2 regions to verify sorting");
        return Ok(());
    }

    // Check that regions are sorted by core ID
    let mut last_core: Option<u64> = None;

    for region in regions {
        let core = region["core"].as_u64().unwrap();

        if let Some(last) = last_core {
            assert!(
                core >= last,
                "Regions should be sorted by core ID (ascending): {} should come after {}",
                core,
                last
            );
        }

        last_core = Some(core);
    }

