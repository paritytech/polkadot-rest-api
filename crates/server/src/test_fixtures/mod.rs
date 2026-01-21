//! Test fixtures and helpers for mocking subxt clients with real metadata.
//!
//! This module provides utilities for creating mock RPC clients that can be used
//! with `OnlineClient::from_rpc_client()` for testing handlers that
//! use `client.at_block()` or `client.at_current_block()`.

use parity_scale_codec::{Compact, Encode};
use serde_json::{json, value::RawValue};
use std::sync::Arc;
use subxt_rpcs::client::mock_rpc_client::{Json as MockJson, MockRpcClientBuilder};
use subxt_rpcs::client::{MockRpcClient, RpcClient};

/// Raw SCALE-encoded metadata from Asset Hub Polkadot.
/// This is fetched via `state_getMetadata` RPC call and saved as a fixture.
pub const ASSET_HUB_METADATA: &[u8] = include_bytes!("asset_hub_polkadot_metadata.scale");

/// Default test block hash used in mocks.
pub const TEST_BLOCK_HASH: &str =
    "0x1234567890123456789012345678901234567890123456789012345678901234";

/// Default genesis hash used in mocks.
pub const TEST_GENESIS_HASH: &str =
    "0x0000000000000000000000000000000000000000000000000000000000000000";

/// Default test block number.
pub const TEST_BLOCK_NUMBER: u64 = 100;

/// Default spec version for Asset Hub Polkadot.
pub const TEST_SPEC_VERSION: u32 = 1_004_000;

/// Default transaction version.
pub const TEST_TRANSACTION_VERSION: u32 = 15;

/// Encode a Core_version response that subxt expects.
/// This is decoded as SpecVersionHeader in subxt.
fn encode_core_version_response() -> Vec<u8> {
    // SpecVersionHeader structure:
    // - spec_name: String
    // - impl_name: String
    // - authoring_version: u32
    // - spec_version: u32
    // - impl_version: u32
    // - apis: Vec<([u8; 8], u32)>
    // - transaction_version: u32
    let mut encoded = Vec::new();

    // spec_name (compact-prefixed string)
    let spec_name = "statemint";
    Compact(spec_name.len() as u32).encode_to(&mut encoded);
    encoded.extend_from_slice(spec_name.as_bytes());

    // impl_name
    let impl_name = "statemint";
    Compact(impl_name.len() as u32).encode_to(&mut encoded);
    encoded.extend_from_slice(impl_name.as_bytes());

    // authoring_version
    1u32.encode_to(&mut encoded);

    // spec_version
    TEST_SPEC_VERSION.encode_to(&mut encoded);

    // impl_version
    0u32.encode_to(&mut encoded);

    // apis (empty vec)
    Compact(0u32).encode_to(&mut encoded);

    // transaction_version
    TEST_TRANSACTION_VERSION.encode_to(&mut encoded);

    encoded
}

/// Encode metadata response for Metadata_metadata runtime call.
/// The response is: (Compact<u32>, RuntimeMetadataPrefixed)
/// where the Compact is the length of the metadata bytes.
fn encode_metadata_response() -> Vec<u8> {
    let mut encoded = Vec::new();

    // Compact length prefix
    Compact(ASSET_HUB_METADATA.len() as u32).encode_to(&mut encoded);

    // The actual metadata bytes
    encoded.extend_from_slice(ASSET_HUB_METADATA);

    encoded
}

/// Create a MockRpcClient builder pre-configured with handlers for:
/// - OnlineClient initialization (rpc_methods, chain_getBlockHash for genesis)
/// - at_current_block() (chain_getFinalizedHead)
/// - at_block() (chain_getBlockHash, chain_getHeader)
/// - Metadata fetching (state_call for Core_version, Metadata_metadata)
///
/// You can add additional handlers to the returned builder before calling .build()
pub fn mock_rpc_client_builder() -> MockRpcClientBuilder {
    let core_version_response = encode_core_version_response();
    let metadata_response = encode_metadata_response();

    MockRpcClient::builder()
        // Required for OnlineClient initialization
        .method_handler("rpc_methods", async |_params| {
            MockJson(json!({ "methods": [] }))
        })
        // Genesis hash lookup (block 0)
        .method_handler("chain_getBlockHash", async |_params| {
            // This is called for both genesis (param: 0) and other blocks
            // For simplicity, return the same hash - tests can override if needed
            MockJson(TEST_BLOCK_HASH)
        })
        // Finalized head for at_current_block()
        .method_handler("chain_getFinalizedHead", async |_params| {
            MockJson(TEST_BLOCK_HASH)
        })
        // Block header for at_block(hash)
        .method_handler("chain_getHeader", async |_params| {
            MockJson(json!({
                "number": format!("0x{:x}", TEST_BLOCK_NUMBER),
                "parentHash": TEST_GENESIS_HASH,
                "stateRoot": TEST_GENESIS_HASH,
                "extrinsicsRoot": TEST_GENESIS_HASH,
                "digest": { "logs": [] }
            }))
        })
        // state_call handler for Core_version and Metadata_metadata
        .method_handler("state_call", move |params: Option<Box<RawValue>>| {
            let core_version = core_version_response.clone();
            let metadata = metadata_response.clone();

            async move {
                // params is [method_name, data, block_hash] as JSON array
                let method = params
                    .and_then(|p| serde_json::from_str::<serde_json::Value>(p.get()).ok())
                    .and_then(|v| v.get(0).and_then(|m| m.as_str().map(String::from)))
                    .unwrap_or_default();

                match method.as_str() {
                    "Core_version" => MockJson(format!("0x{}", hex::encode(&core_version))),
                    "Metadata_metadata_versions" => {
                        // Return empty to trigger fallback to Metadata_metadata
                        // This simplifies the mock since we don't need to handle
                        // the versioned metadata API
                        MockJson(format!("0x{}", hex::encode(Compact(0u32).encode())))
                    }
                    "Metadata_metadata" => MockJson(format!("0x{}", hex::encode(&metadata))),
                    _ => {
                        // Unknown method - return empty
                        MockJson("0x".to_string())
                    }
                }
            }
        })
}

/// Create a pre-configured MockRpcClient suitable for most tests.
pub fn create_mock_rpc_client() -> MockRpcClient {
    mock_rpc_client_builder().build()
}

/// Create an RpcClient from the mock.
pub fn create_rpc_client() -> Arc<RpcClient> {
    Arc::new(RpcClient::new(create_mock_rpc_client()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_fixture_loads() {
        assert!(!ASSET_HUB_METADATA.is_empty());
        // Check magic number "meta"
        assert_eq!(&ASSET_HUB_METADATA[0..4], b"meta");
    }

    #[test]
    fn test_core_version_encoding() {
        let encoded = encode_core_version_response();
        assert!(!encoded.is_empty());
    }

    #[test]
    fn test_metadata_response_encoding() {
        let encoded = encode_metadata_response();
        // Should be larger than raw metadata due to length prefix
        assert!(encoded.len() > ASSET_HUB_METADATA.len());
    }

    #[tokio::test]
    async fn test_mock_client_creates_online_client() {
        let mock_client = create_mock_rpc_client();
        let rpc_client = RpcClient::new(mock_client);

        // This should succeed with our mock handlers
        let result =
            subxt::OnlineClient::<subxt::SubstrateConfig>::from_rpc_client(rpc_client).await;

        assert!(
            result.is_ok(),
            "Failed to create OnlineClient: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_mock_client_supports_at_current_block() {
        let mock_client = create_mock_rpc_client();
        let rpc_client = RpcClient::new(mock_client);

        let client = subxt::OnlineClient::<subxt::SubstrateConfig>::from_rpc_client(rpc_client)
            .await
            .expect("Failed to create OnlineClient");

        let result = client.at_current_block().await;
        assert!(
            result.is_ok(),
            "Failed at_current_block: {:?}",
            result.err()
        );

        let at_block = result.unwrap();
        assert_eq!(at_block.block_number(), TEST_BLOCK_NUMBER);
    }

    #[tokio::test]
    async fn test_mock_client_supports_at_block_number() {
        let mock_client = create_mock_rpc_client();
        let rpc_client = RpcClient::new(mock_client);

        let client = subxt::OnlineClient::<subxt::SubstrateConfig>::from_rpc_client(rpc_client)
            .await
            .expect("Failed to create OnlineClient");

        let result = client.at_block(42u64).await;
        assert!(
            result.is_ok(),
            "Failed at_block(number): {:?}",
            result.err()
        );
    }
}
