// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for /accounts/{accountId}/convert endpoint

use super::{Colorize, get_client, test_accounts};
use anyhow::{Context, Result};

#[tokio::test]
async fn test_convert_basic() -> Result<()> {
    let local_client = get_client().await?;

    // Use a known account ID (32 bytes hex)
    let account_id = test_accounts::ALICE_HEX; // Alice
    let endpoint = format!("/accounts/{}/convert", account_id);

    println!(
        "\n{} Testing convert endpoint for account {}",
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
    let response_obj = local_json.as_object().expect("Response is not an object");

    // Required fields
    assert!(
        response_obj.contains_key("ss58Prefix"),
        "Response missing 'ss58Prefix' field"
    );
    assert!(
        response_obj.contains_key("network"),
        "Response missing 'network' field"
    );
    assert!(
        response_obj.contains_key("address"),
        "Response missing 'address' field"
    );
    assert!(
        response_obj.contains_key("accountId"),
        "Response missing 'accountId' field"
    );
    assert!(
        response_obj.contains_key("scheme"),
        "Response missing 'scheme' field"
    );
    assert!(
        response_obj.contains_key("publicKey"),
        "Response missing 'publicKey' field"
    );

    // Verify default values
    let prefix = response_obj.get("ss58Prefix").unwrap().as_u64().unwrap();
    assert_eq!(prefix, 42, "Default prefix should be 42 (substrate)");

    let network = response_obj.get("network").unwrap().as_str().unwrap();
    assert_eq!(network, "substrate", "Default network should be substrate");

    let scheme = response_obj.get("scheme").unwrap().as_str().unwrap();
    assert_eq!(scheme, "sr25519", "Default scheme should be sr25519");

    // Print some info
    let address = response_obj.get("address").unwrap().as_str().unwrap();
    println!(
        "  {} Address: {}, Network: {}, Prefix: {}",
        "ℹ".blue(),
        address,
        network,
        prefix
    );

    println!("{} Response structure validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_convert_with_polkadot_prefix() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ALICE_HEX; // Alice
    let endpoint = format!("/accounts/{}/convert?prefix=0", account_id);

    println!(
        "\n{} Testing convert with Polkadot prefix (0)",
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

    let prefix = response_obj.get("ss58Prefix").unwrap().as_u64().unwrap();
    assert_eq!(prefix, 0, "Prefix should be 0");

    let network = response_obj.get("network").unwrap().as_str().unwrap();
    assert_eq!(network, "polkadot", "Network should be polkadot");

    let address = response_obj.get("address").unwrap().as_str().unwrap();
    assert!(
        address.starts_with('1'),
        "Polkadot address should start with 1"
    );

    println!(
        "  {} Address: {}, Network: {}",
        "ℹ".blue(),
        address,
        network
    );

    println!("{} Polkadot prefix validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_convert_with_kusama_prefix() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = test_accounts::ALICE_HEX; // Alice
    let endpoint = format!("/accounts/{}/convert?prefix=2", account_id);

    println!(
        "\n{} Testing convert with Kusama prefix (2)",
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

    let prefix = response_obj.get("ss58Prefix").unwrap().as_u64().unwrap();
    assert_eq!(prefix, 2, "Prefix should be 2");

    let network = response_obj.get("network").unwrap().as_str().unwrap();
    assert_eq!(network, "kusama", "Network should be kusama");

    println!(
        "  {} Address: {}, Network: {}",
        "ℹ".blue(),
        response_obj.get("address").unwrap().as_str().unwrap(),
        network
    );

    println!("{} Kusama prefix validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_convert_with_different_schemes() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";

    println!(
        "\n{} Testing convert with different schemes",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    for scheme in ["sr25519", "ed25519", "ecdsa"] {
        let endpoint = format!("/accounts/{}/convert?scheme={}", account_id, scheme);

        let (local_status, local_json) = local_client
            .get_json(&format!("/v1{}", endpoint))
            .await
            .context("Failed to fetch from local API")?;

        assert!(
            local_status.is_success(),
            "Local API returned status {} for scheme {}",
            local_status,
            scheme
        );

        let response_obj = local_json.as_object().unwrap();
        let returned_scheme = response_obj.get("scheme").unwrap().as_str().unwrap();
        assert_eq!(returned_scheme, scheme, "Scheme mismatch");

        println!(
            "  {} Scheme {}: {}",
            "✓".green(),
            scheme,
            response_obj.get("address").unwrap().as_str().unwrap()
        );
    }

    println!("{} All schemes validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_convert_invalid_hex() -> Result<()> {
    let local_client = get_client().await?;

    let invalid_hex = "not-valid-hex";
    let endpoint = format!("/accounts/{}/convert", invalid_hex);

    println!(
        "\n{} Testing convert with invalid hex",
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

    println!("{} Error response validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_convert_invalid_scheme() -> Result<()> {
    let local_client = get_client().await?;

    let account_id = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = format!("/accounts/{}/convert?scheme=invalid", account_id);

    println!(
        "\n{} Testing convert with invalid scheme",
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
        error_msg.contains("scheme"),
        "Error message should mention scheme: {}",
        error_msg
    );

    println!("{} Error response validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_convert_wrong_length_hex() -> Result<()> {
    let local_client = get_client().await?;

    // Only 16 bytes instead of 32
    let short_hex = "0xd43593c715fdd31c61141abd04a99fd6";
    let endpoint = format!("/accounts/{}/convert", short_hex);

    println!(
        "\n{} Testing convert with wrong length hex",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert_eq!(
        local_status.as_u16(),
        500,
        "Expected 500 for wrong length, got {}",
        local_status
    );

    println!("{} Received expected 500 for wrong length", "✓".green());

    let error_obj = local_json.as_object().unwrap();
    assert!(
        error_obj.contains_key("error"),
        "Error response missing 'error' field"
    );

    let error_msg = error_obj.get("error").unwrap().as_str().unwrap();
    assert!(
        error_msg.contains("32 bytes"),
        "Error message should mention 32 bytes: {}",
        error_msg
    );

    println!("{} Error response validated!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_convert_public_key_ecdsa() -> Result<()> {
    let local_client = get_client().await?;

    // 33-byte compressed ECDSA public key
    let public_key = "0x02509540919faacf9ab52146c9aa40db68172d83777250b28e4679176e49ccdd9f";
    let endpoint = format!(
        "/accounts/{}/convert?scheme=ecdsa&publicKey=true",
        public_key
    );

    println!(
        "\n{} Testing convert with ECDSA public key",
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

    let scheme = response_obj.get("scheme").unwrap().as_str().unwrap();
    assert_eq!(scheme, "ecdsa", "Scheme should be ecdsa");

    let public_key_flag = response_obj.get("publicKey").unwrap().as_bool().unwrap();
    assert!(public_key_flag, "publicKey should be true");

    println!(
        "  {} Address: {}",
        "ℹ".blue(),
        response_obj.get("address").unwrap().as_str().unwrap()
    );

    println!(
        "{} ECDSA public key conversion validated!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_convert_without_0x_prefix() -> Result<()> {
    let local_client = get_client().await?;

    // Without 0x prefix
    let account_id = "d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
    let endpoint = format!("/accounts/{}/convert", account_id);

    println!(
        "\n{} Testing convert without 0x prefix",
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

    // Verify the accountId in response has 0x prefix
    let returned_account_id = response_obj.get("accountId").unwrap().as_str().unwrap();
    assert!(
        returned_account_id.starts_with("0x"),
        "Returned accountId should have 0x prefix"
    );

    println!(
        "  {} Input: {}, Output accountId: {}",
        "ℹ".blue(),
        account_id,
        returned_account_id
    );

    println!(
        "{} Conversion without 0x prefix validated!",
        "✓".green().bold()
    );
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}
