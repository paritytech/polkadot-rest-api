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
    
    // Update fixtures based on chain argument
    match chain.as_str() {
        "polkadot" => {
            update_polkadot_fixtures(&client, &api_url).await?;
        }
        "kusama" => {
            update_kusama_fixtures(&client, &api_url).await?;
        }
        "all" => {
            update_polkadot_fixtures(&client, &api_url).await?;
            update_kusama_fixtures(&client, &api_url).await?;
        }
        _ => {
            anyhow::bail!("Invalid chain argument: {}. Use 'polkadot', 'kusama', or 'all'", chain);
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
        match client.get(&format!("{}/v1/health", api_url)).send().await {
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

async fn update_polkadot_fixtures(client: &Client, api_url: &str) -> Result<()> {
    println!("Updating Polkadot fixtures...");
    println!("{}", "-".repeat(60));
    
    // Fetch block 1000000
    println!("\nFetching block 1000000 from API...");
    let block_data = fetch_block_from_api(client, api_url, "1000000").await?;
    save_fixture("polkadot", "blocks_1000000.json", &block_data)?;
    
    println!("\n✓ Polkadot fixtures updated");
    
    Ok(())
}

async fn update_kusama_fixtures(client: &Client, api_url: &str) -> Result<()> {
    println!("\nUpdating Kusama fixtures...");
    println!("{}", "-".repeat(60));
    
    // Fetch block 5000000
    println!("\nFetching block 5000000 from API...");
    let block_data = fetch_block_from_api(client, api_url, "5000000").await?;
    save_fixture("kusama", "blocks_5000000.json", &block_data)?;
    
    println!("\n✓ Kusama fixtures updated");
    
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
    
    // Check for required fields
    let required_fields = ["number", "hash", "parentHash", "stateRoot", "extrinsicsRoot"];
    for field in &required_fields {
        if !block_data.get(field).is_some() {
            anyhow::bail!("API response missing required field: {}", field);
        }
    }
    
    println!("  ✓ Block {}", block_data["number"]);
    
    Ok(block_data)
}

fn save_fixture(chain: &str, filename: &str, data: &Value) -> Result<()> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixtures_dir = PathBuf::from(manifest_dir).join("tests").join("fixtures").join(chain);
    std::fs::create_dir_all(&fixtures_dir)
        .context(format!("Failed to create fixtures directory: {:?}", fixtures_dir))?;
    
    let fixture_path = fixtures_dir.join(filename);
    
    println!("  Saving to {:?}...", fixture_path);
    let json_string = serde_json::to_string_pretty(data)?;
    std::fs::write(&fixture_path, &json_string)
        .context(format!("Failed to write fixture: {:?}", fixture_path))?;
    
    println!("  ✓ Saved {} bytes", json_string.len());
    
    Ok(())
}
