//! Integration tests for /accounts/{accountId}/pool-asset-approvals endpoint

use super::{Colorize, get_client, test_accounts};
use anyhow::{Context, Result};

#[tokio::test]
async fn test_pool_asset_approvals_basic() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let delegate = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
    let asset_id = 0;
    let endpoint = format!(
        "/accounts/{}/pool-asset-approvals?assetId={}&delegate={}",
        account_id, asset_id, delegate
    );

    println!(
        "\n{} Testing pool asset approvals endpoint for account {}",
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
        response_obj.contains_key("amount"),
        "Response missing 'amount' field"
    );
    assert!(
        response_obj.contains_key("deposit"),
        "Response missing 'deposit' field"
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

    let amount = response_obj.get("amount").unwrap();
    let deposit = response_obj.get("deposit").unwrap();

    if amount.is_null() {
        println!(
            "  {} No approval found (amount: null, deposit: null)",
            "ℹ".blue()
        );
    } else {
        println!(
            "  {} Approval found - amount: {}, deposit: {}",
            "✓".green(),
            amount.as_str().unwrap_or("N/A"),
            deposit.as_str().unwrap_or("N/A")
        );
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_pool_asset_approvals_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let delegate = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
    let asset_id = 0;
    let block_number = 10260000;
    let endpoint = format!(
        "/accounts/{}/pool-asset-approvals?assetId={}&delegate={}&at={}",
        account_id, asset_id, delegate, block_number
    );

    println!(
        "\n{} Testing pool asset approvals at block {}",
        "Testing".cyan().bold(),
        block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // PoolAssets pallet may not exist at older blocks - 400 is acceptable
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("pool assets pallet") {
            println!(
                "{} PoolAssets pallet not available at this block",
                "ℹ".blue()
            );
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
async fn test_pool_asset_approvals_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = test_accounts::INVALID_ADDRESS;
    let delegate = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
    let asset_id = 0;
    let endpoint = format!(
        "/accounts/{}/pool-asset-approvals?assetId={}&delegate={}",
        invalid_address, asset_id, delegate
    );

    println!(
        "\n{} Testing pool asset approvals with invalid account address",
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
async fn test_pool_asset_approvals_invalid_delegate() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let invalid_delegate = "invalid-delegate-123";
    let asset_id = 0;
    let endpoint = format!(
        "/accounts/{}/pool-asset-approvals?assetId={}&delegate={}",
        account_id, asset_id, invalid_delegate
    );

    println!(
        "\n{} Testing pool asset approvals with invalid delegate address",
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
        error_msg.contains("Invalid delegate address"),
        "Error message doesn't contain expected text: {}",
        error_msg
    );

    println!("{} Error message validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_pool_asset_approvals_missing_required_params() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;

    println!(
        "\n{} Testing pool asset approvals with missing required parameters",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    // Test missing assetId
    let endpoint_no_asset = format!(
        "/accounts/{}/pool-asset-approvals?delegate=15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5",
        account_id
    );
    let response_no_asset = local_client
        .get(&format!("/v1{}", endpoint_no_asset))
        .await
        .context("Failed to fetch from local API")?;

    assert_eq!(
        response_no_asset.status.as_u16(),
        400,
        "Expected 400 for missing assetId"
    );
    assert!(
        response_no_asset.body.contains("assetId"),
        "Should mention missing assetId"
    );
    println!("{} Missing assetId returns 400", "✓".green());

    // Test missing delegate
    let endpoint_no_delegate = format!("/accounts/{}/pool-asset-approvals?assetId=0", account_id);
    let response_no_delegate = local_client
        .get(&format!("/v1{}", endpoint_no_delegate))
        .await
        .context("Failed to fetch from local API")?;

    assert_eq!(
        response_no_delegate.status.as_u16(),
        400,
        "Expected 400 for missing delegate"
    );
    assert!(
        response_no_delegate.body.contains("delegate"),
        "Should mention missing delegate"
    );
    println!("{} Missing delegate returns 400", "✓".green());

    println!(
        "{} Required parameter validation passed!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_pool_asset_approvals_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let delegate = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
    let asset_id = 0;
    let rc_block_number = 26054957;
    let endpoint = format!(
        "/accounts/{}/pool-asset-approvals?assetId={}&delegate={}&useRcBlock=true&at={}",
        account_id, asset_id, delegate, rc_block_number
    );

    println!(
        "\n{} Testing pool asset approvals with useRcBlock at RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // PoolAssets pallet may not exist at all blocks
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("pool assets pallet") {
            println!(
                "{} PoolAssets pallet not available at this block",
                "ℹ".blue()
            );
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
            item_obj.contains_key("amount"),
            "Item {} missing 'amount'",
            i
        );
        assert!(
            item_obj.contains_key("deposit"),
            "Item {} missing 'deposit'",
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
async fn test_pool_asset_approvals_non_existent_approval() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let delegate = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let asset_id = 999999;
    let endpoint = format!(
        "/accounts/{}/pool-asset-approvals?assetId={}&delegate={}",
        account_id, asset_id, delegate
    );

    println!(
        "\n{} Testing pool asset approvals for non-existent approval",
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
    let amount = response_obj.get("amount").unwrap();
    let deposit = response_obj.get("deposit").unwrap();

    assert!(
        amount.is_null(),
        "Expected null amount for non-existent approval"
    );
    assert!(
        deposit.is_null(),
        "Expected null deposit for non-existent approval"
    );

    println!(
        "{} Non-existent approval returns null values as expected!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}
