//! Integration tests for /accounts/{accountId}/staking-payouts endpoint
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
async fn test_staking_payouts_basic() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known nominator/validator stash account ID for testing
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/accounts/{}/staking-payouts", account_id);

    println!(
        "\n{} Testing staking payouts endpoint for account {}",
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
        if error_msg.contains("staking pallet") || error_msg.contains("No active era") {
            println!(
                "{} Staking pallet not available or no active era",
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
        response_obj.contains_key("erasPayouts"),
        "Response missing 'erasPayouts' field"
    );

    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at object missing 'hash' field");
    assert!(
        at_obj.contains_key("height"),
        "at object missing 'height' field"
    );

    let eras_payouts = response_obj.get("erasPayouts").unwrap().as_array().unwrap();
    println!(
        "  {} Response contains {} era(s)",
        "ℹ".blue(),
        eras_payouts.len()
    );

    // Validate structure of each era payout
    for (i, era_payout) in eras_payouts.iter().enumerate() {
        let era_obj = era_payout.as_object().unwrap();

        // Could be either payout data or a message
        if era_obj.contains_key("message") {
            println!(
                "  {} Era {}: {}",
                "ℹ".blue(),
                i,
                era_obj.get("message").unwrap().as_str().unwrap()
            );
        } else {
            assert!(
                era_obj.contains_key("era"),
                "Era payout {} missing 'era' field",
                i
            );
            assert!(
                era_obj.contains_key("totalEraRewardPoints"),
                "Era payout {} missing 'totalEraRewardPoints' field",
                i
            );
            assert!(
                era_obj.contains_key("totalEraPayout"),
                "Era payout {} missing 'totalEraPayout' field",
                i
            );
            assert!(
                era_obj.contains_key("payouts"),
                "Era payout {} missing 'payouts' field",
                i
            );

            let era = era_obj.get("era").unwrap();
            let payouts = era_obj.get("payouts").unwrap().as_array().unwrap();
            println!(
                "  {} Era {}: {} payout(s)",
                "ℹ".blue(),
                era,
                payouts.len()
            );
        }
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_staking_payouts_with_depth() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let depth = 3;
    let endpoint = format!("/accounts/{}/staking-payouts?depth={}", account_id, depth);

    println!(
        "\n{} Testing staking payouts with depth={}",
        "Testing".cyan().bold(),
        depth.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Staking pallet may not exist - 400 is acceptable
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("staking pallet") || error_msg.contains("No active era") {
            println!(
                "{} Staking pallet not available or no active era",
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
    let eras_payouts = response_obj.get("erasPayouts").unwrap().as_array().unwrap();

    // With depth=3, we should have up to 3 eras
    assert!(
        eras_payouts.len() <= depth as usize,
        "Expected at most {} eras, got {}",
        depth,
        eras_payouts.len()
    );

    println!(
        "{} Depth={} returned {} era(s)",
        "✓".green().bold(),
        depth,
        eras_payouts.len()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_staking_payouts_unclaimed_only_false() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!(
        "/accounts/{}/staking-payouts?unclaimedOnly=false",
        account_id
    );

    println!(
        "\n{} Testing staking payouts with unclaimedOnly=false",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Staking pallet may not exist - 400 is acceptable
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("staking pallet") || error_msg.contains("No active era") {
            println!(
                "{} Staking pallet not available or no active era",
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

    println!(
        "{} unclaimedOnly=false response validated!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_staking_payouts_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = "invalid-address-123";
    let endpoint = format!("/accounts/{}/staking-payouts", invalid_address);

    println!(
        "\n{} Testing staking payouts with invalid address",
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
async fn test_staking_payouts_invalid_depth() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/accounts/{}/staking-payouts?depth=0", account_id);

    println!(
        "\n{} Testing staking payouts with invalid depth=0",
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
    let error_msg = error_obj.get("error").unwrap().as_str().unwrap();

    // Could be depth error or staking pallet not available
    println!("  {} Error: {}", "ℹ".blue(), error_msg);

    println!("{} Invalid depth error validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_staking_payouts_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    // Use a recent RC block
    let rc_block_number = 26054957;
    let endpoint = format!(
        "/accounts/{}/staking-payouts?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing staking payouts with useRcBlock at RC block {}",
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
            || error_msg.contains("No active era")
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
        assert!(item_obj.contains_key("at"), "Item {} missing 'at'", i);
        assert!(
            item_obj.contains_key("erasPayouts"),
            "Item {} missing 'erasPayouts'",
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
async fn test_staking_payouts_hex_address() -> Result<()> {
    let local_client = get_client().await?;

    // Use hex format address
    let account_id = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = format!("/accounts/{}/staking-payouts", account_id);

    println!(
        "\n{} Testing staking payouts with hex address",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Either valid response or error (staking pallet unavailable)
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
        if error_msg.contains("staking pallet") || error_msg.contains("No active era") {
            println!(
                "{} Staking pallet not available or no active era",
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
        response_obj.contains_key("erasPayouts"),
        "Response missing 'erasPayouts' field"
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
