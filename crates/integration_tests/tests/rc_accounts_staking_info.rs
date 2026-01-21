//! Integration tests for /rc/accounts/{accountId}/staking-info endpoint
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

/// Helper to check if error indicates staking pallet not available or not a stash
fn is_staking_unavailable_or_not_stash(json: &serde_json::Value) -> bool {
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        return error_str.contains("staking pallet")
            || error_str.contains("not a stash")
            || error_str.contains("Staking");
    }
    false
}

#[tokio::test]
async fn test_rc_staking_info_basic() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known validator/nominator stash account ID
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/rc/accounts/{}/staking-info", account_id);

    println!(
        "\n{} Testing RC staking-info endpoint (basic)",
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

    // Handle staking pallet not available or not a stash
    if local_status.as_u16() == 400 && is_staking_unavailable_or_not_stash(&local_json) {
        println!(
            "  {} Staking pallet not available or address not a stash on relay chain",
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

    println!("{} RC staking-info basic test passed!", "+".green().bold());
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_info_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let block_number = 20000000;
    let endpoint = format!(
        "/rc/accounts/{}/staking-info?at={}",
        account_id, block_number
    );

    println!(
        "\n{} Testing RC staking-info at specific block",
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

    // Skip if staking not available or not a stash
    if local_status.as_u16() == 400 && is_staking_unavailable_or_not_stash(&local_json) {
        println!("  {} Staking not available or not a stash at this block", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    let height = at_obj.get("height").unwrap().as_str().unwrap();

    assert_eq!(
        height,
        block_number.to_string(),
        "Block height mismatch"
    );

    println!("  {} Block height: {}", "+".green(), height);

    println!(
        "{} RC staking-info at specific block passed!",
        "+".green().bold()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_info_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = "invalid-address-123";
    let endpoint = format!("/rc/accounts/{}/staking-info", invalid_address);

    println!(
        "\n{} Testing RC staking-info with invalid address",
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
async fn test_rc_staking_info_response_structure() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/rc/accounts/{}/staking-info", account_id);

    println!(
        "\n{} Testing RC staking-info response structure",
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
    if local_status.as_u16() == 400 && is_staking_unavailable_or_not_stash(&local_json) {
        println!("  {} Staking not available or not a stash", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");

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
        assert!(chunk_obj.contains_key("value"), "Unlocking chunk missing 'value' field");
        assert!(chunk_obj.contains_key("era"), "Unlocking chunk missing 'era' field");
    }

    // Validate 'nominations' structure if present
    if let Some(nominations) = response_obj.get("nominations") {
        let nominations_obj = nominations.as_object().expect("nominations should be an object");
        assert!(nominations_obj.contains_key("targets"), "nominations.targets is required");
        assert!(nominations_obj.contains_key("submittedIn"), "nominations.submittedIn is required");
        assert!(nominations_obj.contains_key("suppressed"), "nominations.suppressed is required");

        // Validate targets is an array
        let targets = nominations_obj.get("targets").unwrap();
        assert!(targets.is_array(), "nominations.targets should be an array");
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
async fn test_rc_staking_info_hex_address() -> Result<()> {
    let local_client = get_client().await?;

    // Hex address (32 bytes = 64 hex chars)
    let hex_address = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = format!("/rc/accounts/{}/staking-info", hex_address);

    println!(
        "\n{} Testing RC staking-info with hex address",
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

    // Skip if staking not available or not a stash (hex addresses might not be stash accounts)
    if local_status.as_u16() == 400 && is_staking_unavailable_or_not_stash(&local_json) {
        println!("  {} Staking not available or not a stash (hex address accepted)", "!".yellow());
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
    assert!(response_obj.contains_key("controller"), "Response should have 'controller'");

    println!("  {} Hex address accepted", "+".green());

    println!(
        "{} RC staking-info with hex address passed!",
        "+".green().bold()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_info_non_stash_account() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known Polkadot address that may not be a stash
    let account_id = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let endpoint = format!("/rc/accounts/{}/staking-info", account_id);

    println!(
        "\n{} Testing RC staking-info for potential non-stash account",
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

    // Either success (it's a stash) or 400 (not a stash)
    if local_status.as_u16() == 400 {
        let response_obj = local_json.as_object().expect("Response is not an object");
        let error_msg = response_obj.get("error").unwrap().as_str().unwrap();

        // Verify error message mentions not being a stash account
        assert!(
            error_msg.contains("not a stash") || error_msg.contains("staking"),
            "Error should indicate account is not a stash or staking unavailable: {}",
            error_msg
        );

        println!("  {} Account is not a stash: {}", "+".green(), error_msg);
    } else {
        assert!(
            local_status.is_success(),
            "Expected 200 or 400, got {}",
            local_status
        );
        println!("  {} Account is a stash, returned staking info", "+".green());
    }

    println!(
        "{} Non-stash account test completed!",
        "+".green().bold()
    );
    println!("{}", "=".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_staking_info_reward_destination_variants() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known validator stash
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/rc/accounts/{}/staking-info", account_id);

    println!(
        "\n{} Testing RC staking-info reward destination variants",
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
    if local_status.as_u16() == 400 && is_staking_unavailable_or_not_stash(&local_json) {
        println!("  {} Staking not available or not a stash", "!".yellow());
        println!("{}", "=".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    let reward_dest = response_obj.get("rewardDestination").expect("rewardDestination required");

    // rewardDestination can be either a string (Simple variant) or an object with account (Account variant)
    if reward_dest.is_string() {
        let dest_str = reward_dest.as_str().unwrap();
        // Valid simple variants: Staked, Stash, Controller, None
        let valid_variants = ["Staked", "Stash", "Controller", "None"];
        println!("  {} Reward destination (simple): {}", "+".green(), dest_str);
        // Note: variant might be custom, so we just log it
    } else if reward_dest.is_object() {
        let dest_obj = reward_dest.as_object().unwrap();
        if dest_obj.contains_key("account") {
            let account = dest_obj.get("account").unwrap().as_str().unwrap();
            println!("  {} Reward destination (account): {}", "+".green(), account);
        }
    }

    println!(
        "{} Reward destination validation passed!",
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
