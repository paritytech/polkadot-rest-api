//! Script to update test fixtures with real blockchain data
//! This script uses JSON-RPC calls directly to fetch block data

use anyhow::{Context, Result};
use jsonrpsee::core::client::ClientT;
use jsonrpsee::rpc_params;
use jsonrpsee::ws_client::WsClientBuilder;
use serde_json::json;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    println!("{}", "=".repeat(60));
    println!("Fixture Updater");
    println!("{}", "=".repeat(60));
    println!();
    
    // Update Polkadot fixtures
    update_polkadot_fixtures().await?;
    
    // Update Kusama fixtures
    update_kusama_fixtures().await?;
    
    println!();
    println!("{}", "=".repeat(60));
    println!("✓ Fixture update completed");
    println!("{}", "=".repeat(60));
    
    Ok(())
}

async fn update_polkadot_fixtures() -> Result<()> {
    println!("Updating Polkadot fixtures...");
    println!("{}", "-".repeat(60));
    
    let rpc_url = "wss://rpc.polkadot.io";
    println!("Connecting to Polkadot: {}", rpc_url);
    
    let client = WsClientBuilder::default()
        .build(rpc_url)
        .await
        .context("Failed to connect to Polkadot")?;
    
    // Fetch block 1000000
    println!("\nFetching block 1000000...");
    let block_data = fetch_block_data(&client, 1000000).await?;
    save_fixture("polkadot", "blocks_1000000.json", &block_data)?;
    
    // Fetch head block
    println!("\nFetching head block...");
    let head_data = fetch_head_block_data(&client).await?;
    save_fixture("polkadot", "blocks_head.json", &head_data)?;
    
    println!("\n✓ Polkadot fixtures updated");
    
    Ok(())
}

async fn update_kusama_fixtures() -> Result<()> {
    println!("\nUpdating Kusama fixtures...");
    println!("{}", "-".repeat(60));
    
    let rpc_url = "wss://kusama-rpc.polkadot.io";
    println!("Connecting to Kusama: {}", rpc_url);
    
    let client = WsClientBuilder::default()
        .build(rpc_url)
        .await
        .context("Failed to connect to Kusama")?;
    
    // Fetch block 5000000
    println!("\nFetching block 5000000...");
    let block_data = fetch_block_data(&client, 5000000).await?;
    save_fixture("kusama", "blocks_5000000.json", &block_data)?;
    
    println!("\n✓ Kusama fixtures updated");
    
    Ok(())
}

async fn fetch_block_data(client: &jsonrpsee::ws_client::WsClient, block_number: u64) -> Result<serde_json::Value> {
    // Get block hash
    let block_hash: Option<String> = client
        .request("chain_getBlockHash", rpc_params![block_number])
        .await
        .context(format!("Failed to get block hash for block {}", block_number))?;
    
    let block_hash = block_hash.ok_or_else(|| anyhow::anyhow!("Block {} not found", block_number))?;
    
    // Get block
    let block: serde_json::Value = client
        .request("chain_getBlock", rpc_params![&block_hash])
        .await
        .context(format!("Failed to get block {}", block_number))?;
    
    // Get runtime version
    let runtime_version: serde_json::Value = client
        .request("state_getRuntimeVersion", rpc_params![&block_hash])
        .await
        .context("Failed to get runtime version")?;
    
    let spec_name = runtime_version
        .get("other")
        .and_then(|o| o.get("specName"))
        .and_then(|s| s.as_str())
        .unwrap_or("unknown")
        .to_string();
    
    let spec_version = runtime_version
        .get("specVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    
    // Extract block data from JSON
    let block_obj = block
        .get("block")
        .ok_or_else(|| anyhow::anyhow!("Block data not found"))?;
    
    let header = block_obj
        .get("header")
        .ok_or_else(|| anyhow::anyhow!("Block header not found"))?;
    
    // Use the block hash we already fetched (header doesn't contain hash field)
    let hash = &block_hash;
    
    let parent_hash = header
        .get("parentHash")
        .and_then(|h| h.as_str())
        .unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000");
    
    let state_root = header
        .get("stateRoot")
        .and_then(|h| h.as_str())
        .unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000");
    
    let extrinsics_root = header
        .get("extrinsicsRoot")
        .and_then(|h| h.as_str())
        .unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000");
    
    // Get number and convert from hex if needed
    let number = header
        .get("number")
        .and_then(|n| {
            if let Some(s) = n.as_str() {
                // Try to parse hex string
                if s.starts_with("0x") {
                    u64::from_str_radix(&s[2..], 16)
                        .ok()
                        .map(|n| n.to_string())
                        .or_else(|| Some(s.to_string()))
                } else {
                    Some(s.to_string())
                }
            } else {
                n.as_u64().map(|n| n.to_string())
            }
        })
        .unwrap_or_else(|| block_number.to_string());
    
    let extrinsics = block_obj
        .get("extrinsics")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_else(|| Vec::new());
    
    let block_data = json!({
        "number": number,
        "hash": hash,
        "parentHash": parent_hash,
        "stateRoot": state_root,
        "extrinsicsRoot": extrinsics_root,
        "extrinsics": serde_json::Value::Array(extrinsics),
        "spec": {
            "specName": spec_name,
            "specVersion": spec_version
        }
    });
    
    Ok(block_data)
}

async fn fetch_head_block_data(client: &jsonrpsee::ws_client::WsClient) -> Result<serde_json::Value> {
    // Get latest block (no hash parameter = latest)
    let block: serde_json::Value = client
        .request("chain_getBlock", rpc_params![None::<String>])
        .await
        .context("Failed to get latest block")?;
    
    // Get latest block hash
    let block_hash: Option<String> = client
        .request("chain_getBlockHash", rpc_params![None::<u64>])
        .await
        .context("Failed to get block hash")?;
    
    let block_hash = block_hash.ok_or_else(|| anyhow::anyhow!("Failed to get block hash"))?;
    
    // Get runtime version
    let runtime_version: serde_json::Value = client
        .request("state_getRuntimeVersion", rpc_params![&block_hash])
        .await
        .context("Failed to get runtime version")?;
    
    let spec_name = runtime_version
        .get("other")
        .and_then(|o| o.get("specName"))
        .and_then(|s| s.as_str())
        .unwrap_or("unknown")
        .to_string();
    
    let spec_version = runtime_version
        .get("specVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    
    // Extract block data from JSON
    let block_obj = block
        .get("block")
        .ok_or_else(|| anyhow::anyhow!("Block data not found"))?;
    
    let header = block_obj
        .get("header")
        .ok_or_else(|| anyhow::anyhow!("Block header not found"))?;
    
    // Use the block hash we already fetched (header doesn't contain hash field)
    let hash = &block_hash;
    
    let parent_hash = header
        .get("parentHash")
        .and_then(|h| h.as_str())
        .unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000");
    
    let state_root = header
        .get("stateRoot")
        .and_then(|h| h.as_str())
        .unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000");
    
    let extrinsics_root = header
        .get("extrinsicsRoot")
        .and_then(|h| h.as_str())
        .unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000");
    
    // Get number and convert from hex if needed
    let number = header
        .get("number")
        .and_then(|n| {
            if let Some(s) = n.as_str() {
                // Try to parse hex string
                if s.starts_with("0x") {
                    u64::from_str_radix(&s[2..], 16)
                        .ok()
                        .map(|n| n.to_string())
                        .or_else(|| Some(s.to_string()))
                } else {
                    Some(s.to_string())
                }
            } else {
                n.as_u64().map(|n| n.to_string())
            }
        })
        .unwrap_or_else(|| "0".to_string());
    
    let extrinsics = block_obj
        .get("extrinsics")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_else(|| Vec::new());
    
    let block_data = json!({
        "number": number,
        "hash": hash,
        "parentHash": parent_hash,
        "stateRoot": state_root,
        "extrinsicsRoot": extrinsics_root,
        "extrinsics": serde_json::Value::Array(extrinsics),
        "spec": {
            "specName": spec_name,
            "specVersion": spec_version
        }
    });
    
    Ok(block_data)
}

fn save_fixture(chain: &str, filename: &str, data: &serde_json::Value) -> Result<()> {
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
