// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for /accounts/{accountId}/proxy-info endpoint

use super::{Colorize, get_client, test_accounts};
use anyhow::{Context, Result};

#[tokio::test]
async fn test_proxy_info_basic() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let endpoint = format!("/accounts/{}/proxy-info", account_id);

    println!(
        "\n{} Testing proxy info endpoint for account {}",
        "Testing".cyan().bold(),
        account_id.yellow()
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

    let response_obj = local_json.as_object().expect("Response is not an object");

    assert!(
        response_obj.contains_key("at"),
        "Response missing 'at' field"
    );
    assert!(
        response_obj.contains_key("delegatedAccounts"),
        "Response missing 'delegatedAccounts' field"
    );
    assert!(
        response_obj.contains_key("depositHeld"),
        "Response missing 'depositHeld' field"
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

    let delegated_accounts = response_obj
        .get("delegatedAccounts")
        .unwrap()
        .as_array()
        .unwrap();
    let deposit_held = response_obj.get("depositHeld").unwrap().as_str().unwrap();

    println!(
        "  {} Account has {} proxy delegation(s), deposit: {}",
        "ℹ".blue(),
        delegated_accounts.len(),
        deposit_held
    );

    for (i, proxy) in delegated_accounts.iter().enumerate() {
        let proxy_obj = proxy.as_object().unwrap();
        assert!(
            proxy_obj.contains_key("delegate"),
            "Proxy {} missing 'delegate' field",
            i
        );
        assert!(
            proxy_obj.contains_key("proxyType"),
            "Proxy {} missing 'proxyType' field",
            i
        );
        assert!(
            proxy_obj.contains_key("delay"),
            "Proxy {} missing 'delay' field",
            i
        );

        println!(
            "    Proxy {}: delegate={}, type={}, delay={}",
            i,
            proxy_obj.get("delegate").unwrap().as_str().unwrap_or("N/A"),
            proxy_obj
                .get("proxyType")
                .unwrap()
                .as_str()
                .unwrap_or("N/A"),
            proxy_obj.get("delay").unwrap().as_str().unwrap_or("N/A")
        );
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_proxy_info_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let block_number = 10260000;
    let endpoint = format!("/accounts/{}/proxy-info?at={}", account_id, block_number);

    println!(
        "\n{} Testing proxy info at block {}",
        "Testing".cyan().bold(),
        block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Proxy pallet may not exist at older blocks
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("proxy pallet") {
            println!("{} Proxy pallet not available at this block", "ℹ".blue());
            println!("{}", "═".repeat(80).bright_white());
            return Ok(());
        }
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
async fn test_proxy_info_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = test_accounts::INVALID_ADDRESS;
    let endpoint = format!("/accounts/{}/proxy-info", invalid_address);

    println!(
        "\n{} Testing proxy info with invalid address",
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
async fn test_proxy_info_no_proxies() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ALICE_HEX;
    let endpoint = format!("/accounts/{}/proxy-info", account_id);

    println!(
        "\n{} Testing proxy info for account with no proxies",
        "Testing".cyan().bold()
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

    let response_obj = local_json.as_object().unwrap();
    let delegated_accounts = response_obj
        .get("delegatedAccounts")
        .unwrap()
        .as_array()
        .unwrap();
    let deposit_held = response_obj.get("depositHeld").unwrap().as_str().unwrap();

    println!(
        "  {} Delegated accounts: {}, Deposit held: {}",
        "ℹ".blue(),
        delegated_accounts.len(),
        deposit_held
    );

    if delegated_accounts.is_empty() {
        assert_eq!(
            deposit_held, "0",
            "Expected deposit to be 0 when no proxies"
        );
    }

    println!("{} No proxies response validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_proxy_info_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let rc_block_number = 26054957;
    let endpoint = format!(
        "/accounts/{}/proxy-info?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing proxy info with useRcBlock at RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Proxy pallet may not exist at all blocks
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("proxy pallet") {
            println!("{} Proxy pallet not available at this block", "ℹ".blue());
            println!("{}", "═".repeat(80).bright_white());
            return Ok(());
        }
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
            item_obj.contains_key("delegatedAccounts"),
            "Item {} missing 'delegatedAccounts'",
            i
        );
        assert!(
            item_obj.contains_key("depositHeld"),
            "Item {} missing 'depositHeld'",
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
async fn test_proxy_info_hex_address() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ALICE_HEX;
    let endpoint = format!("/accounts/{}/proxy-info", account_id);

    println!(
        "\n{} Testing proxy info with hex address",
        "Testing".cyan().bold()
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

    let response_obj = local_json.as_object().unwrap();
    assert!(
        response_obj.contains_key("at"),
        "Response missing 'at' field"
    );
    assert!(
        response_obj.contains_key("delegatedAccounts"),
        "Response missing 'delegatedAccounts' field"
    );
    assert!(
        response_obj.contains_key("depositHeld"),
        "Response missing 'depositHeld' field"
    );

    println!("{} Hex address validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}
