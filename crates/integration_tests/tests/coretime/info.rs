// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the /v1/coretime/info endpoint.

use super::{init_tracing, is_coretime_chain, setup_client};
use anyhow::Result;

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

