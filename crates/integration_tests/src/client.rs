use anyhow::{Context, Result};
use colored::Colorize;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

/// Default timeout for regular API requests (2 minutes)
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Short timeout for health check connections (2 seconds)
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// HTTP client for making API requests during tests
#[derive(Clone)]
pub struct TestClient {
    base_url: String,
    client: Client,
    health_check_client: Client,
    #[allow(dead_code)]
    timeout: Duration,
}

impl TestClient {
    /// Create a new test client
    pub fn new(base_url: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: Client::builder()
                .timeout(DEFAULT_REQUEST_TIMEOUT)
                .build()
                .expect("Failed to create HTTP client"),
            health_check_client: Client::builder()
                .timeout(HEALTH_CHECK_TIMEOUT)
                .build()
                .expect("Failed to create health check client"),
            timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Create a new test client with custom timeout
    pub fn with_timeout(base_url: String, timeout: Duration) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: Client::builder()
                .timeout(timeout)
                .build()
                .expect("Failed to create HTTP client"),
            health_check_client: Client::builder()
                .timeout(HEALTH_CHECK_TIMEOUT)
                .build()
                .expect("Failed to create health check client"),
            timeout,
        }
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Make a GET request to the API
    pub async fn get(&self, path: &str) -> Result<ApiResponse> {
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{}", self.base_url, path)
        };

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context(format!("Failed to send GET request to {}", url))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        Ok(ApiResponse {
            status,
            body,
            url: url.clone(),
        })
    }

    /// Make a GET request and parse JSON response
    pub async fn get_json(&self, path: &str) -> Result<(reqwest::StatusCode, Value)> {
        let response = self.get(path).await?;
        let json: Value = serde_json::from_str(&response.body).context(format!(
            "Failed to parse JSON response from {}",
            response.url
        ))?;
        Ok((response.status, json))
    }

    /// Wait for the API to be ready
    ///
    /// Uses a short timeout for health checks to fail fast when no server is running.
    pub async fn wait_for_ready(&self, max_retries: u32) -> Result<()> {
        println!("Waiting for API at {} ...", self.base_url.cyan());

        for i in 0..max_retries {
            let url = format!("{}/v1/health", self.base_url);

            match self.health_check_client.get(&url).send().await {
                Ok(response) if response.status().is_success() => {
                    println!(
                        "{} API is ready (took {} seconds)",
                        "ok:".green().bold(),
                        i + 1
                    );
                    return Ok(());
                }
                Ok(response) => {
                    // Server is running but returned non-success status
                    println!(
                        "  Attempt {}/{}: Server returned status {}",
                        i + 1,
                        max_retries,
                        response.status()
                    );
                }
                Err(e) => {
                    // Connection failed - server likely not running
                    let reason = if e.is_connect() {
                        "connection refused"
                    } else if e.is_timeout() {
                        "timeout"
                    } else {
                        "error"
                    };
                    println!(
                        "  Attempt {}/{}: {} ({})",
                        i + 1,
                        max_retries,
                        reason,
                        format!("{:.1}s timeout", HEALTH_CHECK_TIMEOUT.as_secs_f32())
                            .bright_black()
                    );
                }
            }

            if i < max_retries - 1 {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }

        println!(
            "\n{} API did not become ready after {} seconds",
            "error:".red().bold(),
            max_retries
        );
        println!(
            "\n{} Make sure the server is running:",
            "hint:".cyan().bold()
        );
        println!("  cargo run --release --bin polkadot-rest-api\n");

        anyhow::bail!(
            "API at {} did not become ready after {} attempts",
            self.base_url,
            max_retries
        )
    }
}

/// API response wrapper
#[derive(Debug)]
pub struct ApiResponse {
    pub status: reqwest::StatusCode,
    pub body: String,
    pub url: String,
}

impl ApiResponse {
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Parse the response body as JSON
    pub fn json(&self) -> Result<Value> {
        serde_json::from_str(&self.body)
            .context(format!("Failed to parse JSON response from {}", self.url))
    }
}
