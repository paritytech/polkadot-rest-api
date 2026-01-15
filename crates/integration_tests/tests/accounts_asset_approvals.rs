//! Integration tests for /accounts/{accountId}/asset-approvals endpoint
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

    // Wait for API readiness (only blocks on first call, idempotent after)
    client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    // Return a cheap clone - tests can use this concurrently
    Ok(client.clone())
}

#[tokio::test]
async fn test_asset_approvals_basic() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known account ID for testing
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu"; // Alice
    let delegate = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5"; // Bob
    let asset_id = 1984; // USDT on Asset Hub
    let endpoint = format!(
        "/accounts/{}/asset-approvals?assetId={}&delegate={}",
        account_id, asset_id, delegate
    );

    println!(
        "\n{} Testing asset approvals endpoint for account {}",
        "Testing".cyan().bold(),
        account_id.yellow()
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

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    // Validate response structure
    let response_obj = local_json
        .as_object()
        .expect("Response is not an object");

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
    assert!(at_obj.contains_key("hash"), "at object missing 'hash' field");
    assert!(
        at_obj.contains_key("height"),
        "at object missing 'height' field"
    );

    // amount and deposit can be null if no approval exists
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
async fn test_asset_approvals_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let delegate = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
    let asset_id = 1984;
    let block_number = 10260000;
    let endpoint = format!(
        "/accounts/{}/asset-approvals?assetId={}&delegate={}&at={}",
        account_id, asset_id, delegate, block_number
    );

    println!(
        "\n{} Testing asset approvals at block {}",
        "Testing".cyan().bold(),
        block_number.to_string().yellow()
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

    // Verify block height matches
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    let height = at_obj.get("height").unwrap().as_str().unwrap();
    assert_eq!(
        height,
        block_number.to_string(),
        "Block height mismatch"
    );

    println!(
        "{} Response at block {} validated!",
        "✓".green().bold(),
        block_number
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_asset_approvals_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = "invalid-address-123";
    let delegate = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
    let asset_id = 1984;
    let endpoint = format!(
        "/accounts/{}/asset-approvals?assetId={}&delegate={}",
        invalid_address, asset_id, delegate
    );

    println!(
        "\n{} Testing asset approvals with invalid account address",
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

    println!("{} Received expected 400 Bad Request", "✓".green());

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
async fn test_asset_approvals_invalid_delegate() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let invalid_delegate = "invalid-delegate-123";
    let asset_id = 1984;
    let endpoint = format!(
        "/accounts/{}/asset-approvals?assetId={}&delegate={}",
        account_id, asset_id, invalid_delegate
    );

    println!(
        "\n{} Testing asset approvals with invalid delegate address",
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

    println!("{} Received expected 400 Bad Request", "✓".green());

    let error_obj = local_json.as_object().unwrap();
    assert!(
        error_obj.contains_key("error"),
        "Error response missing 'error' field"
    );

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
async fn test_asset_approvals_missing_required_params() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";

    println!(
        "\n{} Testing asset approvals with missing required parameters",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    // Test missing assetId - uses get() instead of get_json() because Axum returns plain text for missing query params
    let endpoint_no_asset = format!(
        "/accounts/{}/asset-approvals?delegate=15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5",
        account_id
    );
    let response_no_asset = local_client
        .get(&format!("/v1{}", endpoint_no_asset))
        .await
        .context("Failed to fetch from local API")?;

    assert_eq!(
        response_no_asset.status.as_u16(),
        400,
        "Expected 400 Bad Request for missing assetId, got {}",
        response_no_asset.status
    );
    assert!(
        response_no_asset.body.contains("assetId"),
        "Error message should mention missing assetId"
    );
    println!("{} Missing assetId returns 400", "✓".green());

    // Test missing delegate - uses get() instead of get_json() because Axum returns plain text for missing query params
    let endpoint_no_delegate = format!(
        "/accounts/{}/asset-approvals?assetId=1984",
        account_id
    );
    let response_no_delegate = local_client
        .get(&format!("/v1{}", endpoint_no_delegate))
        .await
        .context("Failed to fetch from local API")?;

    assert_eq!(
        response_no_delegate.status.as_u16(),
        400,
        "Expected 400 Bad Request for missing delegate, got {}",
        response_no_delegate.status
    );
    assert!(
        response_no_delegate.body.contains("delegate"),
        "Error message should mention missing delegate"
    );
    println!("{} Missing delegate returns 400", "✓".green());

    println!("{} Required parameter validation passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_asset_approvals_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let delegate = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
    let asset_id = 1984;
    let rc_block_number = 10554957;
    let endpoint = format!(
        "/accounts/{}/asset-approvals?assetId={}&delegate={}&useRcBlock=true&at={}",
        account_id, asset_id, delegate, rc_block_number
    );

    println!(
        "\n{} Testing asset approvals with useRcBlock at RC block {}",
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
async fn test_asset_approvals_use_rc_block_empty() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let delegate = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
    let asset_id = 1984;
    // Block 10554958 is a Relay Chain block that doesn't include any Asset Hub blocks
    let rc_block_number = 10554958;
    let endpoint = format!(
        "/accounts/{}/asset-approvals?assetId={}&delegate={}&useRcBlock=true&at={}",
        account_id, asset_id, delegate, rc_block_number
    );

    println!(
        "\n{} Testing asset approvals useRcBlock with empty RC block {}",
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

#[tokio::test]
async fn test_asset_approvals_non_existent_approval() -> Result<()> {
    let local_client = get_client().await?;

    // Use accounts that are unlikely to have approvals set up
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu"; // Alice
    let delegate = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu"; // Same as owner (self-approval unlikely)
    let asset_id = 999999; // Non-existent asset ID
    let endpoint = format!(
        "/accounts/{}/asset-approvals?assetId={}&delegate={}",
        account_id, asset_id, delegate
    );

    println!(
        "\n{} Testing asset approvals for non-existent approval",
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

    // When no approval exists, amount and deposit should be null
    let amount = response_obj.get("amount").unwrap();
    let deposit = response_obj.get("deposit").unwrap();

    assert!(
        amount.is_null(),
        "Expected null amount for non-existent approval, got {:?}",
        amount
    );
    assert!(
        deposit.is_null(),
        "Expected null deposit for non-existent approval, got {:?}",
        deposit
    );

    println!(
        "{} Non-existent approval returns null values as expected!",
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
