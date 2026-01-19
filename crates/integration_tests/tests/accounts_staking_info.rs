//! Integration tests for /accounts/{accountId}/staking-info endpoint
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
async fn test_staking_info_basic() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known validator/nominator stash account ID for testing
    // This is a well-known Polkadot validator stash
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/accounts/{}/staking-info", account_id);

    println!(
        "\n{} Testing staking info endpoint for account {}",
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

    // Staking pallet may not exist on Asset Hub - 400 is acceptable
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("staking pallet") || error_msg.contains("not a stash") {
            println!(
                "{} Staking pallet not available or address not a stash",
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

    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at object missing 'hash' field");
    assert!(
        at_obj.contains_key("height"),
        "at object missing 'height' field"
    );

    let staking_obj = response_obj.get("staking").unwrap().as_object().unwrap();
    assert!(
        staking_obj.contains_key("stash"),
        "staking object missing 'stash' field"
    );
    assert!(
        staking_obj.contains_key("total"),
        "staking object missing 'total' field"
    );
    assert!(
        staking_obj.contains_key("active"),
        "staking object missing 'active' field"
    );
    assert!(
        staking_obj.contains_key("unlocking"),
        "staking object missing 'unlocking' field"
    );

    println!(
        "  {} Controller: {}",
        "ℹ".blue(),
        response_obj.get("controller").unwrap().as_str().unwrap_or("N/A")
    );
    println!(
        "  {} Slashing spans: {}",
        "ℹ".blue(),
        response_obj.get("numSlashingSpans").unwrap()
    );
    println!(
        "  {} Total staked: {}",
        "ℹ".blue(),
        staking_obj.get("total").unwrap().as_str().unwrap_or("N/A")
    );
    println!(
        "  {} Active: {}",
        "ℹ".blue(),
        staking_obj.get("active").unwrap().as_str().unwrap_or("N/A")
    );

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_staking_info_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = "invalid-address-123";
    let endpoint = format!("/accounts/{}/staking-info", invalid_address);

    println!(
        "\n{} Testing staking info with invalid address",
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
async fn test_staking_info_non_stash_account() -> Result<()> {
    let local_client = get_client().await?;

    // Use an account that is valid but likely not a stash account
    let account_id = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = format!("/accounts/{}/staking-info", account_id);

    println!(
        "\n{} Testing staking info for non-stash account",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Either 400 (not a stash / staking pallet unavailable) or 200 (it happens to be a stash)
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        println!(
            "{} Account is not a stash or staking unavailable: {}",
            "ℹ".blue(),
            error_msg
        );
    } else {
        assert!(
            local_status.is_success(),
            "Expected 200 or 400, got {}",
            local_status
        );
        println!("{} Account is a stash, returned staking info", "ℹ".blue());
    }

    println!("{} Non-stash account test completed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_staking_info_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let block_number = 10260000;
    let endpoint = format!("/accounts/{}/staking-info?at={}", account_id, block_number);

    println!(
        "\n{} Testing staking info at block {}",
        "Testing".cyan().bold(),
        block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Staking pallet may not exist at all blocks - 400 is acceptable
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("staking pallet") || error_msg.contains("not a stash") {
            println!(
                "{} Staking pallet not available or address not a stash at this block",
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

#[tokio::test]
async fn test_staking_info_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    // Use a recent RC block
    let rc_block_number = 26054957;
    let endpoint = format!(
        "/accounts/{}/staking-info?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing staking info with useRcBlock at RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Staking pallet may not exist or useRcBlock not supported - 400 is acceptable
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("staking pallet")
            || error_msg.contains("not a stash")
            || error_msg.contains("useRcBlock")
        {
            println!(
                "{} Staking not available or useRcBlock not supported: {}",
                "ℹ".blue(),
                error_msg
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
            item_obj.contains_key("controller"),
            "Item {} missing 'controller'",
            i
        );
        assert!(
            item_obj.contains_key("staking"),
            "Item {} missing 'staking'",
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
async fn test_staking_info_hex_address() -> Result<()> {
    let local_client = get_client().await?;

    // Use hex format address
    let account_id = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = format!("/accounts/{}/staking-info", account_id);

    println!(
        "\n{} Testing staking info with hex address",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Either valid response or error (not a stash / staking pallet unavailable)
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("staking pallet") || error_msg.contains("not a stash") {
            println!(
                "{} Staking pallet not available or address not a stash",
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

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    let response_obj = local_json.as_object().unwrap();
    assert!(
        response_obj.contains_key("at"),
        "Response missing 'at' field"
    );
    assert!(
        response_obj.contains_key("controller"),
        "Response missing 'controller' field"
    );
    assert!(
        response_obj.contains_key("staking"),
        "Response missing 'staking' field"
    );

    println!("{} Hex address validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
