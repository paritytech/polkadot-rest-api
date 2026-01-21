//! Integration tests for staking-payouts endpoints
//!
//! Tests both:
//! - `/accounts/{accountId}/staking-payouts` (standard endpoint)
//! - `/rc/accounts/{accountId}/staking-payouts` (relay chain endpoint)

use anyhow::{Context, Result};
use colored::Colorize;
use integration_tests::{client::TestClient, constants::API_READY_TIMEOUT_SECONDS};
use std::env;
use std::sync::OnceLock;

static CLIENT: OnceLock<TestClient> = OnceLock::new();

// ================================================================================================
// Test Configuration
// ================================================================================================

/// Endpoint type for parameterized testing
#[derive(Debug, Clone, Copy)]
enum EndpointType {
    /// Standard endpoint: /accounts/{accountId}/staking-payouts
    Standard,
    /// RC endpoint: /rc/accounts/{accountId}/staking-payouts
    RelayChain,
}

impl EndpointType {
    fn base_path(&self) -> &'static str {
        match self {
            EndpointType::Standard => "/accounts",
            EndpointType::RelayChain => "/rc/accounts",
        }
    }

    fn name(&self) -> &'static str {
        match self {
            EndpointType::Standard => "standard",
            EndpointType::RelayChain => "RC",
        }
    }

    fn build_endpoint(&self, account_id: &str, query: Option<&str>) -> String {
        let base = format!("{}/{}/staking-payouts", self.base_path(), account_id);
        match query {
            Some(q) => format!("{}?{}", base, q),
            None => base,
        }
    }
}

// ================================================================================================
// Test Helpers
// ================================================================================================

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

/// Check if error indicates relay chain not available (only relevant for RC endpoint)
fn is_relay_chain_not_available(json: &serde_json::Value) -> bool {
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        return error_str.contains("Relay chain not available");
    }
    false
}

/// Check if error indicates staking pallet not available or no active era
fn is_staking_unavailable(json: &serde_json::Value) -> bool {
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        return error_str.contains("staking pallet")
            || error_str.contains("No active era")
            || error_str.contains("Staking");
    }
    false
}

/// Check if error indicates depth is invalid
fn is_depth_error(json: &serde_json::Value) -> bool {
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        return error_str.to_lowercase().contains("depth");
    }
    false
}

/// Handle common skip conditions for staking endpoints
/// Returns Ok(true) if test should be skipped, Ok(false) to continue
fn should_skip_test(
    endpoint_type: EndpointType,
    status: u16,
    json: &serde_json::Value,
) -> Result<bool> {
    if status != 400 {
        return Ok(false);
    }

    // RC endpoint: skip if relay chain not available
    if matches!(endpoint_type, EndpointType::RelayChain) && is_relay_chain_not_available(json) {
        println!(
            "  {} Relay chain not configured (skipping {} test)",
            "!".yellow(),
            endpoint_type.name()
        );
        return Ok(true);
    }

    // Both endpoints: skip if staking pallet not available
    if is_staking_unavailable(json) {
        println!(
            "  {} Staking pallet not available or no active era (skipping {} test)",
            "!".yellow(),
            endpoint_type.name()
        );
        return Ok(true);
    }

    Ok(false)
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

// ================================================================================================
// Shared Test Logic
// ================================================================================================

async fn run_basic_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = endpoint_type.build_endpoint(account_id, None);

    println!(
        "\n{} Testing {} staking-payouts endpoint (basic)",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from API")?;

    if should_skip_test(endpoint_type, status.as_u16(), &json)? {
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        status.is_success(),
        "{} API returned status {}",
        endpoint_type.name(),
        status
    );

    let response_obj = json.as_object().expect("Response is not an object");

    // Validate required fields
    assert!(response_obj.contains_key("at"), "Response missing 'at' field");
    assert!(
        response_obj.contains_key("erasPayouts"),
        "Response missing 'erasPayouts' field"
    );

    // Validate 'at' structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at missing 'hash' field");
    assert!(at_obj.contains_key("height"), "at missing 'height' field");

    let eras_payouts = response_obj.get("erasPayouts").unwrap().as_array().unwrap();

    println!("  {} Block: {}", "+".green(), at_obj.get("height").unwrap());
    println!("  {} Eras payouts count: {}", "+".green(), eras_payouts.len());

    println!(
        "{} {} staking-payouts basic test passed!",
        "+".green().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

async fn run_depth_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let depth = 3;
    let endpoint = endpoint_type.build_endpoint(account_id, Some(&format!("depth={}", depth)));

    println!(
        "\n{} Testing {} staking-payouts with depth={}",
        "Testing".cyan().bold(),
        endpoint_type.name(),
        depth
    );
    println!("{}", "=".repeat(80).bright_white());

    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from API")?;

    if should_skip_test(endpoint_type, status.as_u16(), &json)? {
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        status.is_success(),
        "{} API returned status {}",
        endpoint_type.name(),
        status
    );

    let response_obj = json.as_object().expect("Response is not an object");
    let eras_payouts = response_obj.get("erasPayouts").unwrap().as_array().unwrap();

    assert!(
        eras_payouts.len() <= depth as usize,
        "Should return at most {} eras, got {}",
        depth,
        eras_payouts.len()
    );

    println!("  {} Eras returned: {}", "+".green(), eras_payouts.len());

    println!(
        "{} {} staking-payouts depth test passed!",
        "+".green().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

async fn run_unclaimed_only_false_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = endpoint_type.build_endpoint(account_id, Some("unclaimedOnly=false"));

    println!(
        "\n{} Testing {} staking-payouts with unclaimedOnly=false",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from API")?;

    if should_skip_test(endpoint_type, status.as_u16(), &json)? {
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        status.is_success(),
        "{} API returned status {}",
        endpoint_type.name(),
        status
    );

    let response_obj = json.as_object().expect("Response is not an object");
    assert!(
        response_obj.contains_key("erasPayouts"),
        "Response should have erasPayouts"
    );

    println!("  {} unclaimedOnly=false accepted", "+".green());

    println!(
        "{} {} staking-payouts unclaimedOnly test passed!",
        "+".green().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

async fn run_invalid_address_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let invalid_address = "invalid-address-123";
    let endpoint = endpoint_type.build_endpoint(invalid_address, None);

    println!(
        "\n{} Testing {} staking-payouts with invalid address",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from API")?;

    assert_eq!(
        status.as_u16(),
        400,
        "Expected 400 Bad Request for invalid address"
    );

    let response_obj = json.as_object().expect("Response is not an object");
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
        "{} {} invalid address test passed!",
        "+".green().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

async fn run_invalid_depth_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = endpoint_type.build_endpoint(account_id, Some("depth=0"));

    println!(
        "\n{} Testing {} staking-payouts with invalid depth=0",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from API")?;

    // Should return 400 for invalid depth
    if status.as_u16() == 400 {
        // Could be depth error, relay chain not available, or staking not available
        if is_relay_chain_not_available(&json) || is_staking_unavailable(&json) {
            println!(
                "  {} Relay chain or staking not available (skipping)",
                "!".yellow()
            );
            println!("{}", "=".repeat(80).bright_white());
            return Ok(());
        }

        if is_depth_error(&json) {
            let error_msg = json
                .as_object()
                .and_then(|o| o.get("error"))
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error");
            println!("  {} Invalid depth error: {}", "+".green(), error_msg);
            println!(
                "{} {} invalid depth test passed!",
                "+".green().bold(),
                endpoint_type.name()
            );
            println!("{}", "=".repeat(80).bright_white());
            return Ok(());
        }
    }

    assert_eq!(
        status.as_u16(),
        400,
        "Expected 400 Bad Request for depth=0"
    );

    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

async fn run_hex_address_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let hex_address = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = endpoint_type.build_endpoint(hex_address, None);

    println!(
        "\n{} Testing {} staking-payouts with hex address",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from API")?;

    if should_skip_test(endpoint_type, status.as_u16(), &json)? {
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        status.is_success(),
        "{} API returned status {}",
        endpoint_type.name(),
        status
    );

    let response_obj = json.as_object().expect("Response is not an object");
    assert!(response_obj.contains_key("at"), "Response should have 'at'");
    assert!(
        response_obj.contains_key("erasPayouts"),
        "Response should have 'erasPayouts'"
    );

    println!("  {} Hex address accepted", "+".green());

    println!(
        "{} {} hex address test passed!",
        "+".green().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

async fn run_response_structure_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = endpoint_type.build_endpoint(account_id, None);

    println!(
        "\n{} Testing {} staking-payouts response structure",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from API")?;

    if should_skip_test(endpoint_type, status.as_u16(), &json)? {
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        status.is_success(),
        "{} API returned status {}",
        endpoint_type.name(),
        status
    );

    let response_obj = json.as_object().expect("Response is not an object");

    // Validate required fields
    assert!(response_obj.contains_key("at"), "Response missing 'at' field");
    assert!(
        response_obj.contains_key("erasPayouts"),
        "Response missing 'erasPayouts' field"
    );

    // Validate 'at' object structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at.hash is required");
    assert!(at_obj.contains_key("height"), "at.height is required");

    // Validate erasPayouts array structure
    let eras_payouts = response_obj.get("erasPayouts").unwrap().as_array().unwrap();

    for era_payout in eras_payouts {
        if let Some(era_obj) = era_payout.as_object() {
            // Check if it's a message variant
            if era_obj.contains_key("message") {
                println!(
                    "  {} Era message: {}",
                    "+".green(),
                    era_obj.get("message").unwrap()
                );
                continue;
            }

            // Validate Payouts variant structure
            assert!(era_obj.contains_key("era"), "Era payout missing 'era' field");
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

            // Validate individual payouts
            let payouts = era_obj.get("payouts").unwrap().as_array().unwrap();
            for payout in payouts {
                let payout_obj = payout.as_object().expect("Payout should be an object");
                assert!(payout_obj.contains_key("validatorId"), "Missing 'validatorId'");
                assert!(
                    payout_obj.contains_key("nominatorStakingPayout"),
                    "Missing 'nominatorStakingPayout'"
                );
                assert!(payout_obj.contains_key("claimed"), "Missing 'claimed'");
                assert!(
                    payout_obj.contains_key("totalValidatorRewardPoints"),
                    "Missing 'totalValidatorRewardPoints'"
                );
                assert!(
                    payout_obj.contains_key("validatorCommission"),
                    "Missing 'validatorCommission'"
                );
                assert!(
                    payout_obj.contains_key("totalValidatorExposure"),
                    "Missing 'totalValidatorExposure'"
                );
                assert!(
                    payout_obj.contains_key("nominatorExposure"),
                    "Missing 'nominatorExposure'"
                );
            }
        }
    }

    // RC endpoint should NOT have useRcBlock-related fields
    assert!(
        !response_obj.contains_key("rcBlockHash"),
        "Direct query should not have rcBlockHash"
    );
    assert!(
        !response_obj.contains_key("rcBlockNumber"),
        "Direct query should not have rcBlockNumber"
    );
    assert!(
        !response_obj.contains_key("ahTimestamp"),
        "Direct query should not have ahTimestamp"
    );

    println!(
        "{} {} response structure validated!",
        "+".green().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

// ================================================================================================
// Standard Endpoint Tests
// ================================================================================================

#[tokio::test]
async fn test_standard_staking_payouts_basic() -> Result<()> {
    run_basic_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_payouts_with_depth() -> Result<()> {
    run_depth_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_payouts_unclaimed_only_false() -> Result<()> {
    run_unclaimed_only_false_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_payouts_invalid_address() -> Result<()> {
    run_invalid_address_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_payouts_invalid_depth() -> Result<()> {
    run_invalid_depth_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_payouts_hex_address() -> Result<()> {
    run_hex_address_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_payouts_response_structure() -> Result<()> {
    run_response_structure_test(EndpointType::Standard).await
}

// ================================================================================================
// RC Endpoint Tests
// ================================================================================================

#[tokio::test]
async fn test_rc_staking_payouts_basic() -> Result<()> {
    run_basic_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_payouts_with_depth() -> Result<()> {
    run_depth_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_payouts_unclaimed_only_false() -> Result<()> {
    run_unclaimed_only_false_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_payouts_invalid_address() -> Result<()> {
    run_invalid_address_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_payouts_invalid_depth() -> Result<()> {
    run_invalid_depth_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_payouts_hex_address() -> Result<()> {
    run_hex_address_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_payouts_response_structure() -> Result<()> {
    run_response_structure_test(EndpointType::RelayChain).await
}

// ================================================================================================
// Standard Endpoint Specific Tests (useRcBlock parameter)
// ================================================================================================

#[tokio::test]
async fn test_standard_staking_payouts_use_rc_block() -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let rc_block_number = 26054957;
    let endpoint = format!(
        "/accounts/{}/staking-payouts?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing standard staking-payouts with useRcBlock at RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from API")?;

    // Could be various errors: staking not available, useRcBlock not supported, etc.
    if status.as_u16() == 400 {
        let response_obj = json.as_object().unwrap();
        let error_msg = response_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("staking pallet")
            || error_msg.contains("No active era")
            || error_msg.contains("useRcBlock")
        {
            println!(
                "  {} Staking not available or useRcBlock not supported: {}",
                "!".yellow(),
                error_msg
            );
            println!("{}", "=".repeat(80).bright_white());
            return Ok(());
        }
    }

    assert!(
        status.is_success(),
        "API returned status {}",
        status
    );

    // Response should be an array when useRcBlock=true
    let response_array = json
        .as_array()
        .expect("Response with useRcBlock=true should be an array");

    println!(
        "  {} Response contains {} block(s)",
        "+".green(),
        response_array.len()
    );

    // Validate structure of each response in the array
    for (i, item) in response_array.iter().enumerate() {
        let item_obj = item.as_object().unwrap();

        assert!(
            item_obj.contains_key("rcBlockHash"),
            "Item {} missing 'rcBlockHash'",
            i
        );
        assert!(
            item_obj.contains_key("rcBlockNumber"),
            "Item {} missing 'rcBlockNumber'",
            i
        );
        assert!(item_obj.contains_key("at"), "Item {} missing 'at'", i);
        assert!(
            item_obj.contains_key("erasPayouts"),
            "Item {} missing 'erasPayouts'",
            i
        );

        let rc_block_num = item_obj.get("rcBlockNumber").unwrap().as_str().unwrap();
        assert_eq!(
            rc_block_num,
            rc_block_number.to_string(),
            "RC block number mismatch"
        );
    }

    println!(
        "{} useRcBlock test passed with {} response(s)!",
        "+".green().bold(),
        response_array.len()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}
