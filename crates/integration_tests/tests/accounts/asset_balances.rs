//! Integration tests for /accounts/{accountId}/asset-balances endpoint

use super::{Colorize, get_client, test_accounts};
use anyhow::{Context, Result};
use integration_tests::utils::compare_json;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

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
async fn test_asset_balances_basic() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let endpoint = format!("/accounts/{}/asset-balances", account_id);

    println!(
        "\n{} Testing asset balances endpoint for account {}",
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
        response_obj.contains_key("assets"),
        "Response missing 'assets' field"
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

    let assets = response_obj.get("assets").unwrap().as_array().unwrap();
    println!(
        "  {} Response contains {} asset(s)",
        "✓".green(),
        assets.len()
    );

    for asset in assets {
        let asset_obj = asset.as_object().unwrap();
        assert!(
            asset_obj.contains_key("assetId"),
            "Asset missing 'assetId' field"
        );
        assert!(
            asset_obj.contains_key("balance"),
            "Asset missing 'balance' field"
        );
        assert!(
            asset_obj.contains_key("isFrozen"),
            "Asset missing 'isFrozen' field"
        );
        assert!(
            asset_obj.contains_key("isSufficient"),
            "Asset missing 'isSufficient' field"
        );
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_asset_balances_comparison() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let block_number = 10260000;
    let endpoint = format!(
        "/accounts/{}/asset-balances?at={}",
        account_id, block_number
    );

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

    let fixture_path = get_fixture_path("accounts_asset_balances_alice_10260000.json")?;
    let fixture_content = fs::read_to_string(&fixture_path)
        .with_context(|| format!("Failed to read fixture file: {:?}", fixture_path))?;
    let sidecar_json: Value = serde_json::from_str(&fixture_content)
        .context("Failed to parse expected sidecar response from fixture")?;

    let comparison_result = compare_json(&local_json, &sidecar_json, &[])?;

    if !comparison_result.is_match() {
        println!("{} JSON responses differ:", "✗".red().bold());
        let diff_output = comparison_result.format_diff(&sidecar_json, &local_json);
        println!("{}", diff_output);
    }

    assert!(
        comparison_result.is_match(),
        "Found {} difference(s) between local and expected responses",
        comparison_result.differences().len()
    );

    println!("{} All JSON responses match!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_asset_balances_with_filter() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let endpoint = format!(
        "/accounts/{}/asset-balances?assets[]=1337&assets[]=22222087",
        account_id
    );

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
    let assets = response_obj.get("assets").unwrap().as_array().unwrap();

    println!(
        "  {} Response contains {} asset(s)",
        "✓".green(),
        assets.len()
    );

    for asset in assets {
        let asset_obj = asset.as_object().unwrap();
        let asset_id = asset_obj.get("assetId").unwrap().as_u64().unwrap();
        assert!(
            asset_id == 1337 || asset_id == 22222087,
            "Unexpected asset ID: {}",
            asset_id
        );
    }

    println!("{} Filter validation passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_asset_balances_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = test_accounts::INVALID_ADDRESS;
    let endpoint = format!("/accounts/{}/asset-balances", invalid_address);

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
async fn test_asset_balances_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let rc_block_number = 10554957;
    let endpoint = format!(
        "/accounts/{}/asset-balances?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

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
            item_obj.contains_key("assets"),
            "Item {} missing 'assets'",
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
async fn test_asset_balances_use_rc_block_empty() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ASSET_HUB_ACCOUNT;
    let rc_block_number = 10554958;
    let endpoint = format!(
        "/accounts/{}/asset-balances?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

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

fn get_fixture_path(filename: &str) -> Result<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixture_path = PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join(filename);

    if !fixture_path.exists() {
        anyhow::bail!("Fixture file not found: {:?}", fixture_path);
    }

    Ok(fixture_path)
}
