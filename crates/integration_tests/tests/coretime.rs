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
    let until = lease["until"].as_u64().unwrap();
    assert!(until > 0, "'until' should be a positive timeslice value");

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
        let core = lease.get("core").and_then(|c| c.as_u64());

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

// ============================================================================
// Renewals Response Structure Tests
// ============================================================================

/// Test that the coretime/renewals endpoint returns valid JSON with correct structure
#[tokio::test]
async fn test_coretime_renewals_response_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    tracing::info!("Testing /v1/coretime/renewals endpoint structure");
    let (status, json) = client.get_json("/v1/coretime/renewals").await?;

    assert!(
        status.is_success(),
        "Coretime renewals endpoint should return success status, got {}",
        status
    );

    // Verify response has required fields
    assert!(json.get("at").is_some(), "Response should have 'at' field");
    assert!(
        json.get("renewals").is_some(),
        "Response should have 'renewals' field"
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

    // Verify renewals is an array
    assert!(json["renewals"].is_array(), "'renewals' should be an array");

    println!("ok: Coretime renewals response structure test passed");
    Ok(())
}

/// Test renewal item structure when renewals are present
#[tokio::test]
async fn test_coretime_renewals_item_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/renewals").await?;
    assert!(status.is_success());

    let renewals = json["renewals"].as_array().unwrap();

    if renewals.is_empty() {
        println!("Skipping item structure test: No renewals present on chain");
        return Ok(());
    }

    // Check first renewal item structure
    let renewal = &renewals[0];

    // Required fields
    assert!(
        renewal.get("core").is_some(),
        "Renewal should have 'core' field"
    );
    assert!(
        renewal.get("when").is_some(),
        "Renewal should have 'when' field"
    );
    assert!(
        renewal.get("task").is_some(),
        "Renewal should have 'task' field"
    );

    assert!(renewal["core"].is_number(), "'core' should be a number");
    assert!(renewal["when"].is_number(), "'when' should be a number");
    assert!(renewal["task"].is_string(), "'task' should be a string");

    // Optional fields (if present should have correct types)
    if let Some(completion) = renewal.get("completion") {
        if !completion.is_null() {
            assert!(
                completion.is_string(),
                "'completion' should be a string when present"
            );
            let completion_str = completion.as_str().unwrap();
            assert!(
                completion_str == "Complete" || completion_str == "Partial",
                "'completion' should be 'Complete' or 'Partial', got: {}",
                completion_str
            );
        }
    }

    if let Some(mask) = renewal.get("mask") {
        if !mask.is_null() {
            assert!(mask.is_string(), "'mask' should be a string when present");
            let mask_str = mask.as_str().unwrap();
            assert!(
                mask_str.starts_with("0x"),
                "'mask' should be a hex string starting with 0x"
            );
            // CoreMask is 80 bits = 10 bytes = 20 hex chars + "0x" prefix
            assert_eq!(
                mask_str.len(),
                22,
                "'mask' should be 22 characters (0x + 20 hex digits for 10 bytes)"
            );
        }
    }

    if let Some(price) = renewal.get("price") {
        if !price.is_null() {
            assert!(price.is_string(), "'price' should be a string when present");
            let price_str = price.as_str().unwrap();
            assert!(
                price_str.parse::<u128>().is_ok(),
                "'price' should be a numeric string, got: {}",
                price_str
            );
        }
    }

    // Validate task is empty, "Pool", "Idle", or a numeric string (task ID)
    let task = renewal["task"].as_str().unwrap();
    assert!(
        task.is_empty() || task == "Pool" || task == "Idle" || task.parse::<u32>().is_ok(),
        "'task' should be empty, 'Pool', 'Idle', or a numeric task ID, got: {}",
        task
    );

    // Validate core and when are positive
    let core = renewal["core"].as_u64().unwrap();
    let when = renewal["when"].as_u64().unwrap();
    assert!(when > 0, "'when' should be a positive timeslice value");

    println!(
        "ok: Coretime renewals item structure test passed ({} renewals found, first core: {})",
        renewals.len(),
        core
    );
    Ok(())
}

// ============================================================================
// Renewals Query Parameter Tests
// ============================================================================

/// Test the 'at' query parameter with a block number for renewals
#[tokio::test]
async fn test_coretime_renewals_at_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to find a valid block number
    let (_, latest_json) = client.get_json("/v1/coretime/renewals").await?;
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
        .get_json(&format!("/v1/coretime/renewals?at={}", query_height))
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

    println!("ok: Coretime renewals 'at' block number test passed");
    Ok(())
}

/// Test the 'at' query parameter with a block hash for renewals
#[tokio::test]
async fn test_coretime_renewals_at_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to get a valid block hash
    let (_, latest_json) = client.get_json("/v1/coretime/renewals").await?;
    let block_hash = latest_json["at"]["hash"].as_str().unwrap();

    let (status, json) = client
        .get_json(&format!("/v1/coretime/renewals?at={}", block_hash))
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

    println!("ok: Coretime renewals 'at' block hash test passed");
    Ok(())
}

// ============================================================================
// Renewals Error Handling Tests
// ============================================================================

/// Test error response for non-coretime chains (chains without Broker pallet) for renewals
#[tokio::test]
async fn test_coretime_renewals_non_coretime_chain() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Only run this test on non-coretime chains
    if is_coretime_chain(&client).await {
        println!("Skipping test: This is a coretime chain, need non-coretime chain for this test");
        return Ok(());
    }

    let response = client.get("/v1/coretime/renewals").await?;

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

    println!("ok: Coretime renewals non-coretime chain error test passed");
    Ok(())
}

/// Test error response for invalid block parameter for renewals
#[tokio::test]
async fn test_coretime_renewals_invalid_block_param() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    let response = client.get("/v1/coretime/renewals?at=invalid-block").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for invalid block parameter"
    );

    println!("ok: Coretime renewals invalid block parameter test passed");
    Ok(())
}

/// Test error response for non-existent block (very high block number) for renewals
#[tokio::test]
async fn test_coretime_renewals_nonexistent_block() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/renewals?at=999999999").await?;

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

    println!("ok: Coretime renewals non-existent block test passed");
    Ok(())
}

/// Test error response for very large block numbers that cause RPC errors for renewals
#[tokio::test]
async fn test_coretime_renewals_very_large_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    let response = client.get("/v1/coretime/renewals?at=999999999999").await?;

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

    println!("ok: Coretime renewals very large block number test passed");
    Ok(())
}

/// Test error response for invalid block hash format for renewals
#[tokio::test]
async fn test_coretime_renewals_invalid_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Invalid hex string (not 32 bytes)
    let response = client.get("/v1/coretime/renewals?at=0xabc123").await?;

    // Should return 400 or 404 for invalid block hash format
    assert!(
        response.status.as_u16() == 400 || response.status.as_u16() == 404,
        "Should return 400 or 404 for invalid block hash format, got {}",
        response.status
    );

    println!("ok: Coretime renewals invalid block hash test passed");
    Ok(())
}

// ============================================================================
// Renewals Consistency Tests
// ============================================================================

/// Test that multiple requests return consistent data for renewals
#[tokio::test]
async fn test_coretime_renewals_consistency() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // Get the latest block hash to ensure we're querying the same block
    let (_, first_response) = client.get_json("/v1/coretime/renewals").await?;
    let block_hash = first_response["at"]["hash"].as_str().unwrap();

    // Query the same block multiple times
    for i in 0..3 {
        let (status, json) = client
            .get_json(&format!("/v1/coretime/renewals?at={}", block_hash))
            .await?;

        assert!(status.is_success(), "Request {} should succeed", i + 1);

        assert_eq!(
            json["at"]["hash"].as_str().unwrap(),
            block_hash,
            "Request {} should return same block hash",
            i + 1
        );

        assert_eq!(
            json["renewals"].as_array().map(|a| a.len()),
            first_response["renewals"].as_array().map(|a| a.len()),
            "Request {} should return same number of renewals",
            i + 1
        );
    }

    println!("ok: Coretime renewals consistency test passed");
    Ok(())
}

/// Test that renewals are sorted by core ID
#[tokio::test]
async fn test_coretime_renewals_sorting() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/renewals").await?;
    assert!(status.is_success());

    let renewals = json["renewals"].as_array().unwrap();

    if renewals.len() < 2 {
        println!("Skipping sorting test: Need at least 2 renewals to verify sorting");
        return Ok(());
    }

    // Check that renewals are sorted by core ID (ascending)
    let mut last_core: Option<u64> = None;

    for renewal in renewals {
        let core = renewal["core"].as_u64().unwrap();

        if let Some(last) = last_core {
            assert!(
                core >= last,
                "Renewals should be sorted by core ID (ascending): {} should come after {}",
                core,
                last
            );
        }

        last_core = Some(core);
    }

    println!("ok: Coretime renewals sorting test passed");
    Ok(())
}
