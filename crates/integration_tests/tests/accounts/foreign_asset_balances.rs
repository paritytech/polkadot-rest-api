// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for /accounts/{accountId}/foreign-asset-balances endpoint

use super::{Colorize, get_client, test_accounts};
use anyhow::{Context, Result};

/// Check if error indicates pallet or feature unavailable
fn should_skip_test(status: u16, json: &serde_json::Value) -> bool {
    if status != 400 && status != 500 {
        return false;
    }
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        if error_str.contains("pallet")
            || error_str.contains("not found")
            || error_str.contains("not available")
            || error_str.contains("useRcBlock")
        {
            println!(
                "  {} Feature not available (skipping test): {}",
                "!".yellow(),
                error_str
            );
            return true;
        }
    }
    false
}

#[tokio::test]
async fn test_foreign_asset_balances_basic() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let endpoint = format!("/accounts/{}/foreign-asset-balances", account_id);

    println!(
        "\n{} Testing foreign asset balances endpoint for account {}",
        "Testing".cyan().bold(),
        account_id.yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_test(local_status.as_u16(), &local_json) {
        println!("{}", "═".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");

    assert!(
        response_obj.contains_key("at"),
        "Response missing 'at' field"
    );
    assert!(
        response_obj.contains_key("foreignAssets"),
        "Response missing 'foreignAssets' field"
    );

    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(
        at_obj.contains_key("hash"),
        "at object missing 'hash' field"
    );
    assert!(
        at_obj.contains_key("height"),
        "at object missing 'height' field"
    );

    let foreign_assets = response_obj
        .get("foreignAssets")
        .unwrap()
        .as_array()
        .unwrap();
    println!(
        "  {} Response contains {} foreign asset(s)",
        "✓".green(),
        foreign_assets.len()
    );

    for asset in foreign_assets {
        let asset_obj = asset.as_object().unwrap();
        assert!(
            asset_obj.contains_key("multiLocation"),
            "Foreign asset missing 'multiLocation' field"
        );
        assert!(
            asset_obj.contains_key("balance"),
            "Foreign asset missing 'balance' field"
        );
        assert!(
            asset_obj.contains_key("isFrozen"),
            "Foreign asset missing 'isFrozen' field"
        );
        assert!(
            asset_obj.contains_key("isSufficient"),
            "Foreign asset missing 'isSufficient' field"
        );
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_foreign_asset_balances_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let block_number = 10260000;
    let endpoint = format!(
        "/accounts/{}/foreign-asset-balances?at={}",
        account_id, block_number
    );

    println!(
        "\n{} Testing foreign asset balances at block {}",
        "Testing".cyan().bold(),
        block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_test(local_status.as_u16(), &local_json) {
        println!("{}", "═".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();
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

#[tokio::test]
async fn test_foreign_asset_balances_with_filter() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let multi_location =
        r#"{"parents":"2","interior":{"X1":{"GlobalConsensus":{"Ethereum":{"chainId":"1"}}}}}"#;
    let endpoint = format!(
        "/accounts/{}/foreign-asset-balances?foreignAssets[]={}",
        account_id, multi_location
    );

    println!(
        "\n{} Testing foreign asset balances with filter",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_test(local_status.as_u16(), &local_json) {
        println!("{}", "═".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();
    let foreign_assets = response_obj
        .get("foreignAssets")
        .unwrap()
        .as_array()
        .unwrap();

    println!(
        "  {} Response contains {} foreign asset(s) after filtering",
        "✓".green(),
        foreign_assets.len()
    );

    for asset in foreign_assets {
        let asset_obj = asset.as_object().unwrap();
        assert!(
            asset_obj.contains_key("multiLocation"),
            "Foreign asset missing 'multiLocation' field"
        );
        assert!(
            asset_obj.contains_key("balance"),
            "Foreign asset missing 'balance' field"
        );
        assert!(
            asset_obj.contains_key("isFrozen"),
            "Foreign asset missing 'isFrozen' field"
        );
        assert!(
            asset_obj.contains_key("isSufficient"),
            "Foreign asset missing 'isSufficient' field"
        );

        // Validate the returned multiLocation matches the requested filter
        let location = asset_obj.get("multiLocation").unwrap();
        let location_str = location.to_string();
        assert!(
            location_str.contains("Ethereum"),
            "Filtered result should match the requested Ethereum multiLocation, got: {}",
            location_str
        );
    }

    println!("{} Filter validation passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_foreign_asset_balances_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = test_accounts::INVALID_ADDRESS;
    let endpoint = format!("/accounts/{}/foreign-asset-balances", invalid_address);

    println!(
        "\n{} Testing foreign asset balances with invalid address",
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
        "Expected 400 Bad Request, got {}",
        local_status
    );

    let error_obj = local_json.as_object().unwrap();
    assert!(
        error_obj.contains_key("error"),
        "Error response missing 'error' field"
    );

    let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
    assert!(
        error_msg.contains("Invalid account address"),
        "Error message doesn't contain expected text: {}",
        error_msg
    );

    println!("{} Error message validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_foreign_asset_balances_hex_address() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ALICE_HEX;
    let endpoint = format!("/accounts/{}/foreign-asset-balances", account_id);

    println!(
        "\n{} Testing foreign asset balances with hex address",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_test(local_status.as_u16(), &local_json) {
        println!("{}", "═".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();
    assert!(
        response_obj.contains_key("at"),
        "Response missing 'at' field"
    );
    assert!(
        response_obj.contains_key("foreignAssets"),
        "Response missing 'foreignAssets' field"
    );

    println!("{} Hex address validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_foreign_asset_balances_response_structure() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let endpoint = format!("/accounts/{}/foreign-asset-balances", account_id);

    println!(
        "\n{} Testing foreign asset balances response structure",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_test(local_status.as_u16(), &local_json) {
        println!("{}", "═".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");

    // Validate top-level fields and types
    let at = response_obj.get("at").expect("Missing 'at' field");
    assert!(at.is_object(), "'at' should be an object");

    let at_obj = at.as_object().unwrap();
    assert!(
        at_obj.get("hash").unwrap().is_string(),
        "at.hash should be a string"
    );
    assert!(
        at_obj.get("height").unwrap().is_string(),
        "at.height should be a string"
    );

    let foreign_assets = response_obj
        .get("foreignAssets")
        .expect("Missing 'foreignAssets' field");
    assert!(
        foreign_assets.is_array(),
        "'foreignAssets' should be an array"
    );

    for (i, asset) in foreign_assets.as_array().unwrap().iter().enumerate() {
        let asset_obj = asset.as_object().unwrap();

        let multi_location = asset_obj
            .get("multiLocation")
            .unwrap_or_else(|| panic!("Asset {} missing 'multiLocation'", i));
        assert!(
            multi_location.is_object() || multi_location.is_string(),
            "Asset {} 'multiLocation' should be an object or string",
            i
        );

        let balance = asset_obj
            .get("balance")
            .unwrap_or_else(|| panic!("Asset {} missing 'balance'", i));
        assert!(
            balance.is_string(),
            "Asset {} 'balance' should be a string",
            i
        );

        let is_frozen = asset_obj
            .get("isFrozen")
            .unwrap_or_else(|| panic!("Asset {} missing 'isFrozen'", i));
        assert!(
            is_frozen.is_boolean(),
            "Asset {} 'isFrozen' should be a boolean",
            i
        );

        let is_sufficient = asset_obj
            .get("isSufficient")
            .unwrap_or_else(|| panic!("Asset {} missing 'isSufficient'", i));
        assert!(
            is_sufficient.is_boolean(),
            "Asset {} 'isSufficient' should be a boolean",
            i
        );

        println!(
            "    Asset {}: balance={}, isFrozen={}, isSufficient={}",
            i,
            balance.as_str().unwrap_or("N/A"),
            is_frozen.as_bool().unwrap(),
            is_sufficient.as_bool().unwrap()
        );
    }

    println!("{} Deep response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_foreign_asset_balances_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let rc_block_number = 10554957;
    let endpoint = format!(
        "/accounts/{}/foreign-asset-balances?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing foreign asset balances with useRcBlock at RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_test(local_status.as_u16(), &local_json) {
        println!("{}", "═".repeat(80).bright_white());
        return Ok(());
    }

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
        assert!(
            item_obj.contains_key("foreignAssets"),
            "Item {} missing 'foreignAssets'",
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
        "{} All {} block response(s) validated!",
        "✓".green().bold(),
        local_array.len()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_foreign_asset_balances_use_rc_block_empty() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let rc_block_number = 10554958;
    let endpoint = format!(
        "/accounts/{}/foreign-asset-balances?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing foreign asset balances useRcBlock with empty RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_test(local_status.as_u16(), &local_json) {
        println!("{}", "═".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let local_array = local_json
        .as_array()
        .expect("Response with useRcBlock=true should be an array");
    assert!(
        local_array.is_empty(),
        "Expected empty array for RC block {}",
        rc_block_number
    );

    println!("{} Response is empty array as expected", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_foreign_asset_balances_invalid_filter() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let endpoint = format!(
        "/accounts/{}/foreign-asset-balances?foreignAssets[]=not-valid-json",
        account_id
    );

    println!(
        "\n{} Testing foreign asset balances with invalid filter",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Invalid filter should return 400
    assert_eq!(
        local_status.as_u16(),
        400,
        "Expected 400 Bad Request for invalid filter, got {}",
        local_status
    );

    let error_obj = local_json.as_object().unwrap();
    assert!(
        error_obj.contains_key("error"),
        "Error response missing 'error' field"
    );

    println!("{} Invalid filter error validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_foreign_asset_balances_pallet_not_available() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let early_block = 100;
    let endpoint = format!(
        "/accounts/{}/foreign-asset-balances?at={}",
        account_id, early_block
    );

    println!(
        "\n{} Testing foreign asset balances at early block {} (pallet may not exist)",
        "Testing".cyan().bold(),
        early_block.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("");
        println!(
            "  {} Expected error at early block: {}",
            "✓".green(),
            error_msg
        );
        println!(
            "{} Pallet unavailability handled gracefully!",
            "✓".green().bold()
        );
        println!("{}", "═".repeat(80).bright_white());
        return Ok(());
    }

    // If the pallet exists at this early block, just validate the response
    assert!(
        local_status.is_success(),
        "Expected either 400 or 200, got {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();
    assert!(
        response_obj.contains_key("at"),
        "Response missing 'at' field"
    );
    assert!(
        response_obj.contains_key("foreignAssets"),
        "Response missing 'foreignAssets' field"
    );

    println!(
        "{} Foreign assets pallet available at early block!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}
