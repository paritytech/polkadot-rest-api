//! Integration tests for /accounts/{accountId}/balance-info endpoint
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
async fn test_balance_info_basic() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known account ID for testing
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu"; // Alice
    let endpoint = format!("/accounts/{}/balance-info", account_id);

    println!(
        "\n{} Testing balance info endpoint for account {}",
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

    // Required fields
    assert!(
        response_obj.contains_key("at"),
        "Response missing 'at' field"
    );
    assert!(
        response_obj.contains_key("nonce"),
        "Response missing 'nonce' field"
    );
    assert!(
        response_obj.contains_key("tokenSymbol"),
        "Response missing 'tokenSymbol' field"
    );
    assert!(
        response_obj.contains_key("free"),
        "Response missing 'free' field"
    );
    assert!(
        response_obj.contains_key("reserved"),
        "Response missing 'reserved' field"
    );
    assert!(
        response_obj.contains_key("miscFrozen"),
        "Response missing 'miscFrozen' field"
    );
    assert!(
        response_obj.contains_key("feeFrozen"),
        "Response missing 'feeFrozen' field"
    );
    assert!(
        response_obj.contains_key("frozen"),
        "Response missing 'frozen' field"
    );
    assert!(
        response_obj.contains_key("transferable"),
        "Response missing 'transferable' field"
    );
    assert!(
        response_obj.contains_key("locks"),
        "Response missing 'locks' field"
    );

    // Validate at structure
    let at_obj = response_obj.get("at").unwrap().as_object().unwrap();
    assert!(at_obj.contains_key("hash"), "at object missing 'hash' field");
    assert!(
        at_obj.contains_key("height"),
        "at object missing 'height' field"
    );

    // Validate locks is an array
    let locks = response_obj.get("locks").unwrap();
    assert!(locks.is_array(), "locks should be an array");

    // Print some info
    let nonce = response_obj.get("nonce").unwrap();
    let free = response_obj.get("free").unwrap();
    let token = response_obj.get("tokenSymbol").unwrap();
    println!(
        "  {} Token: {}, Nonce: {}, Free: {}",
        "ℹ".blue(),
        token.as_str().unwrap_or("N/A"),
        nonce.as_str().unwrap_or("N/A"),
        free.as_str().unwrap_or("N/A")
    );

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_balance_info_at_specific_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let block_number = 10260000;
    let endpoint = format!(
        "/accounts/{}/balance-info?at={}",
        account_id, block_number
    );

    println!(
        "\n{} Testing balance info at block {}",
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
async fn test_balance_info_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_address = "invalid-address-123";
    let endpoint = format!("/accounts/{}/balance-info", invalid_address);

    println!(
        "\n{} Testing balance info with invalid address",
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
async fn test_balance_info_with_denominated() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/accounts/{}/balance-info?denominated=true", account_id);

    println!(
        "\n{} Testing balance info with denominated=true",
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

    // Check that free balance contains a decimal point (indicating denomination)
    let free = response_obj.get("free").unwrap().as_str().unwrap();
    // If the balance is not zero, it should contain a decimal point when denominated
    if free != "0" && !free.starts_with("0.") {
        assert!(
            free.contains('.'),
            "Denominated balance should contain decimal point: {}",
            free
        );
    }

    println!(
        "  {} Denominated free balance: {}",
        "ℹ".blue(),
        free
    );

    println!("{} Denominated response validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_balance_info_locks_structure() -> Result<()> {
    let local_client = get_client().await?;

    // Use an account that might have locks
    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/accounts/{}/balance-info", account_id);

    println!(
        "\n{} Testing balance info locks structure",
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
    let locks = response_obj.get("locks").unwrap().as_array().unwrap();

    println!(
        "  {} Account has {} lock(s)",
        "ℹ".blue(),
        locks.len()
    );

    // Validate lock structure if any locks exist
    for (i, lock) in locks.iter().enumerate() {
        let lock_obj = lock.as_object().unwrap();
        assert!(
            lock_obj.contains_key("id"),
            "Lock {} missing 'id' field",
            i
        );
        assert!(
            lock_obj.contains_key("amount"),
            "Lock {} missing 'amount' field",
            i
        );
        assert!(
            lock_obj.contains_key("reasons"),
            "Lock {} missing 'reasons' field",
            i
        );

        println!(
            "    Lock {}: id={}, amount={}, reasons={}",
            i,
            lock_obj.get("id").unwrap().as_str().unwrap_or("N/A"),
            lock_obj.get("amount").unwrap().as_str().unwrap_or("N/A"),
            lock_obj.get("reasons").unwrap().as_str().unwrap_or("N/A")
        );
    }

    println!("{} Locks structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_balance_info_use_rc_block() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let rc_block_number = 10554957;
    let endpoint = format!(
        "/accounts/{}/balance-info?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing balance info with useRcBlock at RC block {}",
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
            item_obj.contains_key("free"),
            "Item {} missing 'free'",
            i
        );
        assert!(
            item_obj.contains_key("locks"),
            "Item {} missing 'locks'",
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
async fn test_balance_info_use_rc_block_empty() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    // Block 10554958 is a Relay Chain block that doesn't include any Asset Hub blocks
    let rc_block_number = 10554958;
    let endpoint = format!(
        "/accounts/{}/balance-info?useRcBlock=true&at={}",
        account_id, rc_block_number
    );

    println!(
        "\n{} Testing balance info useRcBlock with empty RC block {}",
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
async fn test_balance_info_frozen_fields() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";
    let endpoint = format!("/accounts/{}/balance-info", account_id);

    println!(
        "\n{} Testing balance info frozen fields",
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

    let misc_frozen = response_obj.get("miscFrozen").unwrap().as_str().unwrap();
    let fee_frozen = response_obj.get("feeFrozen").unwrap().as_str().unwrap();
    let frozen = response_obj.get("frozen").unwrap().as_str().unwrap();

    // Either frozen exists and miscFrozen/feeFrozen are messages, or vice versa
    let is_new_runtime = misc_frozen.contains("does not exist");
    let is_old_runtime = frozen.contains("does not exist");

    assert!(
        is_new_runtime || is_old_runtime,
        "Expected either frozen or miscFrozen/feeFrozen to indicate runtime version"
    );

    if is_new_runtime {
        println!(
            "  {} New runtime detected (uses 'frozen' field): {}",
            "ℹ".blue(),
            frozen
        );
    } else {
        println!(
            "  {} Old runtime detected (uses 'miscFrozen'/'feeFrozen' fields): {} / {}",
            "ℹ".blue(),
            misc_frozen,
            fee_frozen
        );
    }

    println!("{} Frozen fields validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
