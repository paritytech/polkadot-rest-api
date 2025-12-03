//! Script to update test fixtures with real blockchain data
//! This script calls the REST API to fetch block data, ensuring fixtures match exactly what the API returns

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
        // If no argument, update both chains
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
        "all" => {
            update_chain_fixtures(&client, &api_url, "polkadot", &config).await?;
            update_chain_fixtures(&client, &api_url, "kusama", &config).await?;
            update_chain_fixtures(&client, &api_url, "asset-hub-polkadot", &config).await?;
            update_chain_fixtures(&client, &api_url, "asset-hub-kusama", &config).await?;
        }
        _ => {
            anyhow::bail!(
                "Invalid chain argument: {}. Use 'polkadot', 'kusama', 'asset-hub-polkadot', 'asset-hub-kusama', or 'all'",
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
        if let Some(block_height) = test_case.block_height {
            let endpoint = test_case.endpoint.to_string();

            println!(
                "\nFetching {} at block {} from API...",
                endpoint, block_height
            );

            let data = if endpoint.contains("/blocks/") {
                fetch_block_from_api(client, api_url, &block_height.to_string()).await?
            } else if endpoint.contains("/runtime/") {
                fetch_runtime_from_api(client, api_url, &endpoint, &block_height.to_string())
                    .await?
            } else {
                println!("  ⚠ Skipping unsupported endpoint: {}", endpoint);
                continue;
            };

            let filename = test_case
                .fixture_path
                .file_name()
                .and_then(|n| n.to_str())
                .context("Invalid fixture path")?;

            save_fixture(chain_name, filename, &data)?;
        }
    }

    println!("\n✓ {} fixtures updated", chain_name);

    Ok(())
}

async fn fetch_block_from_api(client: &Client, api_url: &str, block_id: &str) -> Result<Value> {
    let url = format!("{}/v1/blocks/{}", api_url.trim_end_matches('/'), block_id);

    println!("  Requesting: {}", url);

    let response = client
        .get(&url)
        .send()
        .await
        .context(format!("Failed to send request to {}", url))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("API returned status {}: {}", status, text);
    }

    let block_data: Value = response
        .json()
        .await
        .context(format!("Failed to parse JSON response from {}", url))?;

    // Verify the response has the expected structure
    if !block_data.is_object() {
        anyhow::bail!("API response is not a JSON object");
    }

    // Check for required fields matching BlockResponse struct
    // BlockResponse fields: number, hash, parent_hash, state_root, extrinsics_root,
    //                       author_id (optional), logs, extrinsics
    let required_fields = [
        "number",
        "hash",
        "parentHash",
        "stateRoot",
        "extrinsicsRoot",
        "logs",
        "extrinsics",
    ];
    for field in &required_fields {
        if block_data.get(field).is_none() {
            anyhow::bail!("API response missing required field: {}", field);
        }
    }

    // Verify that logs and extrinsics are arrays
    if !block_data["logs"].is_array() {
        anyhow::bail!("API response field 'logs' is not an array");
    }
    if !block_data["extrinsics"].is_array() {
        anyhow::bail!("API response field 'extrinsics' is not an array");
    }

    println!("  ✓ Block {}", block_data["number"]);

    Ok(block_data)
}

async fn fetch_runtime_from_api(
    client: &Client,
    api_url: &str,
    endpoint: &str,
    block_height: &str,
) -> Result<Value> {
    let url = format!(
        "{}{}?at={}",
        api_url.trim_end_matches('/'),
        endpoint,
        block_height
    );

    println!("  Requesting: {}", url);

    let response = client
        .get(&url)
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

    if data.get("at").is_none() {
        anyhow::bail!("API response missing required field: at");
    }

    if endpoint.contains("/metadata/versions") {
        if data.get("versions").is_none() {
            anyhow::bail!("API response missing required field: versions");
        }
    } else if endpoint.contains("/metadata") {
        if data.get("metadata").is_none() {
            anyhow::bail!("API response missing required field: metadata");
        }
        if data.get("magicNumber").is_none() {
            anyhow::bail!("API response missing required field: magicNumber");
        }
    } else if endpoint.contains("/code") && data.get("code").is_none() {
        anyhow::bail!("API response missing required field: code");
    }

    println!("  ✓ Response received (block height: {})", block_height);

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
