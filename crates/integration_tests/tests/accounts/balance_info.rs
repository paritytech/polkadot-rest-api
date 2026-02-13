// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for /accounts/{accountId}/balance-info endpoint
//! Tests both standard (/accounts) and relay chain (/rc/accounts) endpoints

use super::{
    Colorize, EndpointType, get_client, print_skip_message, should_skip_rc_test, test_accounts,
};
use anyhow::{Context, Result};

// ================================================================================================
// Balance Info Endpoint Extension
// ================================================================================================

/// Extension trait for EndpointType to build balance-info endpoints
trait BalanceEndpoint {
    fn build_balance_endpoint(&self, account_id: &str, query: Option<&str>) -> String;
    fn get_balance_test_account(&self) -> &'static str;
    fn get_balance_test_block(&self) -> u64;
}

impl BalanceEndpoint for EndpointType {
    fn build_balance_endpoint(&self, account_id: &str, query: Option<&str>) -> String {
        match query {
            Some(q) => format!("{}/{}/balance-info?{}", self.base_path(), account_id, q),
            None => format!("{}/{}/balance-info", self.base_path(), account_id),
        }
    }

    fn get_balance_test_account(&self) -> &'static str {
        match self {
            EndpointType::Standard => test_accounts::ASSET_HUB_ACCOUNT,
            EndpointType::RelayChain => test_accounts::POLKADOT_TREASURY,
        }
    }

    fn get_balance_test_block(&self) -> u64 {
        match self {
            EndpointType::Standard => 10260000,
            EndpointType::RelayChain => 1000000,
        }
    }
}

// ================================================================================================
// Shared Test Logic
// ================================================================================================

async fn run_basic_test(endpoint_type: EndpointType) -> Result<()> {
    let local_client = get_client().await?;

    let account_id = endpoint_type.get_balance_test_account();
    let endpoint = endpoint_type.build_balance_endpoint(account_id, None);

    println!(
        "\n{} Testing {} balance-info endpoint (basic)",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "═".repeat(80).bright_white());

    println!(
        "{} Fetching from local API: {}{}",
        "→".cyan(),
        local_client.base_url(),
        endpoint
    );

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Check for relay chain skip condition (RC endpoint only)
    if matches!(endpoint_type, EndpointType::RelayChain)
        && should_skip_rc_test(local_status.as_u16(), &local_json)
    {
        print_skip_message("basic", "Relay chain not configured");
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    let response_obj = local_json.as_object().expect("Response is not an object");

    // Required fields
    let required_fields = ["at", "nonce", "tokenSymbol", "free", "reserved", "locks"];
    for field in required_fields {
        assert!(
            response_obj.contains_key(field),
            "Response missing '{}' field",
            field
        );
    }

    // Validate at structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(
        at_obj.contains_key("hash"),
        "at object missing 'hash' field"
    );
    assert!(
        at_obj.contains_key("height"),
        "at object missing 'height' field"
    );

    // Validate locks is an array
    let locks = response_obj.get("locks").unwrap();
    assert!(locks.is_array(), "locks should be an array");

    // Print some info
    let nonce = response_obj.get("nonce").unwrap();
    let free = response_obj.get("free").unwrap();
    let token = response_obj.get("tokenSymbol").unwrap();
    println!(
        "  {} Token: {}, Nonce: {}, Free: {}",
        "ℹ".blue(),
        token.as_str().unwrap_or("N/A"),
        nonce.as_str().unwrap_or("N/A"),
        free.as_str().unwrap_or("N/A")
    );

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

async fn run_at_specific_block_test(endpoint_type: EndpointType) -> Result<()> {
    let local_client = get_client().await?;

    let account_id = endpoint_type.get_balance_test_account();
    let block_number = endpoint_type.get_balance_test_block();
    let endpoint =
        endpoint_type.build_balance_endpoint(account_id, Some(&format!("at={}", block_number)));

    println!(
        "\n{} Testing {} balance-info at block {}",
        "Testing".cyan().bold(),
        endpoint_type.name(),
        block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if matches!(endpoint_type, EndpointType::RelayChain)
        && should_skip_rc_test(local_status.as_u16(), &local_json)
    {
        print_skip_message("at specific block", "Relay chain not configured");
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();

    // Verify block height matches
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    let height = at_obj.get("height").unwrap().as_str().unwrap();
    assert_eq!(height, block_number.to_string(), "Block height mismatch");

    println!(
        "{} Response at block {} validated!",
        "✓".green().bold(),
        block_number
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

async fn run_invalid_address_test(endpoint_type: EndpointType) -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = test_accounts::INVALID_ADDRESS;
    let endpoint = endpoint_type.build_balance_endpoint(invalid_address, None);

    println!(
        "\n{} Testing {} balance-info with invalid address",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert_eq!(
        local_status.as_u16(),
        400,
        "Expected 400 Bad Request, got {}",
        local_status
    );

    println!("{} Received expected 400 Bad Request", "✓".green());

    let error_obj = local_json.as_object().unwrap();
    assert!(
        error_obj.contains_key("error"),
        "Error response missing 'error' field"
    );

    let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
    assert!(
        error_msg.contains("Invalid") || error_msg.contains("address"),
        "Error message doesn't contain expected text: {}",
        error_msg
    );

    println!("{} Error message validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

async fn run_with_denominated_test(endpoint_type: EndpointType) -> Result<()> {
    let local_client = get_client().await?;

    let account_id = endpoint_type.get_balance_test_account();
    let endpoint = endpoint_type.build_balance_endpoint(account_id, Some("denominated=true"));

    println!(
        "\n{} Testing {} balance-info with denominated=true",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if matches!(endpoint_type, EndpointType::RelayChain)
        && should_skip_rc_test(local_status.as_u16(), &local_json)
    {
        print_skip_message("denominated", "Relay chain not configured");
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();

    // Check that free balance contains a decimal point (indicating denomination)
    let free = response_obj.get("free").unwrap().as_str().unwrap();
    // If the balance is not zero, it should contain a decimal point when denominated
    if free != "0" && !free.starts_with("0.") {
        assert!(
            free.contains('.'),
            "Denominated balance should contain decimal point: {}",
            free
        );
    }

    println!("  {} Denominated free balance: {}", "ℹ".blue(), free);

    println!("{} Denominated response validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

async fn run_locks_structure_test(endpoint_type: EndpointType) -> Result<()> {
    let local_client = get_client().await?;

    let account_id = endpoint_type.get_balance_test_account();
    let endpoint = endpoint_type.build_balance_endpoint(account_id, None);

    println!(
        "\n{} Testing {} balance-info locks structure",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if matches!(endpoint_type, EndpointType::RelayChain)
        && should_skip_rc_test(local_status.as_u16(), &local_json)
    {
        print_skip_message("locks structure", "Relay chain not configured");
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();
    let locks = response_obj.get("locks").unwrap().as_array().unwrap();

    println!("  {} Account has {} lock(s)", "ℹ".blue(), locks.len());

    // Validate lock structure if any locks exist
    for (i, lock) in locks.iter().enumerate() {
        let lock_obj = lock.as_object().unwrap();
        assert!(lock_obj.contains_key("id"), "Lock {} missing 'id' field", i);
        assert!(
            lock_obj.contains_key("amount"),
            "Lock {} missing 'amount' field",
            i
        );
        assert!(
            lock_obj.contains_key("reasons"),
            "Lock {} missing 'reasons' field",
            i
        );

        println!(
            "    Lock {}: id={}, amount={}, reasons={}",
            i,
            lock_obj.get("id").unwrap().as_str().unwrap_or("N/A"),
            lock_obj.get("amount").unwrap().as_str().unwrap_or("N/A"),
            lock_obj.get("reasons").unwrap().as_str().unwrap_or("N/A")
        );
    }

    println!("{} Locks structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

async fn run_response_structure_test(endpoint_type: EndpointType) -> Result<()> {
    let local_client = get_client().await?;

    let account_id = endpoint_type.get_balance_test_account();
    let endpoint = endpoint_type.build_balance_endpoint(account_id, None);

    println!(
        "\n{} Testing {} balance-info response structure",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if matches!(endpoint_type, EndpointType::RelayChain)
        && should_skip_rc_test(local_status.as_u16(), &local_json)
    {
        print_skip_message("response structure", "Relay chain not configured");
        return Ok(());
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

    // RC endpoint should NOT have useRcBlock-related fields
    if matches!(endpoint_type, EndpointType::RelayChain) {
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
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

async fn run_hex_address_test(endpoint_type: EndpointType) -> Result<()> {
    let local_client = get_client().await?;

    // Hex address (32 bytes = 64 hex chars)
    let hex_address = endpoint_type.get_hex_address();
    let endpoint = endpoint_type.build_balance_endpoint(hex_address, None);

    println!(
        "\n{} Testing {} balance-info with hex address",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if matches!(endpoint_type, EndpointType::RelayChain)
        && should_skip_rc_test(local_status.as_u16(), &local_json)
    {
        print_skip_message("hex address", "Relay chain not configured");
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    assert!(response_obj.contains_key("at"), "Response should have 'at'");

    println!("  {} Hex address accepted", "✓".green());

    println!("{} Hex address test passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

async fn run_frozen_fields_test(endpoint_type: EndpointType) -> Result<()> {
    let local_client = get_client().await?;

    let account_id = endpoint_type.get_balance_test_account();
    let endpoint = endpoint_type.build_balance_endpoint(account_id, None);

    println!(
        "\n{} Testing {} balance-info frozen fields",
        "Testing".cyan().bold(),
        endpoint_type.name()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if matches!(endpoint_type, EndpointType::RelayChain)
        && should_skip_rc_test(local_status.as_u16(), &local_json)
    {
        print_skip_message("frozen fields", "Relay chain not configured");
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();

    let misc_frozen = response_obj.get("miscFrozen").unwrap().as_str().unwrap();
    let fee_frozen = response_obj.get("feeFrozen").unwrap().as_str().unwrap();
    let frozen = response_obj.get("frozen").unwrap().as_str().unwrap();

    // Either frozen exists and miscFrozen/feeFrozen are messages, or vice versa
    let is_new_runtime = misc_frozen.contains("does not exist");
    let is_old_runtime = frozen.contains("does not exist");

    assert!(
        is_new_runtime || is_old_runtime,
        "Expected either frozen or miscFrozen/feeFrozen to indicate runtime version"
    );

    if is_new_runtime {
        println!(
            "  {} New runtime detected (uses 'frozen' field): {}",
            "ℹ".blue(),
            frozen
        );
    } else {
        println!(
            "  {} Old runtime detected (uses 'miscFrozen'/'feeFrozen' fields): {} / {}",
            "ℹ".blue(),
            misc_frozen,
            fee_frozen
        );
    }

    println!("{} Frozen fields validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

// ================================================================================================
// Standard Endpoint Tests
// ================================================================================================

#[tokio::test]
async fn test_balance_info_basic() -> Result<()> {
    run_basic_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_balance_info_at_specific_block() -> Result<()> {
    run_at_specific_block_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_balance_info_invalid_address() -> Result<()> {
    run_invalid_address_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_balance_info_with_denominated() -> Result<()> {
    run_with_denominated_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_balance_info_locks_structure() -> Result<()> {
    run_locks_structure_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_balance_info_response_structure() -> Result<()> {
    run_response_structure_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_balance_info_hex_address() -> Result<()> {
    run_hex_address_test(EndpointType::Standard).await
}

#[tokio::test]
async fn test_balance_info_frozen_fields() -> Result<()> {
    run_frozen_fields_test(EndpointType::Standard).await
}

// ================================================================================================
// Relay Chain Endpoint Tests
// ================================================================================================

#[tokio::test]
async fn test_rc_balance_info_basic() -> Result<()> {
    run_basic_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_balance_info_at_specific_block() -> Result<()> {
    run_at_specific_block_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_balance_info_invalid_address() -> Result<()> {
    run_invalid_address_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_balance_info_with_denominated() -> Result<()> {
    run_with_denominated_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_balance_info_locks_structure() -> Result<()> {
    run_locks_structure_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_balance_info_response_structure() -> Result<()> {
    run_response_structure_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_balance_info_hex_address() -> Result<()> {
    run_hex_address_test(EndpointType::RelayChain).await
}

#[tokio::test]
async fn test_rc_balance_info_frozen_fields() -> Result<()> {
    run_frozen_fields_test(EndpointType::RelayChain).await
}

// ================================================================================================
// Standard Endpoint Only: useRcBlock Tests
// ================================================================================================

#[tokio::test]
async fn test_balance_info_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let rc_block_number = 10554957;
    let endpoint = format!(
        "/accounts/{}/balance-info?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing balance info with useRcBlock at RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let local_array = local_json
        .as_array()
        .expect("Response with useRcBlock=true should be an array");

    println!(
        "  {} Response contains {} block(s)",
        "✓".green(),
        local_array.len()
    );

    // Validate structure of each response in the array
    for (i, item) in local_array.iter().enumerate() {
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
        assert!(item_obj.contains_key("free"), "Item {} missing 'free'", i);
        assert!(item_obj.contains_key("locks"), "Item {} missing 'locks'", i);

        let rc_block_num = item_obj.get("rcBlockNumber").unwrap().as_str().unwrap();
        assert_eq!(
            rc_block_num,
            rc_block_number.to_string(),
            "RC block number mismatch"
        );
    }

    println!(
        "{} All {} block response(s) validated!",
        "✓".green().bold(),
        local_array.len()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_balance_info_use_rc_block_empty() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    // Block 10554958 is a Relay Chain block that doesn't include any Asset Hub blocks
    let rc_block_number = 10554958;
    let endpoint = format!(
        "/accounts/{}/balance-info?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing balance info useRcBlock with empty RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    let local_array = local_json
        .as_array()
        .expect("Response with useRcBlock=true should be an array");

    assert!(
        local_array.is_empty(),
        "Expected empty array for RC block {}, but got {} block(s)",
        rc_block_number,
        local_array.len()
    );

    println!("{} Response is empty array as expected", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}
