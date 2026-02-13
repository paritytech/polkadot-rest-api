// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for /accounts/{accountId}/vesting-info endpoint

use super::{Colorize, get_client, test_accounts};
use anyhow::{Context, Result};

/// Check if error indicates vesting pallet not available
fn is_vesting_unavailable(status: u16, json: &serde_json::Value) -> bool {
    if status != 400 && status != 500 {
        return false;
    }
    if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
        let error_str = error.as_str().unwrap_or("");
        return error_str.contains("vesting")
            || error_str.contains("Vesting")
            || error_str.contains("pallet")
            || error_str.contains("not found");
    }
    false
}

/// Handle skip conditions for vesting endpoints
/// Returns true if test should be skipped
fn should_skip_vesting_test(status: u16, json: &serde_json::Value) -> bool {
    if is_vesting_unavailable(status, json) {
        if let Some(error) = json.as_object().and_then(|o| o.get("error")) {
            let error_str = error.as_str().unwrap_or("");
            println!(
                "  {} Vesting pallet not available (skipping test): {}",
                "!".yellow(),
                error_str
            );
        } else {
            println!(
                "  {} Vesting pallet not available (skipping test)",
                "!".yellow()
            );
        }
        return true;
    }
    false
}

#[tokio::test]
async fn test_vesting_info_basic() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let endpoint = format!("/accounts/{}/vesting-info", account_id);

    println!(
        "\n{} Testing vesting info endpoint for account {}",
        "Testing".cyan().bold(),
        account_id.yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_vesting_test(local_status.as_u16(), &local_json) {
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
        response_obj.contains_key("vesting"),
        "Response missing 'vesting' field"
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

    let vesting = response_obj.get("vesting").unwrap().as_array().unwrap();
    println!(
        "  {} Response contains {} vesting schedule(s)",
        "✓".green(),
        vesting.len()
    );

    for (i, schedule) in vesting.iter().enumerate() {
        let schedule_obj = schedule.as_object().unwrap();
        assert!(
            schedule_obj.contains_key("locked"),
            "Schedule {} missing 'locked' field",
            i
        );
        assert!(
            schedule_obj.contains_key("perBlock"),
            "Schedule {} missing 'perBlock' field",
            i
        );
        assert!(
            schedule_obj.contains_key("startingBlock"),
            "Schedule {} missing 'startingBlock' field",
            i
        );

        println!(
            "    Schedule {}: locked={}, perBlock={}, startingBlock={}",
            i,
            schedule_obj
                .get("locked")
                .unwrap()
                .as_str()
                .unwrap_or("N/A"),
            schedule_obj
                .get("perBlock")
                .unwrap()
                .as_str()
                .unwrap_or("N/A"),
            schedule_obj
                .get("startingBlock")
                .unwrap()
                .as_str()
                .unwrap_or("N/A")
        );
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_vesting_info_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let block_number = 10260000;
    let endpoint = format!("/accounts/{}/vesting-info?at={}", account_id, block_number);

    println!(
        "\n{} Testing vesting info at block {}",
        "Testing".cyan().bold(),
        block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_vesting_test(local_status.as_u16(), &local_json) {
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
async fn test_vesting_info_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = test_accounts::INVALID_ADDRESS;
    let endpoint = format!("/accounts/{}/vesting-info", invalid_address);

    println!(
        "\n{} Testing vesting info with invalid address",
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
async fn test_vesting_info_schedule_structure() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let endpoint = format!("/accounts/{}/vesting-info", account_id);

    println!(
        "\n{} Testing vesting info schedule structure",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_vesting_test(local_status.as_u16(), &local_json) {
        println!("{}", "═".repeat(80).bright_white());
        return Ok(());
    }

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();
    let vesting = response_obj.get("vesting").unwrap().as_array().unwrap();

    println!(
        "  {} Account has {} vesting schedule(s)",
        "ℹ".blue(),
        vesting.len()
    );

    for (i, schedule) in vesting.iter().enumerate() {
        let schedule_obj = schedule.as_object().unwrap();

        assert!(
            schedule_obj.contains_key("locked"),
            "Schedule {} missing 'locked' field",
            i
        );
        assert!(
            schedule_obj.contains_key("perBlock"),
            "Schedule {} missing 'perBlock' field",
            i
        );
        assert!(
            schedule_obj.contains_key("startingBlock"),
            "Schedule {} missing 'startingBlock' field",
            i
        );

        assert!(
            schedule_obj.get("locked").unwrap().is_string(),
            "Schedule {} 'locked' should be a string",
            i
        );
        assert!(
            schedule_obj.get("perBlock").unwrap().is_string(),
            "Schedule {} 'perBlock' should be a string",
            i
        );
        assert!(
            schedule_obj.get("startingBlock").unwrap().is_string(),
            "Schedule {} 'startingBlock' should be a string",
            i
        );

        let locked = schedule_obj.get("locked").unwrap().as_str().unwrap();
        let per_block = schedule_obj.get("perBlock").unwrap().as_str().unwrap();
        let starting_block = schedule_obj.get("startingBlock").unwrap().as_str().unwrap();

        assert!(
            locked.parse::<u128>().is_ok(),
            "Schedule {} 'locked' is not a valid number",
            i
        );
        assert!(
            per_block.parse::<u128>().is_ok(),
            "Schedule {} 'perBlock' is not a valid number",
            i
        );
        assert!(
            starting_block.parse::<u64>().is_ok(),
            "Schedule {} 'startingBlock' is not a valid number",
            i
        );

        println!(
            "    Schedule {}: locked={}, perBlock={}, startingBlock={}",
            i, locked, per_block, starting_block
        );
    }

    println!("{} Schedule structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_vesting_info_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let rc_block_number = 10554957;
    let endpoint = format!(
        "/accounts/{}/vesting-info?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing vesting info with useRcBlock at RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Vesting pallet may not exist on Asset Hub at this block (pre-migration)
    if should_skip_vesting_test(local_status.as_u16(), &local_json) {
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
            item_obj.contains_key("vesting"),
            "Item {} missing 'vesting'",
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
async fn test_vesting_info_use_rc_block_empty() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let rc_block_number = 10554958;
    let endpoint = format!(
        "/accounts/{}/vesting-info?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing vesting info useRcBlock with empty RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    if should_skip_vesting_test(local_status.as_u16(), &local_json) {
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
