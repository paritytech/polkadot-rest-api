//! Integration tests for /accounts/{accountId}/validate endpoint
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
async fn test_validate_polkadot_address() -> Result<()> {
    let local_client = get_client().await?;

    // A valid Polkadot address (prefix 0)
    let polkadot_addr = "1xN1Q5eKQmS5AzASdjt6R6sHF76611vKR4PFpFjy1kXau4m";
    let endpoint = format!("/accounts/{}/validate", polkadot_addr);

    println!(
        "\n{} Testing validate endpoint for Polkadot address",
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
        response_obj.contains_key("isValid"),
        "Response missing 'isValid' field"
    );

    let is_valid = response_obj.get("isValid").unwrap().as_bool().unwrap();
    assert!(is_valid, "Expected valid address");

    // Should have prefix 0 for Polkadot
    let ss58_prefix = response_obj.get("ss58Prefix").unwrap().as_str().unwrap();
    assert_eq!(ss58_prefix, "0", "Expected Polkadot prefix (0)");

    // Should identify as polkadot network
    let network = response_obj.get("network").unwrap().as_str().unwrap();
    assert_eq!(network, "polkadot", "Expected polkadot network");

    // Should have account ID
    assert!(
        response_obj.contains_key("accountId"),
        "Response missing 'accountId' field"
    );
    let account_id = response_obj.get("accountId").unwrap().as_str().unwrap();
    assert!(account_id.starts_with("0x"), "Account ID should be hex");

    println!(
        "  {} isValid: {}, ss58Prefix: {}, network: {}",
        "✓".green(),
        is_valid,
        ss58_prefix,
        network
    );
    println!("  {} accountId: {}", "✓".green(), account_id);

    println!("{} Polkadot address validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_validate_kusama_address() -> Result<()> {
    let local_client = get_client().await?;

    // A valid Kusama address (prefix 2)
    let kusama_addr = "DXgXPAT5zWtPHo6FhVvrDdiaDPgCNGxhJAeVBYLtiwW9hAc";
    let endpoint = format!("/accounts/{}/validate", kusama_addr);

    println!(
        "\n{} Testing validate endpoint for Kusama address",
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

    let is_valid = response_obj.get("isValid").unwrap().as_bool().unwrap();
    assert!(is_valid, "Expected valid address");

    let ss58_prefix = response_obj.get("ss58Prefix").unwrap().as_str().unwrap();
    assert_eq!(ss58_prefix, "2", "Expected Kusama prefix (2)");

    let network = response_obj.get("network").unwrap().as_str().unwrap();
    assert_eq!(network, "kusama", "Expected kusama network");

    println!(
        "  {} isValid: {}, ss58Prefix: {}, network: {}",
        "✓".green(),
        is_valid,
        ss58_prefix,
        network
    );

    println!("{} Kusama address validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_validate_substrate_address() -> Result<()> {
    let local_client = get_client().await?;

    // A valid Substrate generic address (prefix 42)
    let substrate_addr = "5EnxxUmEbw8DkENKiYuZ1DwQuMoB2UWEQJZZXrTsxoz7SpgG";
    let endpoint = format!("/accounts/{}/validate", substrate_addr);

    println!(
        "\n{} Testing validate endpoint for Substrate address",
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

    let is_valid = response_obj.get("isValid").unwrap().as_bool().unwrap();
    assert!(is_valid, "Expected valid address");

    let ss58_prefix = response_obj.get("ss58Prefix").unwrap().as_str().unwrap();
    assert_eq!(ss58_prefix, "42", "Expected Substrate prefix (42)");

    let network = response_obj.get("network").unwrap().as_str().unwrap();
    assert_eq!(network, "substrate", "Expected substrate network");

    println!(
        "  {} isValid: {}, ss58Prefix: {}, network: {}",
        "✓".green(),
        is_valid,
        ss58_prefix,
        network
    );

    println!("{} Substrate address validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_validate_hex_polkadot_address() -> Result<()> {
    let local_client = get_client().await?;

    // A valid hex-encoded Polkadot address
    // Format: prefix (1 byte) + account id (32 bytes) + checksum (2 bytes)
    let polkadot_hex = "0x002a39366f6620a6c2e2fed5990a3d419e6a19dd127fc7a50b515cf17e2dc5cc592312";
    let endpoint = format!("/accounts/{}/validate", polkadot_hex);

    println!(
        "\n{} Testing validate endpoint for hex-encoded Polkadot address",
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

    let is_valid = response_obj.get("isValid").unwrap().as_bool().unwrap();
    assert!(is_valid, "Expected valid address");

    let ss58_prefix = response_obj.get("ss58Prefix").unwrap().as_str().unwrap();
    assert_eq!(ss58_prefix, "0", "Expected Polkadot prefix (0)");

    let network = response_obj.get("network").unwrap().as_str().unwrap();
    assert_eq!(network, "polkadot", "Expected polkadot network");

    let account_id = response_obj.get("accountId").unwrap().as_str().unwrap();
    assert_eq!(
        account_id.to_lowercase(),
        "0x2a39366f6620a6c2e2fed5990a3d419e6a19dd127fc7a50b515cf17e2dc5cc59",
        "Account ID mismatch"
    );

    println!(
        "  {} isValid: {}, ss58Prefix: {}, network: {}",
        "✓".green(),
        is_valid,
        ss58_prefix,
        network
    );
    println!("  {} accountId: {}", "✓".green(), account_id);

    println!("{} Hex Polkadot address validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_validate_invalid_address() -> Result<()> {
    let local_client = get_client().await?;

    // An invalid address (random string)
    let invalid_addr = "invalid-address-123";
    let endpoint = format!("/accounts/{}/validate", invalid_addr);

    println!(
        "\n{} Testing validate endpoint for invalid address",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    // The endpoint should still return 200 OK with isValid: false
    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    let response_obj = local_json.as_object().unwrap();

    let is_valid = response_obj.get("isValid").unwrap().as_bool().unwrap();
    assert!(!is_valid, "Expected invalid address");

    // Other fields should be null/missing for invalid address
    let ss58_prefix = response_obj.get("ss58Prefix");
    let network = response_obj.get("network");
    let account_id = response_obj.get("accountId");

    // These should either be null or not present
    assert!(
        ss58_prefix.is_none() || ss58_prefix.unwrap().is_null(),
        "ss58Prefix should be null for invalid address"
    );
    assert!(
        network.is_none() || network.unwrap().is_null(),
        "network should be null for invalid address"
    );
    assert!(
        account_id.is_none() || account_id.unwrap().is_null(),
        "accountId should be null for invalid address"
    );

    println!("  {} isValid: {}", "✓".green(), is_valid);

    println!("{} Invalid address handled correctly!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_validate_invalid_hex_address() -> Result<()> {
    let local_client = get_client().await?;

    // An invalid hex address (just a raw account ID without prefix/checksum)
    let invalid_hex = "0x2a39366f6620a6c2e2fed5990a3d419e6a19dd127fc7a50b515cf17e2dc5cc59";
    let endpoint = format!("/accounts/{}/validate", invalid_hex);

    println!(
        "\n{} Testing validate endpoint for invalid hex address",
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

    let is_valid = response_obj.get("isValid").unwrap().as_bool().unwrap();
    assert!(!is_valid, "Expected invalid address (missing prefix and checksum)");

    println!("  {} isValid: {}", "✓".green(), is_valid);

    println!(
        "{} Invalid hex address (no prefix/checksum) handled correctly!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_validate_kulupu_address() -> Result<()> {
    let local_client = get_client().await?;

    // A valid Kulupu address (prefix 16)
    let kulupu_addr = "2cYv9Gk6U4m4a7Taw9pG8qMfd1Pnxw6FLTvV6kYZNhGL6M9y";
    let endpoint = format!("/accounts/{}/validate", kulupu_addr);

    println!(
        "\n{} Testing validate endpoint for Kulupu address",
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

    let is_valid = response_obj.get("isValid").unwrap().as_bool().unwrap();
    assert!(is_valid, "Expected valid address");

    let ss58_prefix = response_obj.get("ss58Prefix").unwrap().as_str().unwrap();
    assert_eq!(ss58_prefix, "16", "Expected Kulupu prefix (16)");

    let network = response_obj.get("network").unwrap().as_str().unwrap();
    assert_eq!(network, "kulupu", "Expected kulupu network");

    println!(
        "  {} isValid: {}, ss58Prefix: {}, network: {}",
        "✓".green(),
        is_valid,
        ss58_prefix,
        network
    );

    println!("{} Kulupu address validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_validate_response_structure() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known valid address
    let address = "5EnxxUmEbw8DkENKiYuZ1DwQuMoB2UWEQJZZXrTsxoz7SpgG";
    let endpoint = format!("/accounts/{}/validate", address);

    println!(
        "\n{} Testing validate endpoint response structure",
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

    // Required field
    assert!(
        response_obj.contains_key("isValid"),
        "Response missing 'isValid' field"
    );

    let is_valid = response_obj.get("isValid").unwrap().as_bool().unwrap();

    if is_valid {
        // For valid addresses, these fields should be present
        assert!(
            response_obj.contains_key("ss58Prefix"),
            "Valid response missing 'ss58Prefix' field"
        );
        assert!(
            response_obj.contains_key("accountId"),
            "Valid response missing 'accountId' field"
        );

        // network may or may not be present depending on whether prefix is known
        if let Some(network) = response_obj.get("network") {
            assert!(
                network.is_string() || network.is_null(),
                "network should be string or null"
            );
        }

        // Validate field types
        let ss58_prefix = response_obj.get("ss58Prefix").unwrap();
        assert!(
            ss58_prefix.is_string(),
            "ss58Prefix should be a string"
        );

        let account_id = response_obj.get("accountId").unwrap();
        assert!(
            account_id.is_string(),
            "accountId should be a string"
        );
        assert!(
            account_id.as_str().unwrap().starts_with("0x"),
            "accountId should be hex-encoded with 0x prefix"
        );
    }

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
