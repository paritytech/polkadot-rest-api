//! Integration tests for /rc/accounts/{accountId}/staking-payouts endpoint
use anyhow::{Context, Result};
use colored::Colorize;
use integration_tests::{client::TestClient, constants::API_READY_TIMEOUT_SECONDS};
use std::env;
use std::sync::OnceLock;

static CLIENT: OnceLock<TestClient> = OnceLock::new();

async fn get_client() -> Result<TestClient> {
    let client = CLIENT.get_or_init(|| {
        init_tracing();
        let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
        TestClient::new(api_url)
    });

    client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    Ok(client.clone())
}

/// Helper to check if error indicates relay chain not available
fn is_relay_chain_not_available(json: &serde_json::Value) -> bool {
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        return error_str.contains("Relay chain not available");
    }
    false
}

/// Helper to check if error indicates staking pallet not available or no active era
fn is_staking_unavailable(json: &serde_json::Value) -> bool {
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        return error_str.contains("staking pallet")
            || error_str.contains("No active era")
            || error_str.contains("Staking");
    }
    false
}

#[tokio::test]
async fn test_rc_staking_payouts_basic() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known validator/nominator stash account ID
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/rc/accounts/{}/staking-payouts", account_id);

    println!(
        "\n{} Testing RC staking-payouts endpoint (basic)",
        "Testing".cyan().bold()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Handle relay chain not available
    if local_status.as_u16() == 400 && is_relay_chain_not_available(&local_json) {
        println!(
            "  {} Relay chain not configured (expected when connected to relay chain directly)",
            "!".yellow()
        );
        println!("{} Test skipped - no relay chain configured", "!".yellow().bold());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    // Handle staking pallet not available
    if local_status.as_u16() == 400 && is_staking_unavailable(&local_json) {
        println!(
            "  {} Staking pallet not available or no active era on relay chain",
            "!".yellow()
        );
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");

    // Validate required fields
    assert!(
        response_obj.contains_key("at"),
        "Response missing 'at' field"
    );
    assert!(
        response_obj.contains_key("erasPayouts"),
        "Response missing 'erasPayouts' field"
    );

    // Validate 'at' structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at missing 'hash' field");
    assert!(at_obj.contains_key("height"), "at missing 'height' field");

    // Validate erasPayouts is an array
    let eras_payouts = response_obj.get("erasPayouts").unwrap();
    assert!(eras_payouts.is_array(), "erasPayouts should be an array");

    println!("  {} Block: {}", "+".green(), at_obj.get("height").unwrap());
    println!(
        "  {} Eras payouts count: {}",
        "+".green(),
        eras_payouts.as_array().unwrap().len()
    );

    println!("{} RC staking-payouts basic test passed!", "+".green().bold());
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_payouts_with_depth() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let depth = 3;
    let endpoint = format!(
        "/rc/accounts/{}/staking-payouts?depth={}",
        account_id, depth
    );

    println!(
        "\n{} Testing RC staking-payouts with depth={}",
        "Testing".cyan().bold(),
        depth
    );
    println!("{}", "=".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Skip if relay chain not available
    if local_status.as_u16() == 400 && is_relay_chain_not_available(&local_json) {
        println!("  {} Relay chain not configured", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    // Skip if staking not available
    if local_status.as_u16() == 400 && is_staking_unavailable(&local_json) {
        println!("  {} Staking not available or no active era", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    let eras_payouts = response_obj.get("erasPayouts").unwrap().as_array().unwrap();

    // When depth=3, we should get up to 3 eras (might be less if history is limited)
    assert!(
        eras_payouts.len() <= depth as usize,
        "Should return at most {} eras",
        depth
    );

    println!("  {} Eras returned: {}", "+".green(), eras_payouts.len());

    println!(
        "{} RC staking-payouts with depth passed!",
        "+".green().bold()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_payouts_unclaimed_only_false() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!(
        "/rc/accounts/{}/staking-payouts?unclaimedOnly=false",
        account_id
    );

    println!(
        "\n{} Testing RC staking-payouts with unclaimedOnly=false",
        "Testing".cyan().bold()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Skip if relay chain not available
    if local_status.as_u16() == 400 && is_relay_chain_not_available(&local_json) {
        println!("  {} Relay chain not configured", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    // Skip if staking not available
    if local_status.as_u16() == 400 && is_staking_unavailable(&local_json) {
        println!("  {} Staking not available or no active era", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    assert!(
        response_obj.contains_key("erasPayouts"),
        "Response should have erasPayouts"
    );

    println!("  {} unclaimedOnly=false accepted", "+".green());

    println!(
        "{} RC staking-payouts with unclaimedOnly=false passed!",
        "+".green().bold()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_payouts_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = "invalid-address-123";
    let endpoint = format!("/rc/accounts/{}/staking-payouts", invalid_address);

    println!(
        "\n{} Testing RC staking-payouts with invalid address",
        "Testing".cyan().bold()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert_eq!(
        local_status.as_u16(),
        400,
        "Expected 400 Bad Request for invalid address"
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    assert!(
        response_obj.contains_key("error"),
        "Error response should contain 'error' field"
    );

    let error_msg = response_obj.get("error").unwrap().as_str().unwrap();
    assert!(
        error_msg.contains("Invalid") || error_msg.contains("address"),
        "Error message should mention invalid address"
    );

    println!("  {} Error: {}", "+".green(), error_msg);

    println!(
        "{} Invalid address error handled correctly!",
        "+".green().bold()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_payouts_response_structure() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/rc/accounts/{}/staking-payouts", account_id);

    println!(
        "\n{} Testing RC staking-payouts response structure",
        "Testing".cyan().bold()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Skip if relay chain not available
    if local_status.as_u16() == 400 && is_relay_chain_not_available(&local_json) {
        println!("  {} Relay chain not configured", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    // Skip if staking not available
    if local_status.as_u16() == 400 && is_staking_unavailable(&local_json) {
        println!("  {} Staking not available or no active era", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");

    // Validate required fields
    assert!(
        response_obj.contains_key("at"),
        "Response missing 'at' field"
    );
    assert!(
        response_obj.contains_key("erasPayouts"),
        "Response missing 'erasPayouts' field"
    );

    // Validate 'at' object structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at.hash is required");
    assert!(at_obj.contains_key("height"), "at.height is required");

    // Validate 'erasPayouts' is an array
    let eras_payouts = response_obj.get("erasPayouts").unwrap();
    assert!(eras_payouts.is_array(), "erasPayouts should be an array");

    // Validate era payout structure (if there are any entries)
    let eras_array = eras_payouts.as_array().unwrap();
    for era_payout in eras_array {
        // Era payout can be either an object with data or an object with message
        if let Some(era_obj) = era_payout.as_object() {
            // Check if it's a message variant
            if era_obj.contains_key("message") {
                println!("  {} Era message: {}", "+".green(), era_obj.get("message").unwrap());
                continue;
            }

            // Otherwise it should be the Payouts variant
            assert!(
                era_obj.contains_key("era"),
                "Era payout missing 'era' field"
            );
            assert!(
                era_obj.contains_key("totalEraRewardPoints"),
                "Era payout missing 'totalEraRewardPoints' field"
            );
            assert!(
                era_obj.contains_key("totalEraPayout"),
                "Era payout missing 'totalEraPayout' field"
            );
            assert!(
                era_obj.contains_key("payouts"),
                "Era payout missing 'payouts' field"
            );

            // Validate payouts array structure
            let payouts = era_obj.get("payouts").unwrap();
            assert!(payouts.is_array(), "payouts should be an array");

            for payout in payouts.as_array().unwrap() {
                let payout_obj = payout.as_object().expect("Payout should be an object");
                assert!(
                    payout_obj.contains_key("validatorId"),
                    "Payout missing 'validatorId'"
                );
                assert!(
                    payout_obj.contains_key("nominatorStakingPayout"),
                    "Payout missing 'nominatorStakingPayout'"
                );
                assert!(
                    payout_obj.contains_key("claimed"),
                    "Payout missing 'claimed'"
                );
                assert!(
                    payout_obj.contains_key("totalValidatorRewardPoints"),
                    "Payout missing 'totalValidatorRewardPoints'"
                );
                assert!(
                    payout_obj.contains_key("validatorCommission"),
                    "Payout missing 'validatorCommission'"
                );
                assert!(
                    payout_obj.contains_key("totalValidatorExposure"),
                    "Payout missing 'totalValidatorExposure'"
                );
                assert!(
                    payout_obj.contains_key("nominatorExposure"),
                    "Payout missing 'nominatorExposure'"
                );
            }
        }
    }

    // RC endpoint should NOT have useRcBlock-related fields
    assert!(
        !response_obj.contains_key("rcBlockHash"),
        "RC endpoint should not have rcBlockHash"
    );
    assert!(
        !response_obj.contains_key("rcBlockNumber"),
        "RC endpoint should not have rcBlockNumber"
    );
    assert!(
        !response_obj.contains_key("ahTimestamp"),
        "RC endpoint should not have ahTimestamp"
    );

    println!("{} Response structure validated!", "+".green().bold());
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_payouts_invalid_depth() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    // depth=0 should be invalid
    let endpoint = format!("/rc/accounts/{}/staking-payouts?depth=0", account_id);

    println!(
        "\n{} Testing RC staking-payouts with invalid depth=0",
        "Testing".cyan().bold()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Skip if relay chain not available
    if local_status.as_u16() == 400 && is_relay_chain_not_available(&local_json) {
        println!("  {} Relay chain not configured", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    // depth=0 should return an error
    if local_status.as_u16() == 400 {
        let response_obj = local_json.as_object().expect("Response is not an object");
        if let Some(error) = response_obj.get("error") {
            let error_str = error.as_str().unwrap_or("");
            if error_str.contains("Depth") || error_str.contains("depth") {
                println!("  {} Invalid depth error: {}", "+".green(), error_str);
                println!(
                    "{} Invalid depth handled correctly!",
                    "+".green().bold()
                );
                println!("{}", "=".repeat(80).bright_white());
                return Ok(());
            }
        }
    }

    // If staking is not available, that's also an acceptable error
    if local_status.as_u16() == 400 && is_staking_unavailable(&local_json) {
        println!("  {} Staking not available", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    // If we got here and status is 400, it should be a depth error
    assert_eq!(
        local_status.as_u16(),
        400,
        "Expected 400 Bad Request for depth=0"
    );

    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_payouts_hex_address() -> Result<()> {
    let local_client = get_client().await?;

    // Hex address (32 bytes = 64 hex chars)
    let hex_address = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = format!("/rc/accounts/{}/staking-payouts", hex_address);

    println!(
        "\n{} Testing RC staking-payouts with hex address",
        "Testing".cyan().bold()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Skip if relay chain not available
    if local_status.as_u16() == 400 && is_relay_chain_not_available(&local_json) {
        println!("  {} Relay chain not configured", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    // Skip if staking not available (hex address might not be a nominator)
    if local_status.as_u16() == 400 && is_staking_unavailable(&local_json) {
        println!("  {} Staking not available or no active era (hex address accepted)", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    assert!(response_obj.contains_key("at"), "Response should have 'at'");
    assert!(
        response_obj.contains_key("erasPayouts"),
        "Response should have 'erasPayouts'"
    );

    println!("  {} Hex address accepted", "+".green());

    println!(
        "{} RC staking-payouts with hex address passed!",
        "+".green().bold()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
