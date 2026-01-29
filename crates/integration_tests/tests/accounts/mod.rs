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
// Test Account Addresses
// ================================================================================================

/// Well-known test addresses for integration tests.
/// These addresses are chosen to be valid on various Substrate chains.
pub mod test_accounts {
    // =============================================================================================
    // Polkadot Relay Chain Addresses
    // =============================================================================================

    /// Polkadot relay chain staker (Web3 Foundation validator)
    pub const POLKADOT_STAKER: &str = "16SpacegeUTft9v3ts27CEC3tJaxgvE4uZeCctThFH3Vb24p";

    /// Alternative Polkadot staker (Parity validator)
    pub const POLKADOT_STAKER_ALT: &str = "1zugcag7cJVBtVRnFxv5Qftn7xKAnR6YJ9x4x3XLgGgmNnS";

    /// Polkadot treasury account
    pub const POLKADOT_TREASURY: &str = "13UVJyLnbVp9RBZYFwFGyDvVd1y27Tt8tkntv6Q7JVPhFsTB";

    // =============================================================================================
    // Generic Substrate Addresses (Alice, Bob, etc.)
    // =============================================================================================

    /// Alice's SS58 address (generic substrate prefix 42)
    pub const ALICE: &str = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";

    /// Alice's hex public key
    pub const ALICE_HEX: &str =
        "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";

    /// Bob's SS58 address (generic substrate prefix 42)
    pub const BOB: &str = "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty";

    /// Charlie's SS58 address (generic substrate prefix 42)
    pub const CHARLIE: &str = "5FLSigC9HGRKVhB9FiEo4Y3koPsNmBmLJbpXg2mp1hXcS59Y";

    // =============================================================================================
    // Asset Hub Addresses
    // =============================================================================================

    /// Asset Hub Polkadot address with known assets
    pub const ASSET_HUB_ACCOUNT: &str = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";

    // =============================================================================================
    // Special Purpose Addresses
    // =============================================================================================

    /// Known non-stash address for negative staking tests
    pub const NON_STASH_ADDRESS: &str = "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty";

    /// Invalid address string for error handling tests
    pub const INVALID_ADDRESS: &str = "invalid-address-123";

    /// Empty address for edge case tests
    pub const EMPTY_ADDRESS: &str = "";
}

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

#[derive(Clone, Copy, Debug)]
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

    /// Get a primary test account appropriate for this endpoint type
    pub fn get_test_account(&self) -> &'static str {
        match self {
            EndpointType::Standard => test_accounts::ALICE,
            EndpointType::RelayChain => test_accounts::POLKADOT_TREASURY,
        }
    }

    /// Get a staker account appropriate for this endpoint type
    pub fn get_test_staker(&self) -> &'static str {
        match self {
            EndpointType::Standard => test_accounts::ALICE, // May not be a staker
            EndpointType::RelayChain => test_accounts::POLKADOT_STAKER,
        }
    }

    /// Get an alternative staker account
    pub fn get_alt_test_staker(&self) -> &'static str {
        match self {
            EndpointType::Standard => test_accounts::BOB,
            EndpointType::RelayChain => test_accounts::POLKADOT_STAKER_ALT,
        }
    }

    /// Get a hex format address for testing
    pub fn get_hex_address(&self) -> &'static str {
        test_accounts::ALICE_HEX
    }

    /// Get an invalid address for error testing
    pub fn get_invalid_address(&self) -> &'static str {
        test_accounts::INVALID_ADDRESS
    }

    /// Get a non-stash address for negative staking tests
    pub fn get_non_stash_address(&self) -> &'static str {
        test_accounts::NON_STASH_ADDRESS
    }

    /// Get a recommended historical block number for this endpoint type
    pub fn get_historical_block(&self) -> u64 {
        match self {
            EndpointType::Standard => 5_000_000, // Asset Hub historical block
            EndpointType::RelayChain => 20_000_000, // Polkadot relay chain historical block
        }
    }

    /// Get a recent block number for this endpoint type
    pub fn get_recent_block(&self) -> u64 {
        match self {
            EndpointType::Standard => 8_000_000,
            EndpointType::RelayChain => 23_000_000,
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
