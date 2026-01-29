//! Script to update test fixtures with real blockchain data
//!
//! This script reads test cases from test_config.json and uses the `endpoint` field
//! to determine which API endpoint to call. It supports any endpoint type (blocks,
//! coretime, runtime, etc.) by using the endpoint path and query_params from each test case.
//!
//! Usage:
//!   cargo run --bin update_fixtures [chain]
//!
//! Examples:
//!   cargo run --bin update_fixtures polkadot
//!   cargo run --bin update_fixtures coretime-kusama
//!   cargo run --bin update_fixtures all

use anyhow::{Context, Result};
use integration_tests::constants::API_READY_TIMEOUT_SECONDS;
use reqwest::Client;
use serde_json::Value;
use std::env;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Parse command line arguments to determine which chain to update
    let args: Vec<String> = env::args().collect();
    let chain = if args.len() > 1 {
        args[1].to_lowercase()
    } else {
        // If no argument, update all chains
        "all".to_string()
    };

    println!("{}", "=".repeat(60));
    println!("Fixture Updater");
    println!("{}", "=".repeat(60));
    println!();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    println!("Using API URL: {}", api_url);
    println!("Target chain: {}", chain);
    println!();

    // Check if API is available
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("Failed to create HTTP client")?;

    if !wait_for_api(&client, &api_url).await? {
        anyhow::bail!(
            "API is not available at {}. Please start the server first.\n\
            Example:\n  export SAS_SUBSTRATE_URL=wss://rpc.polkadot.io\n  cargo run --release --bin polkadot-rest-api",
            api_url
        );
    }

    // Load test config to get all historical test cases
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let config_path = PathBuf::from(manifest_dir)
        .join("tests")
        .join("config")
        .join("test_config.json");
    let config = integration_tests::config::TestConfig::from_file(&config_path)
        .context("Failed to load test config")?;

    // Update fixtures based on chain argument
    match chain.as_str() {
        "polkadot" => {
            update_chain_fixtures(&client, &api_url, "polkadot", &config).await?;
        }
        "kusama" => {
            update_chain_fixtures(&client, &api_url, "kusama", &config).await?;
        }
        "asset-hub-polkadot" => {
            update_chain_fixtures(&client, &api_url, "asset-hub-polkadot", &config).await?;
        }
        "asset-hub-kusama" => {
            update_chain_fixtures(&client, &api_url, "asset-hub-kusama", &config).await?;
        }
        "coretime-polkadot" => {
            update_chain_fixtures(&client, &api_url, "coretime-polkadot", &config).await?;
        }
        "coretime-kusama" => {
            update_chain_fixtures(&client, &api_url, "coretime-kusama", &config).await?;
        }
        "all" => {
            update_chain_fixtures(&client, &api_url, "polkadot", &config).await?;
            update_chain_fixtures(&client, &api_url, "kusama", &config).await?;
            update_chain_fixtures(&client, &api_url, "asset-hub-polkadot", &config).await?;
            update_chain_fixtures(&client, &api_url, "asset-hub-kusama", &config).await?;
            update_chain_fixtures(&client, &api_url, "coretime-polkadot", &config).await?;
            update_chain_fixtures(&client, &api_url, "coretime-kusama", &config).await?;
        }
        _ => {
            anyhow::bail!(
                "Invalid chain argument: {}. Use 'polkadot', 'kusama', 'asset-hub-polkadot', 'asset-hub-kusama', 'coretime-polkadot', 'coretime-kusama', or 'all'",
                chain
            );
        }
    }

    println!();
    println!("{}", "=".repeat(60));
    println!("✓ Fixture update completed");
    println!("{}", "=".repeat(60));

    Ok(())
}

async fn wait_for_api(client: &Client, api_url: &str) -> Result<bool> {
    for i in 0..API_READY_TIMEOUT_SECONDS {
        match client.get(format!("{}/v1/health", api_url)).send().await {
            Ok(response) if response.status().is_success() => {
                println!("✓ API is ready");
                return Ok(true);
            }
            _ => {
                if i < API_READY_TIMEOUT_SECONDS - 1 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
    Ok(false)
}

async fn update_chain_fixtures(
    client: &Client,
    api_url: &str,
    chain_name: &str,
    config: &integration_tests::config::TestConfig,
) -> Result<()> {
    println!("\nUpdating {} fixtures...", chain_name);
    println!("{}", "-".repeat(60));

    let test_cases = config.get_historical_tests(chain_name);

    if test_cases.is_empty() {
        println!("  No historical tests found for {}", chain_name);
        return Ok(());
    }

    for test_case in &test_cases {
        // Build endpoint URL, substituting {blockId} if block_height is provided
        let endpoint = if let Some(block_height) = test_case.block_height {
            test_case
                .endpoint
                .replace("{blockId}", &block_height.to_string())
        } else {
            test_case.endpoint.clone()
        };

        // Build query string from query_params
        let query_string: String = test_case
            .query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        let url = if query_string.is_empty() {
            format!("{}{}", api_url.trim_end_matches('/'), endpoint)
        } else {
            format!(
                "{}{}?{}",
                api_url.trim_end_matches('/'),
                endpoint,
                query_string
            )
        };

        println!("\nFetching: {}", url);

        let data = fetch_endpoint_from_api(client, &url).await?;

        // Extract filename from fixture_path
        let filename = test_case
            .fixture_path
            .file_name()
            .and_then(|n| n.to_str())
            .context("Invalid fixture path")?;

        save_fixture(chain_name, filename, &data)?;
    }

    println!("\n✓ {} fixtures updated", chain_name);

    Ok(())
}

/// Generic function to fetch data from any API endpoint
async fn fetch_endpoint_from_api(client: &Client, url: &str) -> Result<Value> {
    let response = client
        .get(url)
        .send()
        .await
        .context(format!("Failed to send request to {}", url))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("API returned status {}: {}", status, text);
    }

    let data: Value = response
        .json()
        .await
        .context(format!("Failed to parse JSON response from {}", url))?;

    if !data.is_object() {
        anyhow::bail!("API response is not a JSON object");
    }

    println!("  ✓ Response received");

    Ok(data)
}

fn save_fixture(chain: &str, filename: &str, data: &Value) -> Result<()> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixtures_dir = PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join(chain);
    std::fs::create_dir_all(&fixtures_dir).context(format!(
        "Failed to create fixtures directory: {:?}",
        fixtures_dir
    ))?;

    let fixture_path = fixtures_dir.join(filename);

    println!("  Saving to {:?}...", fixture_path);
    let json_string = serde_json::to_string_pretty(data)?;
    std::fs::write(&fixture_path, &json_string)
        .context(format!("Failed to write fixture: {:?}", fixture_path))?;

    println!("  ✓ Saved {} bytes", json_string.len());

    Ok(())
}
