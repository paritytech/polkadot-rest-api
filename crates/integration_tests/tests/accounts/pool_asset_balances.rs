//! Integration tests for /accounts/{accountId}/pool-asset-balances endpoint

use super::{get_client, Colorize};
use anyhow::{Context, Result};

#[tokio::test]
async fn test_pool_asset_balances_basic() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/accounts/{}/pool-asset-balances", account_id);

    println!(
        "\n{} Testing pool asset balances endpoint for account {}",
        "Testing".cyan().bold(),
        account_id.yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(local_status.is_success(), "Local API returned status {}", local_status);

    let response_obj = local_json.as_object().expect("Response is not an object");

    assert!(response_obj.contains_key("at"), "Response missing 'at' field");
    assert!(response_obj.contains_key("poolAssets"), "Response missing 'poolAssets' field");

    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at object missing 'hash' field");
    assert!(at_obj.contains_key("height"), "at object missing 'height' field");

    let pool_assets = response_obj.get("poolAssets").unwrap().as_array().unwrap();
    println!("  {} Response contains {} pool asset(s)", "✓".green(), pool_assets.len());

    for asset in pool_assets {
        let asset_obj = asset.as_object().unwrap();
        assert!(asset_obj.contains_key("assetId"), "Pool asset missing 'assetId' field");
        assert!(asset_obj.contains_key("balance"), "Pool asset missing 'balance' field");
        assert!(asset_obj.contains_key("isFrozen"), "Pool asset missing 'isFrozen' field");
        assert!(asset_obj.contains_key("isSufficient"), "Pool asset missing 'isSufficient' field");
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_pool_asset_balances_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let block_number = 10260000;
    let endpoint = format!("/accounts/{}/pool-asset-balances?at={}", account_id, block_number);

    println!(
        "\n{} Testing pool asset balances at block {}",
        "Testing".cyan().bold(),
        block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(local_status.is_success(), "Local API returned status {}", local_status);

    let response_obj = local_json.as_object().unwrap();
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    let height = at_obj.get("height").unwrap().as_str().unwrap();
    assert_eq!(height, block_number.to_string(), "Block height mismatch");

    println!("{} Response at block {} validated!", "✓".green().bold(), block_number);
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_pool_asset_balances_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = "invalid-address-123";
    let endpoint = format!("/accounts/{}/pool-asset-balances", invalid_address);

    println!(
        "\n{} Testing pool asset balances with invalid address",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert_eq!(local_status.as_u16(), 400, "Expected 400 Bad Request, got {}", local_status);

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
async fn test_pool_asset_balances_with_filter() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/accounts/{}/pool-asset-balances?assets[]=0&assets[]=1", account_id);

    println!(
        "\n{} Testing pool asset balances with filter",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(local_status.is_success(), "Local API returned status {}", local_status);

    let response_obj = local_json.as_object().unwrap();
    let pool_assets = response_obj.get("poolAssets").unwrap().as_array().unwrap();

    println!("  {} Response contains {} pool asset(s)", "✓".green(), pool_assets.len());

    for asset in pool_assets {
        let asset_obj = asset.as_object().unwrap();
        let asset_id = asset_obj.get("assetId").unwrap().as_u64().unwrap();
        assert!(asset_id == 0 || asset_id == 1, "Unexpected pool asset ID: {}", asset_id);
    }

    println!("{} Filter validation passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_pool_asset_balances_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let rc_block_number = 26054957;
    let endpoint = format!(
        "/accounts/{}/pool-asset-balances?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing pool asset balances with useRcBlock at RC block {}",
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
            println!("{} PoolAssets pallet not available at this block", "ℹ".blue());
            println!("{}", "═".repeat(80).bright_white());
            return Ok(());
        }
    }

    assert!(local_status.is_success(), "Local API returned status {}", local_status);

    let local_array = local_json.as_array().expect("Response with useRcBlock=true should be an array");

    println!("  {} Response contains {} block(s)", "✓".green(), local_array.len());

    for (i, item) in local_array.iter().enumerate() {
        let item_obj = item.as_object().unwrap();
        assert!(item_obj.contains_key("rcBlockHash"), "Item {} missing 'rcBlockHash'", i);
        assert!(item_obj.contains_key("rcBlockNumber"), "Item {} missing 'rcBlockNumber'", i);
        assert!(item_obj.contains_key("ahTimestamp"), "Item {} missing 'ahTimestamp'", i);
        assert!(item_obj.contains_key("at"), "Item {} missing 'at'", i);
        assert!(item_obj.contains_key("poolAssets"), "Item {} missing 'poolAssets'", i);

        let rc_block_num = item_obj.get("rcBlockNumber").unwrap().as_str().unwrap();
        assert_eq!(rc_block_num, rc_block_number.to_string(), "RC block number mismatch");
    }

    println!("{} All {} block response(s) validated!", "✓".green().bold(), local_array.len());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_pool_asset_balances_hex_address() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = format!("/accounts/{}/pool-asset-balances", account_id);

    println!(
        "\n{} Testing pool asset balances with hex address",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(local_status.is_success(), "Local API returned status {}", local_status);

    let response_obj = local_json.as_object().unwrap();
    assert!(response_obj.contains_key("at"), "Response missing 'at' field");
    assert!(response_obj.contains_key("poolAssets"), "Response missing 'poolAssets' field");

    println!("{} Hex address validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}
