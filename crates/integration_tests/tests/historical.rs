use anyhow::{Context, Result};
use colored::Colorize;
use integration_tests::{
    client::TestClient, config::TestConfig, constants::API_READY_TIMEOUT_SECONDS,
    fixtures::FixtureLoader, utils::compare_json,
};
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use tokio::time::sleep;

/// Test runner for historical integration tests
struct HistoricalTestRunner {
    client: TestClient,
    config: TestConfig,
    fixture_loader: FixtureLoader,
    chain_name: String,
}

impl HistoricalTestRunner {
    fn new(
        client: TestClient,
        config: TestConfig,
        fixture_loader: FixtureLoader,
        chain_name: String,
    ) -> Self {
        Self {
            client,
            config,
            fixture_loader,
            chain_name,
        }
    }

    /// Run all historical tests for the configured chain
    async fn run_all(&self) -> Result<TestResults> {
        let test_cases = self.config.get_historical_tests(&self.chain_name);
        let total_tests = test_cases.len();

        // Get delay between requests from environment variable (default: 0ms)
        // Set TEST_DELAY_MS to add delay between requests to avoid rate limiting
        let delay_ms: u64 = env::var("TEST_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        println!(
            "\n{} {} historical test cases for chain: {}\n",
            "Running".cyan().bold(),
            total_tests,
            self.chain_name.yellow()
        );

        if delay_ms > 0 {
            println!(
                "{}: {}ms delay between requests\n",
                "Rate limit protection".yellow(),
                delay_ms
            );
        }

        // Run tests sequentially with optional delay to avoid rate limiting
        let mut results = TestResults::default();
        for test_case in test_cases {
            // Add delay before each request (except the first one)
            if delay_ms > 0 && (results.passed + results.failed) > 0 {
                sleep(Duration::from_millis(delay_ms)).await;
            }

            let result = self.run_test_case(&test_case).await;
            let test_name = if let Some(block_height) = test_case.block_height {
                format!("{} (block {})", test_case.endpoint, block_height)
            } else {
                test_case.endpoint.clone()
            };

            match result {
                Ok(()) => {
                    results.passed += 1;
                    println!("{} {}", "✓".green().bold(), test_name);
                }
                Err(e) => {
                    results.failed += 1;
                    results.failures.push(Failure {
                        endpoint: test_case.endpoint.clone(),
                        description: test_case.description.clone(),
                        error: format!("{:?}", e),
                    });
                    println!("{} {} - {}", "✗".red().bold(), test_name, e);
                }
            }
        }

        Ok(results)
    }

    /// Run a single historical test case
    async fn run_test_case(
        &self,
        test_case: &integration_tests::config::HistoricalTestCase,
    ) -> Result<()> {
        // Build the endpoint path with replacements
        let mut replacements = HashMap::new();

        if let Some(block_height) = test_case.block_height {
            replacements.insert("blockId".to_string(), block_height.to_string());
        }

        if let Some(ref account_id) = test_case.account_id {
            replacements.insert("accountId".to_string(), account_id.clone());
        }

        if let Some(extrinsic_index) = test_case.extrinsic_index {
            replacements.insert("extrinsicIndex".to_string(), extrinsic_index.to_string());
        }

        if let Some(ref asset_id) = test_case.asset_id {
            replacements.insert("assetId".to_string(), asset_id.clone());
        }

        let endpoint_path =
            integration_tests::utils::replace_placeholders(&test_case.endpoint, &replacements);
        let query_string = integration_tests::utils::build_query_string(&test_case.query_params);
        let full_path = format!("{}{}", endpoint_path, query_string);

        // Make the API request
        let (status, actual_json) = self
            .client
            .get_json(&full_path)
            .await
            .context(format!("Failed to fetch endpoint: {}", full_path))?;

        // Check status code
        if !status.is_success() {
            anyhow::bail!(
                "Request failed with status {}: {}",
                status,
                serde_json::to_string_pretty(&actual_json).unwrap_or_default()
            );
        }

        // Load expected fixture
        let expected_json = self
            .fixture_loader
            .load(&test_case.fixture_path)
            .context(format!(
                "Failed to load fixture: {:?}",
                test_case.fixture_path
            ))?;

        // Compare responses
        // Ignore fields that may vary (timestamps, etc.)
        let ignore_fields = vec!["timestamp", "at", "blockNumber", "blockHash"];
        let comparison = compare_json(&actual_json, &expected_json, &ignore_fields)
            .context("Failed to compare JSON responses")?;

        if !comparison.is_match() {
            anyhow::bail!(
                "Response mismatch:{}",
                comparison.format_diff(&expected_json, &actual_json)
            );
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
    description: Option<String>,
    error: String,
}

/// Helper function to run historical tests for a specific chain
async fn run_historical_test_for_chain(chain_name: &str) -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let config_path = env::var("TEST_CONFIG_PATH").unwrap_or_else(|_| {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        format!("{}/tests/config/test_config.json", manifest_dir)
    });
    let fixtures_dir = env::var("FIXTURES_DIR").unwrap_or_else(|_| {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        format!("{}/tests/fixtures", manifest_dir)
    });

    let client = TestClient::new(api_url);
    let config = TestConfig::from_file(&config_path)?;
    let fixture_loader = FixtureLoader::new(&fixtures_dir);

    // Wait for API to be ready
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;

    let runner = HistoricalTestRunner::new(client, config, fixture_loader, chain_name.to_string());
    let results = runner.run_all().await?;

    println!("\n{}", "═".repeat(60).bright_white());
    println!("Historical Test Results for {}", chain_name.yellow().bold());
    println!("{}", "═".repeat(60).bright_white());
    println!(
        "  {} Passed: {}",
        "✓".green().bold(),
        results.passed.to_string().green()
    );
    println!(
        "  {} Failed: {}",
        "✗".red().bold(),
        results.failed.to_string().red()
    );
    println!("{}\n", "═".repeat(60).bright_white());

    if !results.failures.is_empty() {
        println!("{}", "Failures:".red().bold());
        for failure in &results.failures {
            println!("  {} {}", "•".red(), failure.endpoint);
            println!("    {}: {}", "Error".red(), failure.error);
            if let Some(ref desc) = failure.description {
                println!("    {}: {}", "Description".yellow(), desc);
            }
        }
        println!();
    }

    assert_eq!(results.failed, 0, "{} test(s) failed", results.failed);

    Ok(())
}

#[tokio::test]
async fn test_historical_polkadot() -> Result<()> {
    run_historical_test_for_chain("polkadot").await
}

#[tokio::test]
async fn test_historical_kusama() -> Result<()> {
    run_historical_test_for_chain("kusama").await
}

#[tokio::test]
async fn test_historical_asset_hub_polkadot() -> Result<()> {
    run_historical_test_for_chain("asset-hub-polkadot").await
}

#[tokio::test]
async fn test_historical_asset_hub_kusama() -> Result<()> {
    run_historical_test_for_chain("asset-hub-kusama").await
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
