//! Integration tests for /accounts endpoints
//!
//! This module contains all account-related integration tests, unified into submodules.

use anyhow::{Context, Result};
use integration_tests::{client::TestClient, constants::API_READY_TIMEOUT_SECONDS};
use std::env;
use std::sync::OnceLock;

// Re-export colored for use in submodules
pub use colored::Colorize;

// Submodules
mod asset_approvals;
mod asset_balances;
mod balance_info;
mod compare;
mod convert;
mod pool_asset_approvals;
mod pool_asset_balances;
mod proxy_info;
mod staking_info;
mod staking_payouts;
mod validate;
mod vesting_info;

// ================================================================================================
// Shared Test Client
// ================================================================================================

static CLIENT: OnceLock<TestClient> = OnceLock::new();

pub async fn get_client() -> Result<TestClient> {
    let client = CLIENT.get_or_init(|| {
        init_tracing();
        let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
        TestClient::new(api_url)
    });

    client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    Ok(client.clone())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

// ================================================================================================
// Endpoint Type Abstraction (for tests supporting both standard and RC endpoints)
// ================================================================================================

#[derive(Clone, Copy)]
pub enum EndpointType {
    Standard,
    RelayChain,
}

impl EndpointType {
    pub fn base_path(&self) -> &'static str {
        match self {
            EndpointType::Standard => "/accounts",
            EndpointType::RelayChain => "/rc/accounts",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            EndpointType::Standard => "Standard",
            EndpointType::RelayChain => "RelayChain",
        }
    }
}

// ================================================================================================
// Common Helper Functions
// ================================================================================================

/// Check if relay chain is not available and test should be skipped
pub fn should_skip_rc_test(status: u16, json: &serde_json::Value) -> bool {
    if status == 400 {
        if let Some(response_obj) = json.as_object() {
            if let Some(error) = response_obj.get("error") {
                let error_str = error.as_str().unwrap_or("");
                if error_str.contains("Relay chain not available") {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if staking pallet is not available and test should be skipped
pub fn should_skip_staking_test(status: u16, json: &serde_json::Value) -> bool {
    if status == 400 || status == 500 {
        if let Some(response_obj) = json.as_object() {
            if let Some(error) = response_obj.get("error") {
                let error_str = error.as_str().unwrap_or("");
                if error_str.contains("Staking pallet not available")
                    || error_str.contains("not available on this chain")
                {
                    return true;
                }
            }
        }
    }
    false
}

pub fn print_skip_message(test_name: &str, reason: &str) {
    println!("  {} {}", "⚠".yellow(), reason);
    println!(
        "{} {} test skipped - {}",
        "⚠".yellow().bold(),
        test_name,
        reason
    );
    println!("{}", "═".repeat(80).bright_white());
}

pub fn print_test_header(test_name: &str) {
    println!("\n{} {}", "Testing".cyan().bold(), test_name);
    println!("{}", "═".repeat(80).bright_white());
}

pub fn print_test_success(message: &str) {
    println!("{} {}", "✓".green().bold(), message);
    println!("{}", "═".repeat(80).bright_white());
}
