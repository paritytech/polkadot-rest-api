use anyhow::{Context, Result};
use futures::future::join_all;
use integration_tests::{
    client::TestClient, config::TestConfig, constants::API_READY_TIMEOUT_SECONDS,
};
use std::collections::HashMap;
use std::env;

struct LatestTestRunner {
    client: TestClient,
    config: TestConfig,
    chain_name: String,
    latest_block: Option<u64>,
}

impl LatestTestRunner {
    fn new(client: TestClient, config: TestConfig, chain_name: String) -> Self {
        Self {
            client,
            config,
            chain_name,
            latest_block: None,
        }
    }

    /// Fetch the latest block number from /blocks/head
    async fn fetch_latest_block(&mut self) -> Result<u64> {
        if let Some(block) = self.latest_block {
            return Ok(block);
        }

        let block_number = match self.client.get_json("/v1/blocks/head").await {
            Ok((status, json)) if status.is_success() => {
                json.get("number")
                    .or_else(|| json.get("blockNumber"))
                    .or_else(|| json.get("block"))
                    .and_then(|v| v.as_u64())
                    .or_else(|| {
                        json.get("block")
                            .and_then(|b| b.get("number"))
                            .and_then(|v| v.as_u64())
                    })
                    .or_else(|| {
                        // Try parsing as string
                        json.get("number")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<u64>().ok())
                    })
                    .context("Could not extract block number from /blocks/head response")?
            }
            _ => {
                tracing::warn!(
                    "Could not fetch latest block from /v1/blocks/head, tests requiring block height will be skipped"
                );
                0
            }
        };

        self.latest_block = Some(block_number);
        tracing::info!("Latest block number: {}", block_number);

        Ok(block_number)
    }

    /// Run all latest tests for the configured chain
    async fn run_all(&mut self) -> Result<TestResults> {
        // Fetch latest block if not already cached
        let latest_block = self.fetch_latest_block().await?;

        let chain_config = self
            .config
            .get_chain(&self.chain_name)
            .context(format!("Chain {} not found in config", self.chain_name))?
            .clone();

        let mut results = TestResults::default();

        tracing::info!(
            "Running latest tests for chain: {} (block: {})",
            self.chain_name,
            latest_block
        );

        // Test each configured endpoint in parallel
        let endpoint_configs = self.config.latest_endpoints.clone();

        // Create futures for parallel execution
        let futures: Vec<_> = endpoint_configs
            .iter()
            .map(|endpoint_config| self.test_endpoint(endpoint_config, &chain_config, latest_block))
            .collect();

        // Execute all tests in parallel
        let all_results = join_all(futures).await;

        // Aggregate results
        for mut endpoint_results in all_results {
            results.passed += endpoint_results.passed;
            results.failed += endpoint_results.failed;
            results.failures.append(&mut endpoint_results.failures);
        }

        Ok(results)
    }

    /// Test a single endpoint with all its variations
    async fn test_endpoint(
        &self,
        endpoint_config: &integration_tests::config::EndpointConfig,
        chain_config: &integration_tests::config::ChainConfig,
        latest_block: u64,
    ) -> TestResults {
        let mut results = TestResults::default();

        // Build base path with replacements
        let mut replacements = HashMap::new();

        if endpoint_config.requires_block_height {
            replacements.insert("blockId".to_string(), latest_block.to_string());
        }

        // If no query param variations specified, test with empty params
        let query_variations = if endpoint_config.query_params.is_empty() {
            vec![HashMap::new()]
        } else {
            endpoint_config.query_params.clone()
        };

        // If requires account, use test accounts from chain config
        let account_variations = if endpoint_config.requires_account {
            if chain_config.test_accounts.is_empty() {
                vec![None]
            } else {
                chain_config
                    .test_accounts
                    .iter()
                    .map(|a| Some(a.clone()))
                    .collect()
            }
        } else {
            vec![None]
        };

        // Test all combinations
        for account_id in &account_variations {
            for query_params in &query_variations {
                if let Some(acc) = account_id {
                    replacements.insert("accountId".to_string(), acc.clone());
                }

                // Replace placeholders in query params
                let mut resolved_query_params = HashMap::new();
                for (key, value) in query_params {
                    let resolved_key =
                        integration_tests::utils::replace_placeholders(key, &replacements);
                    let resolved_value =
                        integration_tests::utils::replace_placeholders(value, &replacements);
                    resolved_query_params.insert(resolved_key, resolved_value);
                }

                let endpoint_path = integration_tests::utils::replace_placeholders(
                    &endpoint_config.path,
                    &replacements,
                );
                let query_string =
                    integration_tests::utils::build_query_string(&resolved_query_params);
                let full_path = format!("{}{}", endpoint_path, query_string);

                match self.test_single_request(&full_path).await {
                    Ok(()) => {
                        results.passed += 1;
                        tracing::debug!("✓ Passed: {}", full_path);
                    }
                    Err(e) => {
                        results.failed += 1;
                        results.failures.push(Failure {
                            endpoint: full_path.clone(),
                            description: None,
                            error: format!("{:?}", e),
                        });
                        tracing::error!("✗ Failed: {} - {}", full_path, e);
                    }
                }
            }
        }

        results
    }

    /// Test a single request and validate response
    async fn test_single_request(&self, path: &str) -> Result<()> {
        let (status, _json) = self
            .client
            .get_json(path)
            .await
            .context(format!("Failed to fetch endpoint: {}", path))?;

        if !status.is_success() {
            anyhow::bail!("Request failed with status {}", status);
        }

        Ok(())
    }
}

#[derive(Default)]
struct TestResults {
    passed: usize,
    failed: usize,
    failures: Vec<Failure>,
}

#[derive(Debug)]
struct Failure {
    endpoint: String,
    #[allow(dead_code)]
    description: Option<String>,
    error: String,
}

/// Helper function to run latest tests for a specific chain
async fn run_latest_test_for_chain(chain_name: &str) -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let config_path = env::var("TEST_CONFIG_PATH").unwrap_or_else(|_| {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        format!("{}/tests/config/test_config.json", manifest_dir)
    });

    let client = TestClient::new(api_url);
    let config = TestConfig::from_file(&config_path)?;

    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;

    let mut runner = LatestTestRunner::new(client, config, chain_name.to_string());
    let results = runner.run_all().await?;

    println!("\n=== Latest Test Results for {} ===", chain_name);
    println!("Passed: {}", results.passed);
    println!("Failed: {}", results.failed);

    if !results.failures.is_empty() {
        println!("\nFailures:");
        for failure in &results.failures {
            println!("  - {}: {}", failure.endpoint, failure.error);
        }
    }

    assert_eq!(results.failed, 0, "{} test(s) failed", results.failed);

    Ok(())
}

#[tokio::test]
async fn test_latest_polkadot() -> Result<()> {
    run_latest_test_for_chain("polkadot").await
}

#[tokio::test]
async fn test_latest_kusama() -> Result<()> {
    run_latest_test_for_chain("kusama").await
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
