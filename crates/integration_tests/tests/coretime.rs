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

// ============================================================================
// Test Helpers
// ============================================================================

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

async fn setup_client() -> Result<TestClient> {
    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;
    Ok(client)
}

/// Check if the connected chain is a coretime chain (has Broker pallet)
async fn is_coretime_chain(client: &TestClient) -> bool {
    if let Ok((status, json)) = client.get_json("/v1/capabilities").await {
        if status.is_success() {
            if let Some(pallets) = json["pallets"].as_array() {
                return pallets.iter().any(|p| p.as_str() == Some("Broker"));
            }
        }
    }
    false
}

// ============================================================================
// Leases Response Structure Tests
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
    assert!(lease["until"].is_string(), "'until' should be a string");

    // 'core' is optional but if present should be a string
    if lease.get("core").is_some() && !lease["core"].is_null() {
        assert!(
            lease["core"].is_string(),
            "'core' should be a string when present"
        );
    }

    // Validate task is a valid parachain ID (numeric string)
    let task = lease["task"].as_str().unwrap();
    assert!(
        task.parse::<u32>().is_ok(),
        "'task' should be a numeric string (parachain ID)"
    );

    // Validate until is a positive number (as string)
    let until = lease["until"].as_str().unwrap();
    let until_num: u64 = until.parse().expect("'until' should be a numeric string");
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

/// Test error response for non-coretime chains (chains without Broker pallet)
#[tokio::test]
async fn test_coretime_leases_non_coretime_chain() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Only run this test on non-coretime chains
    if is_coretime_chain(&client).await {
        println!("Skipping test: This is a coretime chain, need non-coretime chain for this test");
        return Ok(());
    }

    let response = client.get("/v1/coretime/leases").await?;

    assert_eq!(
        response.status.as_u16(),
        404,
        "Should return 404 for non-coretime chains"
    );

    let json = response.json()?;
    assert!(
        json["message"]
            .as_str()
            .map(|m| m.contains("Broker") || m.contains("pallet"))
            .unwrap_or(false),
        "Error message should mention Broker pallet not found"
    );

    println!("ok: Coretime leases non-coretime chain error test passed");
    Ok(())
}

/// Test error response for invalid block parameter
#[tokio::test]
async fn test_coretime_leases_invalid_block_param() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

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

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/leases?at=999999999").await?;

    // Should return 404 for non-existent block
    assert_eq!(
        response.status.as_u16(),
        404,
        "Should return 404 for non-existent block, got {}",
        response.status
    );

    let json = response.json()?;
    assert!(
        json["error"]
            .as_str()
            .map(|m| m.to_lowercase().contains("block") && m.to_lowercase().contains("not found"))
            .unwrap_or(false),
        "Error message should indicate block not found: {:?}",
        json
    );

    println!("ok: Coretime leases non-existent block test passed");
    Ok(())
}

/// Test error response for very large block numbers that cause RPC errors
///
/// Block numbers within a valid range but non-existent return "Block not found".
/// However, very large block numbers (e.g., 999999999999) cause the RPC itself
/// to fail, resulting in "Failed to get block hash".
#[tokio::test]
async fn test_coretime_leases_very_large_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    let response = client.get("/v1/coretime/leases?at=999999999999").await?;

    // Should return 400 Bad Request
    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for very large block number, got {}",
        response.status
    );

    let json = response.json()?;
    assert!(
        json["error"]
            .as_str()
            .map(|m| m.contains("Failed to get block hash"))
            .unwrap_or(false),
        "Should indicate block hash retrieval failed: {:?}",
        json
    );

    println!("ok: Coretime leases very large block number test passed");
    Ok(())
}

/// Test error response for invalid block hash format
#[tokio::test]
async fn test_coretime_leases_invalid_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Invalid hex string (not 32 bytes)
    let response = client.get("/v1/coretime/leases?at=0xabc123").await?;

    // Should return 400 or 404 for invalid block hash format
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

/// Test error response for non-coretime chains (chains without Broker pallet) for reservations
#[tokio::test]
async fn test_coretime_reservations_non_coretime_chain() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Only run this test on non-coretime chains
    if is_coretime_chain(&client).await {
        println!("Skipping test: This is a coretime chain, need non-coretime chain for this test");
        return Ok(());
    }

    let response = client.get("/v1/coretime/reservations").await?;

    assert_eq!(
        response.status.as_u16(),
        404,
        "Should return 404 for non-coretime chains"
    );

    let json = response.json()?;
    assert!(
        json["message"]
            .as_str()
            .map(|m| m.contains("Broker") || m.contains("pallet"))
            .unwrap_or(false),
        "Error message should mention Broker pallet not found"
    );

    println!("ok: Coretime reservations non-coretime chain error test passed");
    Ok(())
}

/// Test error response for invalid block parameter for reservations
#[tokio::test]
async fn test_coretime_reservations_invalid_block_param() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

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

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/reservations?at=999999999").await?;

    // Should return 404 for non-existent block
    assert_eq!(
        response.status.as_u16(),
        404,
        "Should return 404 for non-existent block, got {}",
        response.status
    );

    let json = response.json()?;
    assert!(
        json["error"]
            .as_str()
            .map(|m| m.to_lowercase().contains("block") && m.to_lowercase().contains("not found"))
            .unwrap_or(false),
        "Error message should indicate block not found: {:?}",
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

    let response = client
        .get("/v1/coretime/reservations?at=999999999999")
        .await?;

    // Should return 400 Bad Request
    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for very large block number, got {}",
        response.status
    );

    let json = response.json()?;
    assert!(
        json["error"]
            .as_str()
            .map(|m| m.contains("Failed to get block hash"))
            .unwrap_or(false),
        "Should indicate block hash retrieval failed: {:?}",
        json
    );

    println!("ok: Coretime reservations very large block number test passed");
    Ok(())
}

/// Test error response for invalid block hash format for reservations
#[tokio::test]
async fn test_coretime_reservations_invalid_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Invalid hex string (not 32 bytes)
    let response = client.get("/v1/coretime/reservations?at=0xabc123").await?;

    // Should return 400 or 404 for invalid block hash format
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

/// Test that the coretime/info endpoint returns valid JSON with correct structure for coretime chains
#[tokio::test]
async fn test_coretime_info_response_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    tracing::info!("Testing /v1/coretime/info endpoint structure on coretime chain");
    let (status, json) = client.get_json("/v1/coretime/info").await?;

    assert!(
        status.is_success(),
        "Coretime info endpoint should return success status, got {}",
        status
    );

    // Verify response has required fields
    assert!(json.get("at").is_some(), "Response should have 'at' field");

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

    println!("ok: Coretime info response structure test passed");
    Ok(())
}

/// Test coretime/info configuration section
#[tokio::test]
async fn test_coretime_info_configuration() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/info").await?;
    assert!(status.is_success());

    // Configuration may be present if broker is configured
    if let Some(config) = json.get("configuration") {
        if !config.is_null() {
            assert!(
                config.get("regionLength").is_some(),
                "Configuration should have 'regionLength'"
            );
            assert!(
                config.get("interludeLength").is_some(),
                "Configuration should have 'interludeLength'"
            );
            assert!(
                config.get("leadinLength").is_some(),
                "Configuration should have 'leadinLength'"
            );
            assert!(
                config.get("relayBlocksPerTimeslice").is_some(),
                "Configuration should have 'relayBlocksPerTimeslice'"
            );

            // Verify values are strings
            assert!(
                config["regionLength"].is_string(),
                "'regionLength' should be a string"
            );
            assert!(
                config["relayBlocksPerTimeslice"].is_string(),
                "'relayBlocksPerTimeslice' should be a string"
            );

            println!(
                "ok: Configuration found - regionLength: {}, timeslicePeriod: {}",
                config["regionLength"], config["relayBlocksPerTimeslice"]
            );
        }
    }

    println!("ok: Coretime info configuration test passed");
    Ok(())
}

/// Test coretime/info cores section
#[tokio::test]
async fn test_coretime_info_cores() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/info").await?;
    assert!(status.is_success());

    // Cores section may be present if a sale is active
    if let Some(cores) = json.get("cores") {
        if !cores.is_null() {
            assert!(
                cores.get("available").is_some(),
                "Cores should have 'available'"
            );
            assert!(cores.get("sold").is_some(), "Cores should have 'sold'");
            assert!(cores.get("total").is_some(), "Cores should have 'total'");
            assert!(
                cores.get("currentCorePrice").is_some(),
                "Cores should have 'currentCorePrice'"
            );

            // Verify types - all values are strings per sidecar schema
            assert!(
                cores["available"].is_string(),
                "'available' should be a string"
            );
            assert!(cores["sold"].is_string(), "'sold' should be a string");
            assert!(cores["total"].is_string(), "'total' should be a string");
            assert!(
                cores["currentCorePrice"].is_string(),
                "'currentCorePrice' should be a string"
            );

            // Verify logical constraints
            let available: u64 = cores["available"].as_str().unwrap().parse().unwrap();
            let sold: u64 = cores["sold"].as_str().unwrap().parse().unwrap();
            let total: u64 = cores["total"].as_str().unwrap().parse().unwrap();
            assert!(
                available + sold <= total,
                "available + sold should be <= total"
            );

            println!(
                "ok: Cores found - available: {}, sold: {}, total: {}",
                available, sold, total
            );
        }
    }

    println!("ok: Coretime info cores test passed");
    Ok(())
}

/// Test coretime/info phase section
#[tokio::test]
async fn test_coretime_info_phase() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/info").await?;
    assert!(status.is_success());

    // Phase section may be present if broker is configured and sale is active
    if let Some(phase) = json.get("phase") {
        if !phase.is_null() {
            assert!(
                phase.get("currentPhase").is_some(),
                "Phase should have 'currentPhase'"
            );
            assert!(phase.get("config").is_some(), "Phase should have 'config'");

            let current_phase = phase["currentPhase"].as_str().unwrap();
            assert!(
                ["renewals", "priceDiscovery", "fixedPrice"].contains(&current_phase),
                "'currentPhase' should be one of: renewals, priceDiscovery, fixedPrice, got: {}",
                current_phase
            );

            // Verify config is an array
            assert!(phase["config"].is_array(), "'config' should be an array");

            let config_array = phase["config"].as_array().unwrap();
            if !config_array.is_empty() {
                let first_phase = &config_array[0];
                assert!(
                    first_phase.get("phaseName").is_some(),
                    "Phase config should have 'phaseName'"
                );
                assert!(
                    first_phase.get("lastRelayBlock").is_some(),
                    "Phase config should have 'lastRelayBlock'"
                );
                assert!(
                    first_phase.get("lastTimeslice").is_some(),
                    "Phase config should have 'lastTimeslice'"
                );
            }

            println!("ok: Phase found - currentPhase: {}", current_phase);
        }
    }

    println!("ok: Coretime info phase test passed");
    Ok(())
}

/// Test coretime/info 'at' query parameter with block number
#[tokio::test]
async fn test_coretime_info_at_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to find a valid block number
    let (_, latest_json) = client.get_json("/v1/coretime/info").await?;
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
        .get_json(&format!("/v1/coretime/info?at={}", query_height))
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

    println!("ok: Coretime info 'at' block number test passed");
    Ok(())
}

/// Test coretime/info 'at' query parameter with block hash
#[tokio::test]
async fn test_coretime_info_at_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to get a valid block hash
    let (_, latest_json) = client.get_json("/v1/coretime/info").await?;
    let block_hash = latest_json["at"]["hash"].as_str().unwrap();

    let (status, json) = client
        .get_json(&format!("/v1/coretime/info?at={}", block_hash))
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

    println!("ok: Coretime info 'at' block hash test passed");
    Ok(())
}

/// Test coretime/info error response for invalid block parameter
#[tokio::test]
async fn test_coretime_info_invalid_block_param() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    let response = client.get("/v1/coretime/info?at=invalid-block").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for invalid block parameter"
    );

    println!("ok: Coretime info invalid block parameter test passed");
    Ok(())
}

/// Test coretime/info error response for non-existent block
#[tokio::test]
async fn test_coretime_info_nonexistent_block() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/info?at=999999999").await?;

    // Should return 404 for non-existent block
    assert_eq!(
        response.status.as_u16(),
        404,
        "Should return 404 for non-existent block, got {}",
        response.status
    );

    println!("ok: Coretime info non-existent block test passed");
    Ok(())
}

/// Test coretime/info consistency across multiple requests
#[tokio::test]
async fn test_coretime_info_consistency() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // Get the latest block hash to ensure we're querying the same block
    let (_, first_response) = client.get_json("/v1/coretime/info").await?;
    let block_hash = first_response["at"]["hash"].as_str().unwrap();

    // Query the same block multiple times
    for i in 0..3 {
        let (status, json) = client
            .get_json(&format!("/v1/coretime/info?at={}", block_hash))
            .await?;

        assert!(status.is_success(), "Request {} should succeed", i + 1);

        assert_eq!(
            json["at"]["hash"].as_str().unwrap(),
            block_hash,
            "Request {} should return same block hash",
            i + 1
        );
    }

    println!("ok: Coretime info consistency test passed");
    Ok(())
}

/// Check if the connected chain is a relay chain (has Coretime pallet but not Broker)
async fn is_relay_chain(client: &TestClient) -> bool {
    if let Ok((status, json)) = client.get_json("/v1/capabilities").await {
        if status.is_success() {
            if let Some(pallets) = json["pallets"].as_array() {
                let has_coretime = pallets.iter().any(|p| p.as_str() == Some("Coretime"));
                let has_broker = pallets.iter().any(|p| p.as_str() == Some("Broker"));
                return has_coretime && !has_broker;
            }
        }
    }
    false
}

/// Test that the coretime/info endpoint returns valid JSON on relay chains
#[tokio::test]
async fn test_coretime_info_relay_chain_response() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_relay_chain(&client).await {
        println!(
            "Skipping test: Not a relay chain (Coretime pallet not found or Broker pallet present)"
        );
        return Ok(());
    }

    tracing::info!("Testing /v1/coretime/info endpoint structure on relay chain");
    let (status, json) = client.get_json("/v1/coretime/info").await?;

    assert!(
        status.is_success(),
        "Coretime info endpoint should return success status on relay chain, got {}",
        status
    );

    // Verify response has required fields
    assert!(json.get("at").is_some(), "Response should have 'at' field");

    // Verify 'at' structure
    let at = &json["at"];
    assert!(at["hash"].is_string(), "'at.hash' should be a string");
    assert!(at["height"].is_string(), "'at.height' should be a string");

    // Check for relay-chain specific fields (any of these may be present)
    let has_relay_fields = json.get("brokerId").is_some()
        || json.get("palletVersion").is_some()
        || json.get("maxHistoricalRevenue").is_some();

    if has_relay_fields {
        // If brokerId is present, verify it's a string
        if let Some(broker_id) = json.get("brokerId") {
            if !broker_id.is_null() {
                assert!(
                    broker_id.is_string(),
                    "'brokerId' should be a string when present"
                );
            }
        }

        // If palletVersion is present, verify it's a string
        if let Some(version) = json.get("palletVersion") {
            if !version.is_null() {
                assert!(
                    version.is_string(),
                    "'palletVersion' should be a string when present"
                );
            }
        }

        // If maxHistoricalRevenue is present, verify it's a string
        if let Some(revenue) = json.get("maxHistoricalRevenue") {
            if !revenue.is_null() {
                assert!(
                    revenue.is_string(),
                    "'maxHistoricalRevenue' should be a string when present"
                );
            }
        }
    }

    println!("ok: Coretime info relay chain response test passed");
    Ok(())
}
