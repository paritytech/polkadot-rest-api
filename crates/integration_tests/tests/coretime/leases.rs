// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the /v1/coretime/leases endpoint.

use super::{init_tracing, is_coretime_chain, setup_client};
use anyhow::Result;

// ============================================================================
// Response Structure Tests
// ============================================================================

/// Test that the coretime/leases endpoint returns valid JSON with correct structure
#[tokio::test]
async fn test_coretime_leases_response_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    tracing::info!("Testing /v1/coretime/leases endpoint structure");
    let (status, json) = client.get_json("/v1/coretime/leases").await?;

    assert!(
        status.is_success(),
        "Coretime leases endpoint should return success status, got {}",
        status
    );

    // Verify response has required fields
    assert!(json.get("at").is_some(), "Response should have 'at' field");
    assert!(
        json.get("leases").is_some(),
        "Response should have 'leases' field"
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

    // Verify leases is an array
    assert!(json["leases"].is_array(), "'leases' should be an array");

    println!("ok: Coretime leases response structure test passed");
    Ok(())
}

/// Test lease item structure when leases are present
#[tokio::test]
async fn test_coretime_leases_item_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/leases").await?;
    assert!(status.is_success());

    let leases = json["leases"].as_array().unwrap();

    if leases.is_empty() {
        println!("Skipping item structure test: No leases present on chain");
        return Ok(());
    }

    // Check first lease item structure
    let lease = &leases[0];

    assert!(
        lease.get("task").is_some(),
        "Lease should have 'task' field"
    );
    assert!(
        lease.get("until").is_some(),
        "Lease should have 'until' field"
    );

    assert!(lease["task"].is_string(), "'task' should be a string");
    assert!(lease["until"].is_number(), "'until' should be a number");

    // 'core' is optional but if present should be a number
    if lease.get("core").is_some() && !lease["core"].is_null() {
        assert!(
            lease["core"].is_number(),
            "'core' should be a number when present"
        );
    }

    // Validate task is a valid parachain ID (numeric string)
    let task = lease["task"].as_str().unwrap();
    assert!(
        task.parse::<u32>().is_ok(),
        "'task' should be a numeric string (parachain ID)"
    );

    // Validate until is a positive number
    let until_num = lease["until"].as_u64().expect("'until' should be a number");
    assert!(
        until_num > 0,
        "'until' should be a positive timeslice value"
    );

    println!(
        "ok: Coretime leases item structure test passed ({} leases found)",
        leases.len()
    );
    Ok(())
}

// ============================================================================
// Query Parameter Tests
// ============================================================================

/// Test the 'at' query parameter with a block number
#[tokio::test]
async fn test_coretime_leases_at_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to find a valid block number
    let (_, latest_json) = client.get_json("/v1/coretime/leases").await?;
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
        .get_json(&format!("/v1/coretime/leases?at={}", query_height))
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

    println!("ok: Coretime leases 'at' block number test passed");
    Ok(())
}

/// Test the 'at' query parameter with a block hash
#[tokio::test]
async fn test_coretime_leases_at_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to get a valid block hash
    let (_, latest_json) = client.get_json("/v1/coretime/leases").await?;
    let block_hash = latest_json["at"]["hash"].as_str().unwrap();

    let (status, json) = client
        .get_json(&format!("/v1/coretime/leases?at={}", block_hash))
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

    println!("ok: Coretime leases 'at' block hash test passed");
    Ok(())
}

// ============================================================================
// Error Handling Tests
// ============================================================================

/// Test error response for invalid block parameter
#[tokio::test]
async fn test_coretime_leases_invalid_block_param() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    let response = client.get("/v1/coretime/leases?at=invalid-block").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for invalid block parameter"
    );

    println!("ok: Coretime leases invalid block parameter test passed");
    Ok(())
}

/// Test error response for non-existent block (very high block number)
#[tokio::test]
async fn test_coretime_leases_nonexistent_block() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/leases?at=999999999").await?;

    // Should return 400 for non-existent block (block number larger than chain height)
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

    println!("ok: Coretime leases non-existent block test passed");
    Ok(())
}

/// Test error response for very large block numbers that cause RPC errors
#[tokio::test]
async fn test_coretime_leases_very_large_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    let response = client.get("/v1/coretime/leases?at=999999999999").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for very large block number, got {}",
        response.status
    );

    println!("ok: Coretime leases very large block number test passed");
    Ok(())
}

/// Test error response for invalid block hash format
#[tokio::test]
async fn test_coretime_leases_invalid_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    // Invalid hex string (not 32 bytes)
    let response = client.get("/v1/coretime/leases?at=0xabc123").await?;

    assert!(
        response.status.as_u16() == 400 || response.status.as_u16() == 404,
        "Should return 400 or 404 for invalid block hash format, got {}",
        response.status
    );

    println!("ok: Coretime leases invalid block hash test passed");
    Ok(())
}

// ============================================================================
// Consistency Tests
// ============================================================================

/// Test that multiple requests return consistent data
#[tokio::test]
async fn test_coretime_leases_consistency() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // Get the latest block hash to ensure we're querying the same block
    let (_, first_response) = client.get_json("/v1/coretime/leases").await?;
    let block_hash = first_response["at"]["hash"].as_str().unwrap();

    // Query the same block multiple times
    for i in 0..3 {
        let (status, json) = client
            .get_json(&format!("/v1/coretime/leases?at={}", block_hash))
            .await?;

        assert!(status.is_success(), "Request {} should succeed", i + 1);

        assert_eq!(
            json["at"]["hash"].as_str().unwrap(),
            block_hash,
            "Request {} should return same block hash",
            i + 1
        );

        assert_eq!(
            json["leases"].as_array().map(|a| a.len()),
            first_response["leases"].as_array().map(|a| a.len()),
            "Request {} should return same number of leases",
            i + 1
        );
    }

    println!("ok: Coretime leases consistency test passed");
    Ok(())
}

/// Test that leases are sorted by core ID
#[tokio::test]
async fn test_coretime_leases_sorting() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/leases").await?;
    assert!(status.is_success());

    let leases = json["leases"].as_array().unwrap();

    if leases.len() < 2 {
        println!("Skipping sorting test: Need at least 2 leases to verify sorting");
        return Ok(());
    }

    // Check that leases with cores are sorted by core ID
    // and leases without cores come last
    let mut last_core: Option<u64> = None;
    let mut seen_none = false;

    for lease in leases {
        let core = lease
            .get("core")
            .and_then(|c| c.as_str())
            .and_then(|s| s.parse::<u64>().ok());

        match (core, last_core, seen_none) {
            // If we see a Some after a None, that's wrong
            (Some(_), _, true) => {
                panic!("Leases with cores should come before leases without cores");
            }
            // If we have two Some values, they should be in order
            (Some(c), Some(last), _) => {
                assert!(
                    c >= last,
                    "Leases should be sorted by core ID (ascending): {} should come after {}",
                    c,
                    last
                );
            }
            // If we transition from Some to None, mark it
            (None, _, _) => {
                seen_none = true;
            }
            _ => {}
        }

        last_core = core;
    }

    println!("ok: Coretime leases sorting test passed");
    Ok(())
}
