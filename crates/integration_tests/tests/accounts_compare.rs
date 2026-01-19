//! Integration tests for /accounts/compare endpoint
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
async fn test_compare_same_account_different_formats() -> Result<()> {
    let local_client = get_client().await?;

    // Same account in different SS58 formats (Polkadot, Kusama, Substrate generic)
    // These addresses all represent the same underlying public key
    let polkadot_addr = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let kusama_addr = "CpjsLDC1JFyrhm3ftC9Gs4QoyrkHKhZKtK7YqGTRFtTafgp";
    let substrate_addr = "5C4hrfjw9DjXZTzV3MwzrrAr9P1MJhSrvWGWqi1eSuyUpnhM";

    let endpoint = format!(
        "/accounts/compare?addresses={},{},{}",
        polkadot_addr, kusama_addr, substrate_addr
    );

    println!(
        "\n{} Testing compare endpoint with same account in different formats",
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

    let response_obj = local_json.as_object().expect("Response is not an object");

    // Validate response structure
    assert!(
        response_obj.contains_key("areEqual"),
        "Response missing 'areEqual' field"
    );
    assert!(
        response_obj.contains_key("addresses"),
        "Response missing 'addresses' field"
    );

    let are_equal = response_obj.get("areEqual").unwrap().as_bool().unwrap();
    assert!(are_equal, "Expected addresses to be equal (same public key)");

    let addresses = response_obj.get("addresses").unwrap().as_array().unwrap();
    assert_eq!(addresses.len(), 3, "Expected 3 addresses in response");

    // Verify all addresses have the same public key
    let first_public_key = addresses[0]
        .as_object()
        .unwrap()
        .get("publicKey")
        .unwrap()
        .as_str()
        .unwrap();

    for addr in addresses.iter() {
        let addr_obj = addr.as_object().unwrap();
        let public_key = addr_obj.get("publicKey").unwrap().as_str().unwrap();
        assert_eq!(
            public_key, first_public_key,
            "All addresses should have the same public key"
        );
    }

    // Verify different prefixes
    let prefixes: Vec<u64> = addresses
        .iter()
        .map(|a| a.as_object().unwrap().get("ss58Prefix").unwrap().as_u64().unwrap())
        .collect();

    assert!(prefixes.contains(&0), "Should contain Polkadot prefix (0)");
    assert!(prefixes.contains(&2), "Should contain Kusama prefix (2)");
    assert!(prefixes.contains(&42), "Should contain Substrate prefix (42)");

    println!("  {} areEqual: {}", "✓".green(), are_equal);
    println!("  {} All addresses share public key: {}", "✓".green(), first_public_key);
    println!("  {} Prefixes: {:?}", "✓".green(), prefixes);

    println!(
        "{} Same account in different formats validated!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_compare_different_accounts() -> Result<()> {
    let local_client = get_client().await?;

    // Two different accounts (different public keys)
    let addr1 = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let addr2 = "12xLgPQunSsPkwMJ3vAgfac7mtU3Xw6R4fbHQcCp2QqXzdtu";

    let endpoint = format!("/accounts/compare?addresses={},{}", addr1, addr2);

    println!(
        "\n{} Testing compare endpoint with different accounts",
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

    let response_obj = local_json.as_object().expect("Response is not an object");

    let are_equal = response_obj.get("areEqual").unwrap().as_bool().unwrap();
    assert!(!are_equal, "Expected addresses to NOT be equal (different public keys)");

    let addresses = response_obj.get("addresses").unwrap().as_array().unwrap();
    assert_eq!(addresses.len(), 2, "Expected 2 addresses in response");

    // Verify different public keys
    let pk1 = addresses[0]
        .as_object()
        .unwrap()
        .get("publicKey")
        .unwrap()
        .as_str()
        .unwrap();
    let pk2 = addresses[1]
        .as_object()
        .unwrap()
        .get("publicKey")
        .unwrap()
        .as_str()
        .unwrap();

    assert_ne!(pk1, pk2, "Public keys should be different");

    println!("  {} areEqual: {}", "✓".green(), are_equal);
    println!("  {} publicKey1: {}", "✓".green(), pk1);
    println!("  {} publicKey2: {}", "✓".green(), pk2);

    println!(
        "{} Different accounts correctly identified!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_compare_single_address() -> Result<()> {
    let local_client = get_client().await?;

    let addr = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let endpoint = format!("/accounts/compare?addresses={}", addr);

    println!(
        "\n{} Testing compare endpoint with single address",
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

    let response_obj = local_json.as_object().expect("Response is not an object");

    let are_equal = response_obj.get("areEqual").unwrap().as_bool().unwrap();
    assert!(are_equal, "Single address should be equal to itself");

    let addresses = response_obj.get("addresses").unwrap().as_array().unwrap();
    assert_eq!(addresses.len(), 1, "Expected 1 address in response");

    println!("  {} areEqual: {}", "✓".green(), are_equal);
    println!("  {} Single address handled correctly", "✓".green());

    println!("{} Single address comparison validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_compare_with_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    // Mix of valid and invalid addresses
    let valid_addr = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let invalid_addr = "invalid-address-123";

    let endpoint = format!("/accounts/compare?addresses={},{}", valid_addr, invalid_addr);

    println!(
        "\n{} Testing compare endpoint with invalid address",
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

    let response_obj = local_json.as_object().expect("Response is not an object");

    let are_equal = response_obj.get("areEqual").unwrap().as_bool().unwrap();
    assert!(
        !are_equal,
        "Should not be equal when one address is invalid"
    );

    let addresses = response_obj.get("addresses").unwrap().as_array().unwrap();
    assert_eq!(addresses.len(), 2, "Expected 2 addresses in response");

    // First address should be valid
    let first_addr = addresses[0].as_object().unwrap();
    assert!(
        first_addr.get("publicKey").is_some()
            && !first_addr.get("publicKey").unwrap().is_null(),
        "First address should have publicKey"
    );

    // Second address should be invalid (null fields)
    let second_addr = addresses[1].as_object().unwrap();
    let second_public_key = second_addr.get("publicKey");
    assert!(
        second_public_key.is_none() || second_public_key.unwrap().is_null(),
        "Invalid address should have null publicKey"
    );

    println!("  {} areEqual: {}", "✓".green(), are_equal);
    println!("  {} Invalid address handled gracefully", "✓".green());

    println!(
        "{} Invalid address in compare handled correctly!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_compare_too_many_addresses() -> Result<()> {
    let local_client = get_client().await?;

    // Create 31 addresses (exceeds limit of 30)
    let addr = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let addresses: Vec<&str> = (0..31).map(|_| addr).collect();
    let addresses_param = addresses.join(",");

    let endpoint = format!("/accounts/compare?addresses={}", addresses_param);

    println!(
        "\n{} Testing compare endpoint with too many addresses (31)",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Should return 400 Bad Request
    assert_eq!(
        local_status.as_u16(),
        400,
        "Expected 400 Bad Request for too many addresses, got {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    assert!(
        response_obj.contains_key("error"),
        "Error response should contain 'error' field"
    );

    let error_msg = response_obj.get("error").unwrap().as_str().unwrap();
    println!("  {} Error: {}", "✓".green(), error_msg);

    println!(
        "{} Too many addresses error handled correctly!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_compare_no_addresses() -> Result<()> {
    let local_client = get_client().await?;

    let endpoint = "/accounts/compare?addresses=";

    println!(
        "\n{} Testing compare endpoint with empty addresses",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // Should return 400 Bad Request
    assert_eq!(
        local_status.as_u16(),
        400,
        "Expected 400 Bad Request for empty addresses, got {}",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");
    assert!(
        response_obj.contains_key("error"),
        "Error response should contain 'error' field"
    );

    let error_msg = response_obj.get("error").unwrap().as_str().unwrap();
    println!("  {} Error: {}", "✓".green(), error_msg);

    println!(
        "{} Empty addresses error handled correctly!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_compare_response_structure() -> Result<()> {
    let local_client = get_client().await?;

    let addr1 = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let addr2 = "DXgXPAT5zWtPHo6FhVvrDdiaDPgCNGxhJAeVBYLtiwW9hAc";

    let endpoint = format!("/accounts/compare?addresses={},{}", addr1, addr2);

    println!(
        "\n{} Testing compare endpoint response structure",
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

    let response_obj = local_json.as_object().expect("Response is not an object");

    // Validate top-level structure
    assert!(
        response_obj.contains_key("areEqual"),
        "Response missing 'areEqual' field"
    );
    assert!(
        response_obj.get("areEqual").unwrap().is_boolean(),
        "'areEqual' should be boolean"
    );

    assert!(
        response_obj.contains_key("addresses"),
        "Response missing 'addresses' field"
    );
    assert!(
        response_obj.get("addresses").unwrap().is_array(),
        "'addresses' should be array"
    );

    let addresses = response_obj.get("addresses").unwrap().as_array().unwrap();

    for (i, addr) in addresses.iter().enumerate() {
        let addr_obj = addr.as_object().expect("Address should be an object");

        // Required field
        assert!(
            addr_obj.contains_key("ss58Format"),
            "Address {} missing 'ss58Format' field",
            i
        );
        assert!(
            addr_obj.get("ss58Format").unwrap().is_string(),
            "'ss58Format' should be string"
        );

        // For valid addresses, these should be present
        if let Some(ss58_prefix) = addr_obj.get("ss58Prefix") {
            if !ss58_prefix.is_null() {
                assert!(
                    ss58_prefix.is_u64(),
                    "'ss58Prefix' should be a number when present"
                );
            }
        }

        if let Some(network) = addr_obj.get("network") {
            if !network.is_null() {
                assert!(
                    network.is_string(),
                    "'network' should be a string when present"
                );
            }
        }

        if let Some(public_key) = addr_obj.get("publicKey") {
            if !public_key.is_null() {
                assert!(
                    public_key.is_string(),
                    "'publicKey' should be a string when present"
                );
                assert!(
                    public_key.as_str().unwrap().starts_with("0x"),
                    "'publicKey' should be hex-encoded with 0x prefix"
                );
            }
        }
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_compare_addresses_with_spaces() -> Result<()> {
    let local_client = get_client().await?;

    // Addresses with spaces around them (should be trimmed)
    let addr1 = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let addr2 = "DXgXPAT5zWtPHo6FhVvrDdiaDPgCNGxhJAeVBYLtiwW9hAc";

    // Note: URL encoding would handle spaces, but the handler trims them
    let endpoint = format!("/accounts/compare?addresses={}, {}", addr1, addr2);

    println!(
        "\n{} Testing compare endpoint with spaces in addresses",
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

    let response_obj = local_json.as_object().expect("Response is not an object");
    let addresses = response_obj.get("addresses").unwrap().as_array().unwrap();

    assert_eq!(addresses.len(), 2, "Expected 2 addresses after trimming");

    // Both addresses should be valid (spaces trimmed)
    for addr in addresses.iter() {
        let addr_obj = addr.as_object().unwrap();
        let public_key = addr_obj.get("publicKey");
        assert!(
            public_key.is_some() && !public_key.unwrap().is_null(),
            "Address should be valid after trimming spaces"
        );
    }

    println!("  {} Spaces trimmed correctly", "✓".green());

    println!(
        "{} Addresses with spaces handled correctly!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_compare_max_addresses() -> Result<()> {
    let local_client = get_client().await?;

    // Create exactly 30 addresses (the maximum allowed)
    let addr = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let addresses: Vec<&str> = (0..30).map(|_| addr).collect();
    let addresses_param = addresses.join(",");

    let endpoint = format!("/accounts/compare?addresses={}", addresses_param);

    println!(
        "\n{} Testing compare endpoint with maximum addresses (30)",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(
        local_status.is_success(),
        "Local API returned status {} (should accept 30 addresses)",
        local_status
    );

    let response_obj = local_json.as_object().expect("Response is not an object");

    let are_equal = response_obj.get("areEqual").unwrap().as_bool().unwrap();
    assert!(are_equal, "All 30 identical addresses should be equal");

    let addresses = response_obj.get("addresses").unwrap().as_array().unwrap();
    assert_eq!(addresses.len(), 30, "Expected 30 addresses in response");

    println!("  {} areEqual: {}", "✓".green(), are_equal);
    println!("  {} 30 addresses processed successfully", "✓".green());

    println!(
        "{} Maximum addresses (30) handled correctly!",
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
