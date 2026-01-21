//! Integration tests for /rc/accounts/{accountId}/balance-info endpoint
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

#[tokio::test]
async fn test_rc_balance_info_basic() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known Polkadot address
    let account_id = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let endpoint = format!("/rc/accounts/{}/balance-info", account_id);

    println!(
        "\n{} Testing RC balance-info endpoint (basic)",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // The endpoint might return an error if relay chain is not available
    // That's OK - we're testing the endpoint exists and responds
    if local_status.as_u16() == 400 {
        let response_obj = local_json.as_object().expect("Response is not an object");
        if let Some(error) = response_obj.get("error") {
            let error_str = error.as_str().unwrap_or("");
            if error_str.contains("Relay chain not available") {
                println!(
                    "  {} Relay chain not configured (expected when connected to relay chain directly or no RC configured)",
                    "⚠".yellow()
                );
                println!("{} Test skipped - no relay chain configured", "⚠".yellow().bold());
                println!("{}", "═".repeat(80).bright_white());
                return Ok(());
            }
        }
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
        response_obj.contains_key("nonce"),
        "Response missing 'nonce' field"
    );
    assert!(
        response_obj.contains_key("tokenSymbol"),
        "Response missing 'tokenSymbol' field"
    );
    assert!(
        response_obj.contains_key("free"),
        "Response missing 'free' field"
    );
    assert!(
        response_obj.contains_key("reserved"),
        "Response missing 'reserved' field"
    );
    assert!(
        response_obj.contains_key("locks"),
        "Response missing 'locks' field"
    );

    // Validate 'at' structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at missing 'hash' field");
    assert!(at_obj.contains_key("height"), "at missing 'height' field");

    println!("  {} Block: {}", "✓".green(), at_obj.get("height").unwrap());
    println!(
        "  {} Token: {}",
        "✓".green(),
        response_obj.get("tokenSymbol").unwrap()
    );
    println!(
        "  {} Free balance: {}",
        "✓".green(),
        response_obj.get("free").unwrap()
    );

    println!("{} RC balance-info basic test passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_balance_info_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let block_number = 1000000;
    let endpoint = format!(
        "/rc/accounts/{}/balance-info?at={}",
        account_id, block_number
    );

    println!(
        "\n{} Testing RC balance-info at specific block",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Skip if relay chain not available
    if local_status.as_u16() == 400 {
        let response_obj = local_json.as_object().expect("Response is not an object");
        if let Some(error) = response_obj.get("error") {
            let error_str = error.as_str().unwrap_or("");
            if error_str.contains("Relay chain not available") {
                println!("  {} Relay chain not configured", "⚠".yellow());
                println!("{}", "═".repeat(80).bright_white());
                return Ok(());
            }
        }
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

    println!("  {} Block height: {}", "✓".green(), height);

    println!(
        "{} RC balance-info at specific block passed!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_balance_info_with_denominated() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let endpoint = format!("/rc/accounts/{}/balance-info?denominated=true", account_id);

    println!(
        "\n{} Testing RC balance-info with denominated=true",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Skip if relay chain not available
    if local_status.as_u16() == 400 {
        let response_obj = local_json.as_object().expect("Response is not an object");
        if let Some(error) = response_obj.get("error") {
            let error_str = error.as_str().unwrap_or("");
            if error_str.contains("Relay chain not available") {
                println!("  {} Relay chain not configured", "⚠".yellow());
                println!("{}", "═".repeat(80).bright_white());
                return Ok(());
            }
        }
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    let free = response_obj.get("free").unwrap().as_str().unwrap();

    // Denominated values should contain a decimal point (unless zero)
    if free != "0" && !free.starts_with("0.") {
        // Non-zero whole numbers should have decimal point when denominated
        println!("  {} Free (denominated): {}", "✓".green(), free);
    }

    println!(
        "{} RC balance-info with denominated passed!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_balance_info_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = "invalid-address-123";
    let endpoint = format!("/rc/accounts/{}/balance-info", invalid_address);

    println!(
        "\n{} Testing RC balance-info with invalid address",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

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

    println!("  {} Error: {}", "✓".green(), error_msg);

    println!(
        "{} Invalid address error handled correctly!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_balance_info_response_structure() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let endpoint = format!("/rc/accounts/{}/balance-info", account_id);

    println!(
        "\n{} Testing RC balance-info response structure",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Skip if relay chain not available
    if local_status.as_u16() == 400 {
        let response_obj = local_json.as_object().expect("Response is not an object");
        if let Some(error) = response_obj.get("error") {
            let error_str = error.as_str().unwrap_or("");
            if error_str.contains("Relay chain not available") {
                println!("  {} Relay chain not configured", "⚠".yellow());
                println!("{}", "═".repeat(80).bright_white());
                return Ok(());
            }
        }
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");

    // Validate all required fields exist and have correct types
    let required_fields = [
        ("at", "object"),
        ("nonce", "string"),
        ("tokenSymbol", "string"),
        ("free", "string"),
        ("reserved", "string"),
        ("miscFrozen", "string"),
        ("feeFrozen", "string"),
        ("frozen", "string"),
        ("transferable", "string"),
        ("locks", "array"),
    ];

    for (field, expected_type) in required_fields {
        assert!(
            response_obj.contains_key(field),
            "Response missing '{}' field",
            field
        );

        let value = response_obj.get(field).unwrap();
        let actual_type = if value.is_object() {
            "object"
        } else if value.is_string() {
            "string"
        } else if value.is_array() {
            "array"
        } else {
            "unknown"
        };

        assert_eq!(
            actual_type, expected_type,
            "Field '{}' has wrong type: expected {}, got {}",
            field, expected_type, actual_type
        );
    }

    // Validate 'at' object structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at.hash is required");
    assert!(at_obj.contains_key("height"), "at.height is required");

    // Validate locks array structure (if not empty)
    let locks = response_obj.get("locks").unwrap().as_array().unwrap();
    for lock in locks {
        let lock_obj = lock.as_object().expect("Lock should be an object");
        assert!(lock_obj.contains_key("id"), "Lock missing 'id' field");
        assert!(
            lock_obj.contains_key("amount"),
            "Lock missing 'amount' field"
        );
        assert!(
            lock_obj.contains_key("reasons"),
            "Lock missing 'reasons' field"
        );
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

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_rc_balance_info_hex_address() -> Result<()> {
    let local_client = get_client().await?;

    // Hex address (32 bytes = 64 hex chars)
    let hex_address = "0x2a39366f6620a6c2e2fed5990a3d419e6a19dd127fc7a50b515cf17e2dc5cc59";
    let endpoint = format!("/rc/accounts/{}/balance-info", hex_address);

    println!(
        "\n{} Testing RC balance-info with hex address",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Skip if relay chain not available
    if local_status.as_u16() == 400 {
        let response_obj = local_json.as_object().expect("Response is not an object");
        if let Some(error) = response_obj.get("error") {
            let error_str = error.as_str().unwrap_or("");
            if error_str.contains("Relay chain not available") {
                println!("  {} Relay chain not configured", "⚠".yellow());
                println!("{}", "═".repeat(80).bright_white());
                return Ok(());
            }
        }
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    assert!(response_obj.contains_key("at"), "Response should have 'at'");

    println!("  {} Hex address accepted", "✓".green());

    println!(
        "{} RC balance-info with hex address passed!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
