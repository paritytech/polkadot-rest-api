//! Integration tests for staking-info endpoints
//!
//! Tests both:
//! - `/accounts/{accountId}/staking-info` (standard endpoint)
//! - `/rc/accounts/{accountId}/staking-info` (relay chain endpoint)

use super::{get_client, Colorize};
use anyhow::{Context, Result};

// ================================================================================================
// Test Configuration
// ================================================================================================

/// Endpoint type for parameterized testing
#[derive(Debug, Clone, Copy)]
enum EndpointType {
    /// Standard endpoint: /accounts/{accountId}/staking-info
    Standard,
    /// RC endpoint: /rc/accounts/{accountId}/staking-info
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
        let base = format!("{}/{}/staking-info", self.base_path(), account_id);
        match query {
            Some(q) => format!("{}?{}", base, q),
            None => base,
        }
    }
}

// ================================================================================================
// Test Helpers
// ================================================================================================

/// Check if error indicates relay chain not available (only relevant for RC endpoint)
fn is_relay_chain_not_available(json: &serde_json::Value) -> bool {
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        return error_str.contains("Relay chain not available");
    }
    false
}

/// Check if error indicates staking pallet not available or not a stash
fn is_staking_unavailable_or_not_stash(json: &serde_json::Value) -> bool {
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        return error_str.contains("staking pallet")
            || error_str.contains("not a stash")
            || error_str.contains("Staking");
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

    // Both endpoints: skip if staking pallet not available or not a stash
    if is_staking_unavailable_or_not_stash(json) {
        println!(
            "  {} Staking pallet not available or not a stash (skipping {} test)",
            "!".yellow(),
            endpoint_type.name()
        );
        return Ok(true);
    }

    Ok(false)
}

// ================================================================================================
// Shared Test Logic
// ================================================================================================

async fn run_basic_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = endpoint_type.build_endpoint(account_id, None);

    println!(
        "\n{} Testing {} staking-info endpoint (basic)",
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
        response_obj.contains_key("controller"),
        "Response missing 'controller' field"
    );
    assert!(
        response_obj.contains_key("rewardDestination"),
        "Response missing 'rewardDestination' field"
    );
    assert!(
        response_obj.contains_key("numSlashingSpans"),
        "Response missing 'numSlashingSpans' field"
    );
    assert!(
        response_obj.contains_key("staking"),
        "Response missing 'staking' field"
    );

    // Validate 'at' structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at missing 'hash' field");
    assert!(at_obj.contains_key("height"), "at missing 'height' field");

    // Validate 'staking' structure
    let staking_obj = response_obj.get("staking").unwrap().as_object().unwrap();
    assert!(staking_obj.contains_key("stash"), "staking missing 'stash' field");
    assert!(staking_obj.contains_key("total"), "staking missing 'total' field");
    assert!(staking_obj.contains_key("active"), "staking missing 'active' field");
    assert!(staking_obj.contains_key("unlocking"), "staking missing 'unlocking' field");

    println!("  {} Block: {}", "+".green(), at_obj.get("height").unwrap());
    println!(
        "  {} Controller: {}",
        "+".green(),
        response_obj.get("controller").unwrap()
    );
    println!(
        "  {} Total staked: {}",
        "+".green(),
        staking_obj.get("total").unwrap()
    );

    println!(
        "{} {} staking-info basic test passed!",
        "+".green().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

async fn run_at_specific_block_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let block_number = match endpoint_type {
        EndpointType::Standard => 10260000,
        EndpointType::RelayChain => 20000000,
    };
    let endpoint = endpoint_type.build_endpoint(account_id, Some(&format!("at={}", block_number)));

    println!(
        "\n{} Testing {} staking-info at specific block {}",
        "Testing".cyan().bold(),
        endpoint_type.name(),
        block_number
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
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    let height = at_obj.get("height").unwrap().as_str().unwrap();

    assert_eq!(
        height,
        block_number.to_string(),
        "Block height mismatch"
    );

    println!("  {} Block height: {}", "+".green(), height);

    println!(
        "{} {} staking-info at specific block passed!",
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
        "\n{} Testing {} staking-info with invalid address",
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

async fn run_non_stash_account_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let account_id = match endpoint_type {
        EndpointType::Standard => "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d",
        EndpointType::RelayChain => "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m",
    };
    let endpoint = endpoint_type.build_endpoint(account_id, None);

    println!(
        "\n{} Testing {} staking-info for potential non-stash account",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());

    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from API")?;

    // Skip if relay chain not available (RC only)
    if matches!(endpoint_type, EndpointType::RelayChain)
        && status.as_u16() == 400
        && is_relay_chain_not_available(&json)
    {
        println!("  {} Relay chain not configured", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    // Either success (it's a stash) or 400 (not a stash or staking unavailable)
    if status.as_u16() == 400 {
        let response_obj = json.as_object().expect("Response is not an object");
        let error_msg = response_obj.get("error").unwrap().as_str().unwrap();
        println!(
            "  {} Account is not a stash or staking unavailable: {}",
            "+".green(),
            error_msg
        );
    } else {
        assert!(
            status.is_success(),
            "Expected 200 or 400, got {}",
            status
        );
        println!("  {} Account is a stash, returned staking info", "+".green());
    }

    println!(
        "{} {} non-stash account test completed!",
        "+".green().bold(),
        endpoint_type.name()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

async fn run_hex_address_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let hex_address = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = endpoint_type.build_endpoint(hex_address, None);

    println!(
        "\n{} Testing {} staking-info with hex address",
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
        response_obj.contains_key("controller"),
        "Response should have 'controller'"
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
        "\n{} Testing {} staking-info response structure",
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

    // Validate all required fields exist
    let required_fields = ["at", "controller", "rewardDestination", "numSlashingSpans", "staking"];
    for field in required_fields {
        assert!(
            response_obj.contains_key(field),
            "Response missing '{}' field",
            field
        );
    }

    // Validate 'at' object structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at.hash is required");
    assert!(at_obj.contains_key("height"), "at.height is required");

    // Validate 'staking' object structure
    let staking_obj = response_obj.get("staking").unwrap().as_object().unwrap();
    let staking_fields = ["stash", "total", "active", "unlocking"];
    for field in staking_fields {
        assert!(
            staking_obj.contains_key(field),
            "staking.{} is required",
            field
        );
    }

    // Validate 'unlocking' is an array
    let unlocking = staking_obj.get("unlocking").unwrap();
    assert!(unlocking.is_array(), "staking.unlocking should be an array");

    // Validate unlocking chunks structure (if not empty)
    let unlocking_arr = unlocking.as_array().unwrap();
    for chunk in unlocking_arr {
        let chunk_obj = chunk.as_object().expect("Unlocking chunk should be an object");
        assert!(
            chunk_obj.contains_key("value"),
            "Unlocking chunk missing 'value' field"
        );
        assert!(
            chunk_obj.contains_key("era"),
            "Unlocking chunk missing 'era' field"
        );
    }

    // Validate 'nominations' structure if present
    if let Some(nominations) = response_obj.get("nominations") {
        let nominations_obj = nominations.as_object().expect("nominations should be an object");
        assert!(
            nominations_obj.contains_key("targets"),
            "nominations.targets is required"
        );
        assert!(
            nominations_obj.contains_key("submittedIn"),
            "nominations.submittedIn is required"
        );
        assert!(
            nominations_obj.contains_key("suppressed"),
            "nominations.suppressed is required"
        );

        let targets = nominations_obj.get("targets").unwrap();
        assert!(targets.is_array(), "nominations.targets should be an array");
    }

    // Direct query should NOT have useRcBlock-related fields
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

async fn run_reward_destination_variants_test(endpoint_type: EndpointType) -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = endpoint_type.build_endpoint(account_id, None);

    println!(
        "\n{} Testing {} staking-info reward destination variants",
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
    let reward_dest = response_obj
        .get("rewardDestination")
        .expect("rewardDestination required");

    // rewardDestination can be either a string (Simple variant) or an object with account (Account variant)
    if reward_dest.is_string() {
        let dest_str = reward_dest.as_str().unwrap();
        println!("  {} Reward destination (simple): {}", "+".green(), dest_str);
    } else if reward_dest.is_object() {
        let dest_obj = reward_dest.as_object().unwrap();
        if dest_obj.contains_key("account") {
            let account = dest_obj.get("account").unwrap().as_str().unwrap();
            println!("  {} Reward destination (account): {}", "+".green(), account);
        }
    }

    println!(
        "{} {} reward destination validation passed!",
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
async fn test_standard_staking_info_basic() -> Result<()> {
    run_basic_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_info_at_specific_block() -> Result<()> {
    run_at_specific_block_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_info_invalid_address() -> Result<()> {
    run_invalid_address_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_info_non_stash_account() -> Result<()> {
    run_non_stash_account_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_info_hex_address() -> Result<()> {
    run_hex_address_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_info_response_structure() -> Result<()> {
    run_response_structure_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_standard_staking_info_reward_destination_variants() -> Result<()> {
    run_reward_destination_variants_test(EndpointType::Standard).await
}

// ================================================================================================
// RC Endpoint Tests
// ================================================================================================

#[tokio::test]
async fn test_rc_staking_info_basic() -> Result<()> {
    run_basic_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_info_at_specific_block() -> Result<()> {
    run_at_specific_block_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_info_invalid_address() -> Result<()> {
    run_invalid_address_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_info_non_stash_account() -> Result<()> {
    run_non_stash_account_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_info_hex_address() -> Result<()> {
    run_hex_address_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_info_response_structure() -> Result<()> {
    run_response_structure_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_staking_info_reward_destination_variants() -> Result<()> {
    run_reward_destination_variants_test(EndpointType::RelayChain).await
}

// ================================================================================================
// Standard Endpoint Specific Tests (useRcBlock parameter)
// ================================================================================================

#[tokio::test]
async fn test_standard_staking_info_use_rc_block() -> Result<()> {
    let client = get_client().await?;
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let rc_block_number = 26054957;
    let endpoint = format!(
        "/accounts/{}/staking-info?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing standard staking-info with useRcBlock at RC block {}",
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
            || error_msg.contains("not a stash")
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
        assert!(
            item_obj.contains_key("ahTimestamp"),
            "Item {} missing 'ahTimestamp'",
            i
        );
        assert!(item_obj.contains_key("at"), "Item {} missing 'at'", i);
        assert!(
            item_obj.contains_key("controller"),
            "Item {} missing 'controller'",
            i
        );
        assert!(
            item_obj.contains_key("staking"),
            "Item {} missing 'staking'",
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
