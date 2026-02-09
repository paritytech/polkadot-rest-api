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

/// Test error response for invalid block parameter for renewals
#[tokio::test]
async fn test_coretime_renewals_invalid_block_param() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

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

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/renewals?at=999999999").await?;

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

    println!("ok: Coretime renewals non-existent block test passed");
    Ok(())
}

/// Test error response for very large block numbers that cause RPC errors for renewals
#[tokio::test]
async fn test_coretime_renewals_very_large_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    let response = client.get("/v1/coretime/renewals?at=999999999999").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for very large block number, got {}",
        response.status
    );

    println!("ok: Coretime renewals very large block number test passed");
    Ok(())
}

/// Test error response for invalid block hash format for renewals
#[tokio::test]
async fn test_coretime_renewals_invalid_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain");
        return Ok(());
    }

    // Invalid hex string (not 32 bytes)
    let response = client.get("/v1/coretime/renewals?at=0xabc123").await?;

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

    println!("ok: Coretime regions sorting test passed");
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

            // Verify values are numbers (u32 fields)
            assert!(
                config["regionLength"].is_number(),
                "'regionLength' should be a number"
            );
            assert!(
                config["relayBlocksPerTimeslice"].is_number(),
                "'relayBlocksPerTimeslice' should be a number"
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

            // Verify types - u32 fields are numbers, u128 (Balance) fields are strings
            assert!(
                cores["available"].is_number(),
                "'available' should be a number"
            );
            assert!(cores["sold"].is_number(), "'sold' should be a number");
            assert!(cores["total"].is_number(), "'total' should be a number");
            assert!(
                cores["currentCorePrice"].is_string(),
                "'currentCorePrice' should be a string (u128 Balance)"
            );

            // Verify logical constraints
            let available = cores["available"].as_u64().unwrap();
            let sold = cores["sold"].as_u64().unwrap();
            let total = cores["total"].as_u64().unwrap();
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

    // Coretime routes only exist on relay and coretime chains
    if !is_coretime_chain(&client).await && !is_relay_chain(&client).await {
        println!("Skipping test: No coretime routes on this chain type");
        return Ok(());
    }

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

    // Coretime routes only exist on relay and coretime chains
    if !is_coretime_chain(&client).await && !is_relay_chain(&client).await {
        println!("Skipping test: No coretime routes on this chain type");
        return Ok(());
    }

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/info?at=999999999").await?;

    // Should return 400 for non-existent block (block number larger than chain height)
    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for non-existent block, got {}",
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
        || json.get("storageVersion").is_some()
        || json.get("maxHistoricalRevenue").is_some();

    if has_relay_fields {
        // If brokerId is present, verify it's a number (u32)
        if let Some(broker_id) = json.get("brokerId") {
            if !broker_id.is_null() {
                assert!(
                    broker_id.is_number(),
                    "'brokerId' should be a number when present"
                );
            }
        }

        // If storageVersion is present, verify it's a number (u16)
        if let Some(version) = json.get("storageVersion") {
            if !version.is_null() {
                assert!(
                    version.is_number(),
                    "'storageVersion' should be a number when present"
                );
            }
        }

        // If maxHistoricalRevenue is present, verify it's a number (u32)
        if let Some(revenue) = json.get("maxHistoricalRevenue") {
            if !revenue.is_null() {
                assert!(
                    revenue.is_number(),
                    "'maxHistoricalRevenue' should be a number when present"
                );
            }
        }
    }

    println!("ok: Coretime info relay chain response test passed");
    Ok(())
}

// ============================================================================
// Overview Response Structure Tests
// ============================================================================

/// Test that the coretime/overview endpoint returns valid JSON with correct structure
#[tokio::test]
async fn test_coretime_overview_response_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    tracing::info!("Testing /v1/coretime/overview endpoint structure");
    let (status, json) = client.get_json("/v1/coretime/overview").await?;

    assert!(
        status.is_success(),
        "Coretime overview endpoint should return success status, got {}",
        status
    );

    // Verify response has required fields
    assert!(json.get("at").is_some(), "Response should have 'at' field");
    assert!(
        json.get("cores").is_some(),
        "Response should have 'cores' field"
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

    // Verify cores is an array
    assert!(json["cores"].is_array(), "'cores' should be an array");

    println!("ok: Coretime overview response structure test passed");
    Ok(())
}

/// Test core item structure when cores are present
#[tokio::test]
async fn test_coretime_overview_item_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/overview").await?;
    assert!(status.is_success());

    let cores = json["cores"].as_array().unwrap();

    if cores.is_empty() {
        println!("Skipping item structure test: No cores present on chain");
        return Ok(());
    }

    // Check first core item structure
    let core = &cores[0];

    // Required fields
    assert!(
        core.get("coreId").is_some(),
        "Core should have 'coreId' field"
    );
    assert!(
        core.get("paraId").is_some(),
        "Core should have 'paraId' field"
    );
    assert!(
        core.get("workload").is_some(),
        "Core should have 'workload' field"
    );
    assert!(
        core.get("workplan").is_some(),
        "Core should have 'workplan' field"
    );
    assert!(core.get("type").is_some(), "Core should have 'type' field");
    assert!(
        core.get("regions").is_some(),
        "Core should have 'regions' field"
    );

    assert!(core["coreId"].is_number(), "'coreId' should be a number");
    assert!(core["paraId"].is_string(), "'paraId' should be a string");
    assert!(
        core["workload"].is_object(),
        "'workload' should be an object"
    );
    assert!(core["workplan"].is_array(), "'workplan' should be an array");
    assert!(core["type"].is_object(), "'type' should be an object");
    assert!(core["regions"].is_array(), "'regions' should be an array");

    // Validate workload structure
    let workload = &core["workload"];
    assert!(
        workload.get("isPool").is_some(),
        "Workload should have 'isPool' field"
    );
    assert!(
        workload.get("isTask").is_some(),
        "Workload should have 'isTask' field"
    );
    assert!(
        workload.get("mask").is_some(),
        "Workload should have 'mask' field"
    );
    assert!(
        workload.get("task").is_some(),
        "Workload should have 'task' field"
    );

    assert!(
        workload["isPool"].is_boolean(),
        "'workload.isPool' should be a boolean"
    );
    assert!(
        workload["isTask"].is_boolean(),
        "'workload.isTask' should be a boolean"
    );
    assert!(
        workload["mask"].is_string(),
        "'workload.mask' should be a string"
    );
    assert!(
        workload["task"].is_string(),
        "'workload.task' should be a string"
    );

    // Validate mask format (should start with 0x, 10 bytes = 20 hex chars)
    let mask = workload["mask"].as_str().unwrap();
    assert!(
        mask.starts_with("0x"),
        "'workload.mask' should be a hex string starting with 0x"
    );
    assert_eq!(
        mask.len(),
        22,
        "'workload.mask' should be 22 characters (0x + 20 hex digits for 10 bytes)"
    );

    // Validate type structure
    let core_type = &core["type"];
    assert!(
        core_type.get("condition").is_some(),
        "Type should have 'condition' field"
    );
    assert!(
        core_type["condition"].is_string(),
        "'type.condition' should be a string"
    );

    let condition = core_type["condition"].as_str().unwrap();
    assert!(
        condition == "lease"
            || condition == "bulk"
            || condition == "reservation"
            || condition == "ondemand",
        "'type.condition' should be 'lease', 'bulk', 'reservation', or 'ondemand', got: {}",
        condition
    );

    println!(
        "ok: Coretime overview item structure test passed ({} cores found)",
        cores.len()
    );
    Ok(())
}

// ============================================================================
// Overview Query Parameter Tests
// ============================================================================

/// Test the 'at' query parameter with a block number for overview
#[tokio::test]
async fn test_coretime_overview_at_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to find a valid block number
    let (_, latest_json) = client.get_json("/v1/coretime/overview").await?;
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
        .get_json(&format!("/v1/coretime/overview?at={}", query_height))
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

    println!("ok: Coretime overview 'at' block number test passed");
    Ok(())
}

/// Test the 'at' query parameter with a block hash for overview
#[tokio::test]
async fn test_coretime_overview_at_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // First get the latest block to get a valid block hash
    let (_, latest_json) = client.get_json("/v1/coretime/overview").await?;
    let block_hash = latest_json["at"]["hash"].as_str().unwrap();

    let (status, json) = client
        .get_json(&format!("/v1/coretime/overview?at={}", block_hash))
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

    println!("ok: Coretime overview 'at' block hash test passed");
    Ok(())
}

// ============================================================================
// Overview Error Handling Tests
// ============================================================================

/// Test error response for invalid block parameter for overview
#[tokio::test]
async fn test_coretime_overview_invalid_block_param() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Coretime routes only exist on relay and coretime chains
    if !is_coretime_chain(&client).await && !is_relay_chain(&client).await {
        println!("Skipping test: No coretime routes on this chain type");
        return Ok(());
    }

    let response = client.get("/v1/coretime/overview?at=invalid-block").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for invalid block parameter"
    );

    println!("ok: Coretime overview invalid block parameter test passed");
    Ok(())
}

/// Test error response for non-existent block (very high block number) for overview
#[tokio::test]
async fn test_coretime_overview_nonexistent_block() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Coretime routes only exist on relay and coretime chains
    if !is_coretime_chain(&client).await && !is_relay_chain(&client).await {
        println!("Skipping test: No coretime routes on this chain type");
        return Ok(());
    }

    // Use a very high block number that doesn't exist
    let response = client.get("/v1/coretime/overview?at=999999999").await?;

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

    println!("ok: Coretime overview non-existent block test passed");
    Ok(())
}

/// Test error response for very large block numbers that cause RPC errors for overview
#[tokio::test]
async fn test_coretime_overview_very_large_block_number() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Coretime routes only exist on relay and coretime chains
    if !is_coretime_chain(&client).await && !is_relay_chain(&client).await {
        println!("Skipping test: No coretime routes on this chain type");
        return Ok(());
    }

    let response = client.get("/v1/coretime/overview?at=999999999999").await?;

    assert_eq!(
        response.status.as_u16(),
        400,
        "Should return 400 for very large block number, got {}",
        response.status
    );

    println!("ok: Coretime overview very large block number test passed");
    Ok(())
}

/// Test error response for invalid block hash format for overview
#[tokio::test]
async fn test_coretime_overview_invalid_block_hash() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    // Invalid hex string (not 32 bytes)
    let response = client.get("/v1/coretime/overview?at=0xabc123").await?;

    // Should return 400 or 404 for invalid block hash format
    assert!(
        response.status.as_u16() == 400 || response.status.as_u16() == 404,
        "Should return 400 or 404 for invalid block hash format, got {}",
        response.status
    );

    println!("ok: Coretime overview invalid block hash test passed");
    Ok(())
}

// ============================================================================
// Overview Consistency Tests
// ============================================================================

/// Test that multiple requests return consistent data for overview
#[tokio::test]
async fn test_coretime_overview_consistency() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // Get the latest block hash to ensure we're querying the same block
    let (_, first_response) = client.get_json("/v1/coretime/overview").await?;
    let block_hash = first_response["at"]["hash"].as_str().unwrap();

    // Query the same block multiple times
    for i in 0..3 {
        let (status, json) = client
            .get_json(&format!("/v1/coretime/overview?at={}", block_hash))
            .await?;

        assert!(status.is_success(), "Request {} should succeed", i + 1);

        assert_eq!(
            json["at"]["hash"].as_str().unwrap(),
            block_hash,
            "Request {} should return same block hash",
            i + 1
        );

        assert_eq!(
            json["cores"].as_array().map(|a| a.len()),
            first_response["cores"].as_array().map(|a| a.len()),
            "Request {} should return same number of cores",
            i + 1
        );
    }

    println!("ok: Coretime overview consistency test passed");
    Ok(())
}

/// Test that cores are sorted by core ID
#[tokio::test]
async fn test_coretime_overview_sorting() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/overview").await?;
    assert!(status.is_success());

    let cores = json["cores"].as_array().unwrap();

    if cores.len() < 2 {
        println!("Skipping sorting test: Need at least 2 cores to verify sorting");
        return Ok(());
    }

    // Check that cores are sorted by core ID (ascending)
    let mut last_core_id: Option<u64> = None;

    for core in cores {
        let core_id = core["coreId"].as_u64().unwrap();

        if let Some(last) = last_core_id {
            assert!(
                core_id >= last,
                "Cores should be sorted by coreId (ascending): {} should come after {}",
                core_id,
                last
            );
        }

        last_core_id = Some(core_id);
    }

    println!("ok: Coretime overview sorting test passed");
    Ok(())
}

// ============================================================================
// Overview Data Consistency Tests
// ============================================================================

/// Test that overview aggregates data correctly from other endpoints
#[tokio::test]
async fn test_coretime_overview_data_aggregation() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    // Get overview at specific block
    let (_, overview_json) = client.get_json("/v1/coretime/overview").await?;
    let block_hash = overview_json["at"]["hash"].as_str().unwrap();

    // Get individual endpoints at the same block for comparison
    let (_, leases_json) = client
        .get_json(&format!("/v1/coretime/leases?at={}", block_hash))
        .await?;
    let (_, reservations_json) = client
        .get_json(&format!("/v1/coretime/reservations?at={}", block_hash))
        .await?;
    let (_, regions_json) = client
        .get_json(&format!("/v1/coretime/regions?at={}", block_hash))
        .await?;

    let cores = overview_json["cores"].as_array().unwrap();
    let leases = leases_json["leases"].as_array().unwrap();
    let reservations = reservations_json["reservations"].as_array().unwrap();
    let regions = regions_json["regions"].as_array().unwrap();

    // Count cores by type in overview
    let mut lease_count = 0;
    let mut reservation_count = 0;
    let mut ondemand_count = 0;
    let mut bulk_count = 0;

    for core in cores {
        match core["type"]["condition"].as_str().unwrap() {
            "lease" => lease_count += 1,
            "reservation" => reservation_count += 1,
            "ondemand" => ondemand_count += 1,
            "bulk" => bulk_count += 1,
            _ => {}
        }
    }

    // Verify counts are reasonable (overview should see same or more cores than individual endpoints)
    // Note: The counts may not match exactly due to the classification logic
    println!(
        "Overview: {} cores ({} lease, {} reservation, {} ondemand, {} bulk)",
        cores.len(),
        lease_count,
        reservation_count,
        ondemand_count,
        bulk_count
    );
    println!(
        "Individual endpoints: {} leases, {} reservations, {} regions",
        leases.len(),
        reservations.len(),
        regions.len()
    );

    // Basic sanity checks
    assert!(
        cores.len() > 0 || (leases.is_empty() && reservations.is_empty()),
        "If there are leases or reservations, there should be cores in overview"
    );

    println!("ok: Coretime overview data aggregation test passed");
    Ok(())
}

/// Test that workplan entries have correct structure
#[tokio::test]
async fn test_coretime_overview_workplan_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    if !is_coretime_chain(&client).await {
        println!("Skipping test: Not a coretime chain (Broker pallet not found)");
        return Ok(());
    }

    let (status, json) = client.get_json("/v1/coretime/overview").await?;
    assert!(status.is_success());

    let cores = json["cores"].as_array().unwrap();

    // Find a core with workplan entries
    let core_with_workplan = cores.iter().find(|c| {
        c["workplan"]
            .as_array()
            .map(|arr| !arr.is_empty())
            .unwrap_or(false)
    });

    if let Some(core) = core_with_workplan {
        let workplan = core["workplan"].as_array().unwrap();
        let entry = &workplan[0];

        // Verify workplan entry structure
        assert!(
            entry.get("core").is_some(),
            "Workplan entry should have 'core' field"
        );
        assert!(
            entry.get("timeslice").is_some(),
            "Workplan entry should have 'timeslice' field"
        );
        assert!(
            entry.get("info").is_some(),
            "Workplan entry should have 'info' field"
        );

        assert!(
            entry["core"].is_number(),
            "'workplan.core' should be a number"
        );
        assert!(
            entry["timeslice"].is_number(),
            "'workplan.timeslice' should be a number"
        );
        assert!(
            entry["info"].is_array(),
            "'workplan.info' should be an array"
        );

        // If info array is not empty, check its structure
        if let Some(info_array) = entry["info"].as_array() {
            if !info_array.is_empty() {
                let info_item = &info_array[0];
                assert!(
                    info_item.get("isPool").is_some(),
                    "Workplan info should have 'isPool' field"
                );
                assert!(
                    info_item.get("isTask").is_some(),
                    "Workplan info should have 'isTask' field"
                );
                assert!(
                    info_item.get("mask").is_some(),
                    "Workplan info should have 'mask' field"
                );
                assert!(
                    info_item.get("task").is_some(),
                    "Workplan info should have 'task' field"
                );
            }
        }

        println!(
            "ok: Coretime overview workplan structure test passed ({} workplan entries in first matching core)",
            workplan.len()
        );
    } else {
        println!("Skipping workplan structure test: No cores have workplan entries");
    }

    Ok(())
}
