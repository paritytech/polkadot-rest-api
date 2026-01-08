// Integration tests for relay chain connection and config validation
use config::ChainConfigs;
use jsonrpsee::{core::client::ClientT, rpc_params, ws_client::WsClientBuilder};
use serde_json::Value;
use std::time::Duration;

#[tokio::test]
async fn test_connect_to_polkadot_relay_chain() {
    let chain_configs = ChainConfigs::default();

    // Connect to Polkadot relay chain with timeout
    let client = WsClientBuilder::default()
        .request_timeout(Duration::from_secs(30))
        .build("wss://rpc.polkadot.io")
        .await
        .expect("Failed to connect to Polkadot relay chain");

    // Get chain name
    let chain_name: String = client
        .request("system_chain", rpc_params![])
        .await
        .expect("Failed to get chain name");

    assert_eq!(chain_name, "Polkadot", "Should connect to Polkadot");

    // Get runtime version to verify spec_name
    let runtime_version: Value = client
        .request("state_getRuntimeVersion", rpc_params![])
        .await
        .expect("Failed to get runtime version");

    let spec_name = runtime_version["specName"]
        .as_str()
        .expect("spec_name should be present");

    assert_eq!(spec_name, "polkadot", "Spec name should be 'polkadot'");

    // Verify we can load config for this chain
    let config = chain_configs.get(spec_name);
    assert!(config.is_some(), "Should have config for Polkadot");

    let config = config.unwrap();
    assert_eq!(config.legacy_types, "polkadot");
    assert_eq!(format!("{}", config.hasher), "Blake2_256");
    assert!(config.finalizes, "Polkadot should finalize blocks");

    println!("Successfully connected to Polkadot relay chain");
    println!("   Chain: {}", chain_name);
    println!("   Spec: {}", spec_name);
    println!(
        "   Config loaded with legacy_types: {}",
        config.legacy_types
    );
}

#[tokio::test]
async fn test_connect_to_kusama_relay_chain() {
    let chain_configs = ChainConfigs::default();

    // Connect to Kusama relay chain with timeout
    let client = WsClientBuilder::default()
        .request_timeout(Duration::from_secs(30))
        .build("wss://kusama-rpc.polkadot.io")
        .await
        .expect("Failed to connect to Kusama relay chain");

    let chain_name: String = client
        .request("system_chain", rpc_params![])
        .await
        .expect("Failed to get chain name");

    assert_eq!(chain_name, "Kusama", "Should connect to Kusama");

    let runtime_version: Value = client
        .request("state_getRuntimeVersion", rpc_params![])
        .await
        .expect("Failed to get runtime version");

    let spec_name = runtime_version["specName"]
        .as_str()
        .expect("spec_name should be present");

    assert_eq!(spec_name, "kusama", "Spec name should be 'kusama'");

    // Verify config
    let config = chain_configs.get(spec_name).unwrap();
    assert_eq!(config.legacy_types, "polkadot");
    assert!(config.finalizes);

    println!("Successfully connected to Kusama relay chain");
    println!("   Chain: {}", chain_name);
    println!("   Spec: {}", spec_name);
}

#[tokio::test]
async fn test_asset_hub_polkadot_references_relay_chain() {
    let chain_configs = ChainConfigs::default();

    // Connect to Asset Hub Polkadot with timeout
    let client = WsClientBuilder::default()
        .request_timeout(Duration::from_secs(30))
        .build("wss://polkadot-asset-hub-rpc.polkadot.io")
        .await
        .expect("Failed to connect to Asset Hub Polkadot");

    let runtime_version: Value = client
        .request("state_getRuntimeVersion", rpc_params![])
        .await
        .expect("Failed to get runtime version");

    let spec_name = runtime_version["specName"]
        .as_str()
        .expect("spec_name should be present");

    // Asset Hub Polkadot's spec_name is "statemint" (legacy name)
    // Our config has entries for both "statemint" and "asset-hub-polkadot"
    assert!(
        spec_name == "statemint" || spec_name == "asset-hub-polkadot",
        "Spec name should be 'statemint' or 'asset-hub-polkadot', got: {}",
        spec_name
    );

    // Get Asset Hub config (try both names)
    let ahp_config = chain_configs
        .get(spec_name)
        .or_else(|| chain_configs.get("statemint"))
        .or_else(|| chain_configs.get("asset-hub-polkadot"))
        .expect("Should have config for Asset Hub Polkadot");

    // Verify it references Polkadot relay chain
    assert_eq!(
        ahp_config.relay_chain.as_deref(),
        Some("polkadot"),
        "Asset Hub Polkadot should reference 'polkadot' relay chain"
    );
    assert_eq!(ahp_config.para_id, Some(1000));

    // Verify relay chain config exists
    let relay_config = chain_configs.get("polkadot");
    assert!(
        relay_config.is_some(),
        "Relay chain 'polkadot' should have config"
    );

    println!("Asset Hub Polkadot correctly references relay chain");
    println!("   Parachain: {}", spec_name);
    println!(
        "   Relay chain: {}",
        ahp_config.relay_chain.as_ref().unwrap()
    );
    println!("   Para ID: {}", ahp_config.para_id.unwrap());
}

#[tokio::test]
async fn test_dual_connection_config_loading() {
    let chain_configs = ChainConfigs::default();

    // Simulate what AppState::connect_relay_chain does:
    // 1. Connect to Asset Hub with timeout
    let ahp_client = WsClientBuilder::default()
        .request_timeout(Duration::from_secs(30))
        .build("wss://polkadot-asset-hub-rpc.polkadot.io")
        .await
        .expect("Failed to connect to Asset Hub");

    let ahp_runtime: Value = ahp_client
        .request("state_getRuntimeVersion", rpc_params![])
        .await
        .expect("Failed to get AHP runtime");

    let ahp_spec_name = ahp_runtime["specName"].as_str().unwrap();
    let ahp_config = chain_configs.get(ahp_spec_name).unwrap();

    // 2. Connect to its relay chain with timeout
    let relay_client = WsClientBuilder::default()
        .request_timeout(Duration::from_secs(30))
        .build("wss://rpc.polkadot.io")
        .await
        .expect("Failed to connect to relay chain");

    let relay_runtime: Value = relay_client
        .request("state_getRuntimeVersion", rpc_params![])
        .await
        .expect("Failed to get relay runtime");

    let relay_spec_name = relay_runtime["specName"].as_str().unwrap();
    let relay_config = chain_configs.get(relay_spec_name).unwrap();

    // 3. Verify dual-connection config matches
    assert_eq!(
        ahp_config.relay_chain.as_deref(),
        Some(relay_spec_name),
        "Asset Hub config should reference the connected relay chain"
    );

    // 4. Verify relay config properties
    assert_eq!(relay_config.legacy_types, "polkadot");
    assert_eq!(format!("{}", relay_config.hasher), "Blake2_256");

    println!("Dual-connection config loading validated");
    println!(
        "   Parachain: {} (para_id: {})",
        ahp_spec_name,
        ahp_config.para_id.unwrap()
    );
    println!(
        "   Relay: {} (legacy_types: {})",
        relay_spec_name, relay_config.legacy_types
    );
}
