use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

/// HTTP client for making API requests during tests
#[derive(Clone)]
pub struct TestClient {
    base_url: String,
    client: Client,
    #[allow(dead_code)]
    timeout: Duration,
}

impl TestClient {
    /// Create a new test client
    pub fn new(base_url: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            timeout: Duration::from_secs(30),
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
    pub async fn wait_for_ready(&self, max_retries: u32) -> Result<()> {
        for i in 0..max_retries {
            match self.get("/v1/health").await {
                Ok(response) if response.status.is_success() => {
                    tracing::info!("API is ready");
                    return Ok(());
                }
                _ => {
                    if i < max_retries - 1 {
                        tracing::debug!(
                            "API not ready yet, retrying in 1s... (attempt {}/{})",
                            i + 1,
                            max_retries
                        );
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
        anyhow::bail!("API did not become ready after {} attempts", max_retries)
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
