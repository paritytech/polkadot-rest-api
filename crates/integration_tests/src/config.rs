// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration for a single chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Chain name (e.g., "polkadot", "kusama")
    pub name: String,
    /// Base URL for the API when testing this chain
    #[serde(default = "default_api_url")]
    pub api_url: String,
    /// Test accounts to use for account-related endpoints
    #[serde(default)]
    pub test_accounts: Vec<String>,
    /// Additional chain-specific configuration
    #[serde(default)]
    pub extra: HashMap<String, serde_json::Value>,
}

fn default_api_url() -> String {
    "http://localhost:8080".to_string()
}

/// Endpoint configuration for latest tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfig {
    pub path: String,
    /// Query parameter variations to test
    #[serde(default)]
    pub query_params: Vec<HashMap<String, String>>,
    /// Whether this endpoint requires a block height parameter
    #[serde(default)]
    pub requires_block_height: bool,
    /// Whether this endpoint requires an account identifier
    #[serde(default)]
    pub requires_account: bool,
    /// Optional list of chains to test this endpoint on.
    /// If None or empty, the endpoint is tested on all chains.
    #[serde(default)]
    pub only_chains: Option<Vec<String>>,
    /// Whether this endpoint is only available on relay chains (not asset hubs)
    #[serde(default)]
    pub relay_chain_only: bool,
}

/// Historical test case configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalTestCase {
    /// Endpoint path (with placeholders like {blockId} or {accountId})
    pub endpoint: String,
    /// Block height to test against
    pub block_height: Option<u64>,
    /// Account identifier to test
    pub account_id: Option<String>,
    /// Asset identifier to test
    pub asset_id: Option<String>,
    /// Extrinsic index to test
    pub extrinsic_index: Option<u64>,
    /// Pool identifier for nomination pools
    pub pool_id: Option<String>,
    /// Query parameters (if any)
    #[serde(default)]
    pub query_params: HashMap<String, String>,
    /// Path to the expected JSON fixture file
    pub fixture_path: PathBuf,
    /// Optional description of the test case
    pub description: Option<String>,
}

/// Test configuration loaded from file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    /// Chain configurations
    pub chains: Vec<ChainConfig>,
    /// Endpoints to test in latest tests
    #[serde(default)]
    pub latest_endpoints: Vec<EndpointConfig>,
    /// Historical test cases organized by chain
    #[serde(default)]
    pub historical_tests: HashMap<String, Vec<HistoricalTestCase>>,
}

impl TestConfig {
    /// Load test configuration from a JSON file
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref()).context(format!(
            "Failed to read test config from {:?}",
            path.as_ref()
        ))?;
        let config: TestConfig = serde_json::from_str(&content).context(format!(
            "Failed to parse test config from {:?}",
            path.as_ref()
        ))?;
        Ok(config)
    }

    /// Get chain configuration by name
    pub fn get_chain(&self, name: &str) -> Option<&ChainConfig> {
        self.chains.iter().find(|c| c.name == name)
    }

    /// Get historical test cases for a specific chain
    pub fn get_historical_tests(&self, chain_name: &str) -> Vec<HistoricalTestCase> {
        self.historical_tests
            .get(chain_name)
            .cloned()
            .unwrap_or_default()
    }
}

/// Default test configuration for common chains
impl Default for TestConfig {
    fn default() -> Self {
        Self {
            chains: vec![
                ChainConfig {
                    name: "polkadot".to_string(),
                    api_url: "http://localhost:8080".to_string(),
                    test_accounts: vec![],
                    extra: HashMap::new(),
                },
                ChainConfig {
                    name: "kusama".to_string(),
                    api_url: "http://localhost:8080".to_string(),
                    test_accounts: vec![],
                    extra: HashMap::new(),
                },
                ChainConfig {
                    name: "westend".to_string(),
                    api_url: "http://localhost:8080".to_string(),
                    test_accounts: vec![],
                    extra: HashMap::new(),
                },
            ],
            latest_endpoints: vec![],
            historical_tests: HashMap::new(),
        }
    }
}
