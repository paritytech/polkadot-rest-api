// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the /v1/coretime/overview endpoint.

use super::{init_tracing, is_coretime_chain, setup_client};
use anyhow::Result;

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
