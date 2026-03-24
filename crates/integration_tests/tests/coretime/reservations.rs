// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the /v1/coretime/reservations endpoint.

use super::{init_tracing, is_coretime_chain, setup_client};
use anyhow::Result;

// ============================================================================
// Reservations Response Structure Tests
// ============================================================================

/// Test that the coretime/reservations endpoint returns valid JSON with correct structure
#[tokio::test]
async fn test_coretime_reservations_response_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    tracing::info!("Testing /v1/coretime/reservations endpoint structure");
    let (status, json) = client.get_json("/v1/coretime/reservations").await?;

    assert!(
        status.is_success(),
        "Coretime reservations endpoint should return success status, got {}",
        status
    );

    // Verify response has required fields
    assert!(json.get("at").is_some(), "Response should have 'at' field");
    assert!(
        json.get("reservations").is_some(),
        "Response should have 'reservations' field"
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

    // Verify reservations is an array
    assert!(
        json["reservations"].is_array(),
        "'reservations' should be an array"
    );

    println!("ok: Coretime reservations response structure test passed");
    Ok(())
}

/// Test reservation item structure when reservations are present
#[tokio::test]
async fn test_coretime_reservations_item_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/reservations").await?;
    assert!(status.is_success());

    let reservations = json["reservations"].as_array().unwrap();

    if reservations.is_empty() {
        println!("Skipping item structure test: No reservations present on chain");
        return Ok(());
    }

    // Check first reservation item structure
    let reservation = &reservations[0];

    assert!(
        reservation.get("mask").is_some(),
        "Reservation should have 'mask' field"
    );
    assert!(
        reservation.get("task").is_some(),
        "Reservation should have 'task' field"
    );

    assert!(reservation["mask"].is_string(), "'mask' should be a string");
    assert!(reservation["task"].is_string(), "'task' should be a string");

    // Validate mask is a valid hex string (should start with 0x)
    let mask = reservation["mask"].as_str().unwrap();
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

    // Validate task is either empty, "Pool", or a numeric string (task ID)
    let task = reservation["task"].as_str().unwrap();
    assert!(
        task.is_empty() || task == "Pool" || task.parse::<u32>().is_ok(),
        "'task' should be empty (Idle), 'Pool', or a numeric task ID, got: {}",
        task
    );

    println!(
        "ok: Coretime reservations item structure test passed ({} reservations found)",
        reservations.len()
    );
    Ok(())
}

// ============================================================================
// Reservations Query Parameter Tests
// ============================================================================

/// Test the 'at' query parameter with a block number for reservations
#[tokio::test]
async fn test_coretime_reservations_at_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to find a valid block number
    let (_, latest_json) = client.get_json("/v1/coretime/reservations").await?;
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
        .get_json(&format!("/v1/coretime/reservations?at={}", query_height))
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

    println!("ok: Coretime reservations 'at' block number test passed");
    Ok(())
}

/// Test the 'at' query parameter with a block hash for reservations
#[tokio::test]
async fn test_coretime_reservations_at_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to get a valid block hash
    let (_, latest_json) = client.get_json("/v1/coretime/reservations").await?;
    let block_hash = latest_json["at"]["hash"].as_str().unwrap();

    let (status, json) = client
        .get_json(&format!("/v1/coretime/reservations?at={}", block_hash))
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

    println!("ok: Coretime reservations 'at' block hash test passed");
    Ok(())
}

// ============================================================================
// Reservations Error Handling Tests
// ============================================================================

/// Test error response for invalid block parameter for reservations
#[tokio::test]
async fn test_coretime_reservations_invalid_block_param() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    let response = client
        .get("/v1/coretime/reservations?at=invalid-block")
        .await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for invalid block parameter"
    );

    println!("ok: Coretime reservations invalid block parameter test passed");
    Ok(())
}

/// Test error response for non-existent block (very high block number) for reservations
#[tokio::test]
async fn test_coretime_reservations_nonexistent_block() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/reservations?at=999999999").await?;

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

    println!("ok: Coretime reservations non-existent block test passed");
    Ok(())
}

/// Test error response for very large block numbers that cause RPC errors for reservations
#[tokio::test]
async fn test_coretime_reservations_very_large_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    let response = client
        .get("/v1/coretime/reservations?at=999999999999")
        .await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for very large block number, got {}",
        response.status
    );

    println!("ok: Coretime reservations very large block number test passed");
    Ok(())
}

/// Test error response for invalid block hash format for reservations
#[tokio::test]
async fn test_coretime_reservations_invalid_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    // Invalid hex string (not 32 bytes)
    let response = client.get("/v1/coretime/reservations?at=0xabc123").await?;

    assert!(
        response.status.as_u16() == 400 || response.status.as_u16() == 404,
        "Should return 400 or 404 for invalid block hash format, got {}",
        response.status
    );

    println!("ok: Coretime reservations invalid block hash test passed");
    Ok(())
}

// ============================================================================
// Reservations Consistency Tests
// ============================================================================

/// Test that multiple requests return consistent data for reservations
#[tokio::test]
async fn test_coretime_reservations_consistency() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // Get the latest block hash to ensure we're querying the same block
    let (_, first_response) = client.get_json("/v1/coretime/reservations").await?;
    let block_hash = first_response["at"]["hash"].as_str().unwrap();

    // Query the same block multiple times
    for i in 0..3 {
        let (status, json) = client
            .get_json(&format!("/v1/coretime/reservations?at={}", block_hash))
            .await?;

        assert!(status.is_success(), "Request {} should succeed", i + 1);

        assert_eq!(
            json["at"]["hash"].as_str().unwrap(),
            block_hash,
            "Request {} should return same block hash",
            i + 1
        );

        assert_eq!(
            json["reservations"].as_array().map(|a| a.len()),
            first_response["reservations"].as_array().map(|a| a.len()),
            "Request {} should return same number of reservations",
            i + 1
        );
    }

    println!("ok: Coretime reservations consistency test passed");
    Ok(())
}

