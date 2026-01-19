//! Integration tests for /accounts/{accountId}/vesting-info endpoint
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
async fn test_vesting_info_basic() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known account ID for testing
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu"; // Alice
    let endpoint = format!("/accounts/{}/vesting-info", account_id);

    println!(
        "\n{} Testing vesting info endpoint for account {}",
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
        response_obj.contains_key("vesting"),
        "Response missing 'vesting' field"
    );

    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at object missing 'hash' field");
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

    // Validate each vesting schedule structure if any exist
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
            schedule_obj.get("locked").unwrap().as_str().unwrap_or("N/A"),
            schedule_obj.get("perBlock").unwrap().as_str().unwrap_or("N/A"),
            schedule_obj.get("startingBlock").unwrap().as_str().unwrap_or("N/A")
        );
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_vesting_info_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let block_number = 10260000;
    let endpoint = format!(
        "/accounts/{}/vesting-info?at={}",
        account_id, block_number
    );

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
async fn test_vesting_info_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = "invalid-address-123";
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
async fn test_vesting_info_with_include_claimable() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/accounts/{}/vesting-info?includeClaimable=true", account_id);

    println!(
        "\n{} Testing vesting info with includeClaimable=true",
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

    // Basic fields should still exist
    assert!(response_obj.contains_key("at"), "Response missing 'at' field");
    assert!(response_obj.contains_key("vesting"), "Response missing 'vesting' field");

    let vesting = response_obj.get("vesting").unwrap().as_array().unwrap();

    // If there are vesting schedules, additional fields should be present
    if !vesting.is_empty() {
        // Check for calculated fields
        assert!(
            response_obj.contains_key("vestedBalance"),
            "Response with vesting schedules missing 'vestedBalance' field"
        );
        assert!(
            response_obj.contains_key("vestingTotal"),
            "Response with vesting schedules missing 'vestingTotal' field"
        );
        assert!(
            response_obj.contains_key("vestedClaimable"),
            "Response with vesting schedules missing 'vestedClaimable' field"
        );
        assert!(
            response_obj.contains_key("blockNumberForCalculation"),
            "Response missing 'blockNumberForCalculation' field"
        );
        assert!(
            response_obj.contains_key("blockNumberSource"),
            "Response missing 'blockNumberSource' field"
        );

        // Print calculated values
        println!(
            "  {} vestedBalance: {}",
            "ℹ".blue(),
            response_obj.get("vestedBalance").unwrap().as_str().unwrap_or("N/A")
        );
        println!(
            "  {} vestingTotal: {}",
            "ℹ".blue(),
            response_obj.get("vestingTotal").unwrap().as_str().unwrap_or("N/A")
        );
        println!(
            "  {} vestedClaimable: {}",
            "ℹ".blue(),
            response_obj.get("vestedClaimable").unwrap().as_str().unwrap_or("N/A")
        );
        println!(
            "  {} blockNumberSource: {}",
            "ℹ".blue(),
            response_obj.get("blockNumberSource").unwrap().as_str().unwrap_or("N/A")
        );

        // Validate each schedule has vested field
        for (i, schedule) in vesting.iter().enumerate() {
            let schedule_obj = schedule.as_object().unwrap();
            assert!(
                schedule_obj.contains_key("vested"),
                "Schedule {} missing 'vested' field when includeClaimable=true",
                i
            );
        }
    } else {
        println!("  {} No vesting schedules found for this account", "ℹ".blue());
    }

    println!("{} includeClaimable response validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_vesting_info_schedule_structure() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
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

    // Validate schedule structure if any exist
    for (i, schedule) in vesting.iter().enumerate() {
        let schedule_obj = schedule.as_object().unwrap();

        // Required fields
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

        // Fields should be strings (serialized u128/u64)
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

        // Values should be parseable as numbers
        assert!(
            locked.parse::<u128>().is_ok(),
            "Schedule {} 'locked' is not a valid number: {}",
            i,
            locked
        );
        assert!(
            per_block.parse::<u128>().is_ok(),
            "Schedule {} 'perBlock' is not a valid number: {}",
            i,
            per_block
        );
        assert!(
            starting_block.parse::<u64>().is_ok(),
            "Schedule {} 'startingBlock' is not a valid number: {}",
            i,
            starting_block
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

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
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
    // In that case, we expect a 400 error
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap_or("");
        if error_msg.contains("vesting pallet") {
            println!(
                "  {} Vesting pallet not available on Asset Hub at this block (pre-migration)",
                "ℹ".blue()
            );
            println!("{} Test skipped - pallet not available", "✓".green().bold());
            println!("{}", "═".repeat(80).bright_white());
            return Ok(());
        }
        // Other 400 errors should fail the test
        panic!("Unexpected 400 error: {}", error_msg);
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

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    // Block 10554958 is a Relay Chain block that doesn't include any Asset Hub blocks
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
async fn test_vesting_info_use_rc_block_with_include_claimable() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let rc_block_number = 10554957;
    let endpoint = format!(
        "/accounts/{}/vesting-info?useRcBlock=true&at={}&includeClaimable=true",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing vesting info with useRcBlock and includeClaimable at RC block {}",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Vesting pallet may not exist on Asset Hub at this block (pre-migration)
    // In that case, we expect a 400 error
    if local_status.as_u16() == 400 {
        let error_obj = local_json.as_object().unwrap();
        let error_msg = error_obj.get("error").unwrap().as_str().unwrap_or("");
        if error_msg.contains("vesting pallet") {
            println!(
                "  {} Vesting pallet not available on Asset Hub at this block (pre-migration)",
                "ℹ".blue()
            );
            println!("{} Test skipped - pallet not available", "✓".green().bold());
            println!("{}", "═".repeat(80).bright_white());
            return Ok(());
        }
        // Other 400 errors should fail the test
        panic!("Unexpected 400 error: {}", error_msg);
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

        // Standard useRcBlock fields
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
            item_obj.contains_key("vesting"),
            "Item {} missing 'vesting'",
            i
        );

        let vesting = item_obj.get("vesting").unwrap().as_array().unwrap();

        // If includeClaimable=true and vesting schedules exist, should have calculation fields
        if !vesting.is_empty() {
            assert!(
                item_obj.contains_key("vestedBalance"),
                "Item {} missing 'vestedBalance'",
                i
            );
            assert!(
                item_obj.contains_key("vestingTotal"),
                "Item {} missing 'vestingTotal'",
                i
            );
            assert!(
                item_obj.contains_key("vestedClaimable"),
                "Item {} missing 'vestedClaimable'",
                i
            );

            // When useRcBlock=true and includeClaimable=true, blockNumberSource should be "relay"
            if let Some(source) = item_obj.get("blockNumberSource") {
                let source_str = source.as_str().unwrap_or("");
                println!(
                    "    Item {} blockNumberSource: {}",
                    i, source_str
                );
                assert_eq!(
                    source_str, "relay",
                    "When useRcBlock=true, blockNumberSource should be 'relay'"
                );
            }
        }
    }

    println!(
        "{} All {} block response(s) with includeClaimable validated!",
        "✓".green().bold(),
        local_array.len()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
