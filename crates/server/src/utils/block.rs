// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::{AppState, SubstrateLegacyRpc};
use primitive_types::H256;
use std::str::FromStr;
use subxt::client::BlockNumberOrRef;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};
use subxt_rpcs::{RpcClient, rpc_params};
use thiserror::Error;

/// Represents a block identifier that can be either a hash or a number
#[derive(Debug, Clone)]
pub enum BlockId {
    /// Block hash (32 bytes)
    Hash(H256),
    /// Block number
    Number(u64),
}

/// Implement conversion to Subxt's BlockNumberOrRef for use with client.at_block()
impl From<BlockId> for BlockNumberOrRef<SubstrateConfig> {
    fn from(block_id: BlockId) -> Self {
        match block_id {
            BlockId::Hash(hash) => hash.into(),
            BlockId::Number(number) => number.into(),
        }
    }
}

/// Error type for parsing BlockId from string
#[derive(Debug, Error)]
pub enum BlockIdParseError {
    #[error("Invalid block number")]
    InvalidNumber(#[source] std::num::ParseIntError),

    #[error("Invalid block hash format")]
    InvalidHash(#[source] rustc_hex::FromHexError),
}

impl FromStr for BlockId {
    type Err = BlockIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try to parse as H256 first (starts with "0x")
        if s.starts_with("0x") {
            H256::from_str(s)
                .map(BlockId::Hash)
                .map_err(BlockIdParseError::InvalidHash)
        } else {
            // Otherwise try to parse as block number
            s.parse::<u64>()
                .map(BlockId::Number)
                .map_err(BlockIdParseError::InvalidNumber)
        }
    }
}

#[derive(Debug, Error)]
pub enum BlockResolveError {
    #[error("Block not found: {0}")]
    NotFound(String),

    #[error("Failed to get finalized head")]
    FinalizedHeadFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get block hash")]
    BlockHashFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get block header")]
    BlockHeaderFailed(#[source] subxt_rpcs::Error),

    #[error("Block number not found in header")]
    BlockNumberNotFound,

    #[error("Failed to parse block number from header")]
    BlockNumberParseFailed(#[source] std::num::ParseIntError),

    #[error("RPC error")]
    RpcError(#[source] subxt_rpcs::Error),
}

/// Represents a resolved block with both hash and number
#[derive(Debug, Clone)]
pub struct ResolvedBlock {
    /// Block hash as hex string (with 0x prefix)
    pub hash: String,
    /// Block number
    pub number: u64,
}

/// Fetch the timestamp from the Timestamp.Now storage entry at a given block.
pub async fn fetch_block_timestamp(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<String> {
    // Use typed dynamic storage to decode timestamp directly as u64
    let timestamp_addr = subxt::dynamic::storage::<(), u64>("Timestamp", "Now");
    let timestamp = client_at_block
        .storage()
        .fetch(timestamp_addr, ())
        .await
        .ok()?;
    let timestamp_value = timestamp.decode().ok()?;
    Some(timestamp_value.to_string())
}

/// Helper function to get header JSON and extract block number from hash
async fn get_block_number_from_hash(
    state: &AppState,
    hash: &str,
) -> Result<u64, BlockResolveError> {
    // Make raw RPC call to get the header data as JSON
    // We need to use raw JSON because subxt-historic's RpcConfig has Header = ()
    let header_json = state
        .get_header_json(hash)
        .await
        .map_err(BlockResolveError::RpcError)?;

    // Check if the response is null (block doesn't exist)
    if header_json.is_null() {
        return Err(BlockResolveError::NotFound(format!(
            "Block with hash {} not found",
            hash
        )));
    }

    // Extract block number from the header JSON
    // The response structure is: { "number": "0x..." }
    let number_hex = header_json
        .get("number")
        .and_then(|v| v.as_str())
        .ok_or(BlockResolveError::BlockNumberNotFound)?;

    // Parse hex string to u64 (remove 0x prefix)
    let number = u64::from_str_radix(number_hex.trim_start_matches("0x"), 16)
        .map_err(BlockResolveError::BlockNumberParseFailed)?;

    Ok(number)
}

async fn get_header_json_with_rpc(
    rpc_client: &RpcClient,
    hash: &str,
) -> Result<serde_json::Value, BlockResolveError> {
    rpc_client
        .request("chain_getHeader", rpc_params![hash])
        .await
        .map_err(BlockResolveError::RpcError)
}

pub async fn get_block_number_from_hash_with_rpc(
    rpc_client: &RpcClient,
    hash: &str,
) -> Result<u64, BlockResolveError> {
    let header_json = get_header_json_with_rpc(rpc_client, hash).await?;

    if header_json.is_null() {
        return Err(BlockResolveError::NotFound(format!(
            "Block with hash {} not found",
            hash
        )));
    }

    let number_hex = header_json
        .get("number")
        .and_then(|v| v.as_str())
        .ok_or(BlockResolveError::BlockNumberNotFound)?;

    let number = u64::from_str_radix(number_hex.trim_start_matches("0x"), 16)
        .map_err(BlockResolveError::BlockNumberParseFailed)?;

    Ok(number)
}

pub async fn resolve_block_with_rpc(
    rpc_client: &RpcClient,
    legacy_rpc: &SubstrateLegacyRpc,
    at: Option<BlockId>,
) -> Result<ResolvedBlock, BlockResolveError> {
    match at {
        None => {
            let hash = legacy_rpc
                .chain_get_finalized_head()
                .await
                .map_err(BlockResolveError::FinalizedHeadFailed)?;
            let hash_str = format!("{:#x}", hash);
            let number = get_block_number_from_hash_with_rpc(rpc_client, &hash_str).await?;
            Ok(ResolvedBlock {
                hash: hash_str,
                number,
            })
        }
        Some(BlockId::Hash(hash)) => {
            let hash_str = format!("{:#x}", hash);
            let number = get_block_number_from_hash_with_rpc(rpc_client, &hash_str).await?;
            Ok(ResolvedBlock {
                hash: hash_str,
                number,
            })
        }
        Some(BlockId::Number(number)) => {
            let hash: Option<String> = rpc_client
                .request("chain_getBlockHash", rpc_params![number])
                .await
                .map_err(BlockResolveError::BlockHashFailed)?;
            let hash = hash.ok_or_else(|| {
                BlockResolveError::NotFound(format!("Block at height {} not found", number))
            })?;
            Ok(ResolvedBlock { hash, number })
        }
    }
}

/// Resolves a block from an optional block identifier
///
/// # Arguments
/// * `state` - Application state containing RPC client
/// * `at` - Optional block identifier (hash or number)
///
/// # Returns
/// * `ResolvedBlock` containing both hash and number
///
/// # Behavior
/// - If `at` is `None`, returns the latest finalized block
/// - If `at` is `BlockId::Hash`, fetches the block number for that hash
/// - If `at` is `BlockId::Number`, fetches the block hash for that number
pub async fn resolve_block(
    state: &AppState,
    at: Option<BlockId>,
) -> Result<ResolvedBlock, BlockResolveError> {
    match at {
        None => {
            // Get latest finalized block header hash
            let hash = state
                .legacy_rpc
                .chain_get_finalized_head()
                .await
                .map_err(BlockResolveError::FinalizedHeadFailed)?;

            let hash_str = format!("{:#x}", hash);
            let number = get_block_number_from_hash(state, &hash_str).await?;

            Ok(ResolvedBlock {
                hash: hash_str,
                number,
            })
        }
        Some(BlockId::Hash(hash)) => {
            // Convert H256 to hex string for RPC call
            let hash_str = format!("{:#x}", hash);

            // Fetch block number by hash
            let number = get_block_number_from_hash(state, &hash_str).await?;

            Ok(ResolvedBlock {
                hash: hash_str,
                number,
            })
        }
        Some(BlockId::Number(number)) => {
            // Fetch block hash by number
            let hash = state
                .get_block_hash_at_number(number)
                .await
                .map_err(BlockResolveError::BlockHashFailed)?
                .ok_or_else(|| {
                    BlockResolveError::NotFound(format!("Block at height {} not found", number))
                })?;

            Ok(ResolvedBlock { hash, number })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_types::H256;
    use std::str::FromStr;

    // ===== BlockId::from_str tests =====

    #[test]
    fn test_blockid_parse_valid_hash() {
        let hash_str = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let result = BlockId::from_str(hash_str);

        assert!(result.is_ok());
        match result.unwrap() {
            BlockId::Hash(h) => {
                assert_eq!(format!("{:#x}", h), hash_str);
            }
            BlockId::Number(_) => panic!("Expected Hash variant"),
        }
    }

    #[test]
    fn test_blockid_parse_valid_number() {
        let result = BlockId::from_str("12345");

        assert!(result.is_ok());
        match result.unwrap() {
            BlockId::Number(n) => assert_eq!(n, 12345),
            BlockId::Hash(_) => panic!("Expected Number variant"),
        }
    }

    #[test]
    fn test_blockid_parse_zero() {
        let result = BlockId::from_str("0");

        assert!(result.is_ok());
        match result.unwrap() {
            BlockId::Number(n) => assert_eq!(n, 0),
            BlockId::Hash(_) => panic!("Expected Number variant"),
        }
    }

    #[test]
    fn test_blockid_parse_invalid_hash_too_short() {
        let hash_str = "0x1234"; // Too short
        let result = BlockId::from_str(hash_str);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockIdParseError::InvalidHash(_)
        ));
    }

    #[test]
    fn test_blockid_parse_invalid_hash_too_long() {
        let hash_str = "0x12345678901234567890123456789012345678901234567890123456789012345"; // Too long (65 hex chars)
        let result = BlockId::from_str(hash_str);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockIdParseError::InvalidHash(_)
        ));
    }

    #[test]
    fn test_blockid_parse_invalid_hash_non_hex() {
        let hash_str = "0xGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG";
        let result = BlockId::from_str(hash_str);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockIdParseError::InvalidHash(_)
        ));
    }

    #[test]
    fn test_blockid_parse_invalid_number() {
        let result = BlockId::from_str("not_a_number");

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockIdParseError::InvalidNumber(_)
        ));
    }

    #[test]
    fn test_blockid_parse_invalid_negative_number() {
        let result = BlockId::from_str("-123");

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockIdParseError::InvalidNumber(_)
        ));
    }

    #[test]
    fn test_blockid_parse_empty_string() {
        let result = BlockId::from_str("");

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockIdParseError::InvalidNumber(_)
        ));
    }

    // ===== resolve_block tests =====

    use crate::state::AppState;
    use config::SidecarConfig;
    use serde_json::json;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    /// Helper to create a test AppState with mocked RPC responses
    ///
    /// Note: This helper creates an AppState with a placeholder OnlineClient.
    /// Tests using this helper should only exercise code paths that don't
    /// require the OnlineClient (e.g., RPC-based resolution paths).
    async fn create_test_state_with_mock(mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let rpc_client = Arc::new(RpcClient::new(mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: config::ChainType::Relay,
            spec_name: "test".to_string(),
            spec_version: 1,
            ss58_prefix: 42,
        };

        // Note: Creating an OnlineClient requires metadata from the node.
        // For tests, we attempt to create one but tests should be designed
        // to not rely on OnlineClient functionality when using mocks.
        let client = subxt::OnlineClient::from_rpc_client((*rpc_client).clone())
            .await
            .expect("Failed to create test OnlineClient - ensure mock provides required metadata");

        AppState {
            config,
            client: Arc::new(client),
            legacy_rpc,
            rpc_client,
            chain_info,
            relay_client: None,
            relay_rpc_client: None,
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: crate::routes::RouteRegistry::new(),
            relay_chain_rpc: None,
            lazy_relay_rpc: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    #[tokio::test]
    async fn test_resolve_block_finalized() {
        let mock_client = MockRpcClient::builder()
            .method_handler("rpc_methods", async |_params| {
                Json(json!({ "methods": [] }))
            })
            .method_handler("chain_getBlockHash", async |_params| {
                Json("0x0000000000000000000000000000000000000000000000000000000000000000")
            })
            .method_handler("chain_getFinalizedHead", async |_params| {
                Json("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("chain_getHeader", async |_params| {
                Json(json!({
                    "number": "0x2a", // Block 42
                    "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "stateRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "extrinsicsRoot": "0x0000000000000000000000000000000000000000000000000000000000000000"
                }))
            })
            .build();

        let state = create_test_state_with_mock(mock_client).await;

        let result = resolve_block(&state, None).await;
        assert!(result.is_ok());

        let resolved = result.unwrap();
        assert_eq!(resolved.number, 42);
        assert!(resolved.hash.starts_with("0x"));
    }

    #[tokio::test]
    async fn test_resolve_block_by_hash() {
        let test_hash = "0xabcdef1234567890123456789012345678901234567890123456789012345678";

        let mock_client = MockRpcClient::builder()
            .method_handler("rpc_methods", async |_params| {
                Json(json!({ "methods": [] }))
            })
            .method_handler("chain_getBlockHash", async |_params| {
                Json("0x0000000000000000000000000000000000000000000000000000000000000000")
            })
            .method_handler("chain_getHeader", async |_params| {
                Json(json!({
                    "number": "0x64", // Block 100
                    "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "stateRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "extrinsicsRoot": "0x0000000000000000000000000000000000000000000000000000000000000000"
                }))
            })
            .build();

        let state = create_test_state_with_mock(mock_client).await;

        let block_id = BlockId::Hash(H256::from_str(test_hash).unwrap());
        let result = resolve_block(&state, Some(block_id)).await;

        assert!(result.is_ok());

        let resolved = result.unwrap();
        assert_eq!(resolved.number, 100);
        assert_eq!(resolved.hash, test_hash);
    }

    #[tokio::test]
    async fn test_resolve_block_by_number() {
        let test_number = 200u64;
        let expected_hash = "0x9876543210987654321098765432109876543210987654321098765432109876";

        let mock_client = MockRpcClient::builder()
            .method_handler("rpc_methods", async |_params| {
                Json(json!({ "methods": [] }))
            })
            .method_handler("chain_getBlockHash", async |_params| {
                // Just return test hash - OnlineClient init uses different code path
                Json("0x9876543210987654321098765432109876543210987654321098765432109876")
            })
            .build();

        let state = create_test_state_with_mock(mock_client).await;

        let block_id = BlockId::Number(test_number);
        let result = resolve_block(&state, Some(block_id)).await;

        assert!(result.is_ok());

        let resolved = result.unwrap();
        assert_eq!(resolved.number, test_number);
        assert_eq!(resolved.hash, expected_hash);
    }

    #[tokio::test]
    async fn test_resolve_block_hash_not_found() {
        let test_hash = "0xabcdef1234567890123456789012345678901234567890123456789012345678";

        let mock_client = MockRpcClient::builder()
            .method_handler("rpc_methods", async |_params| {
                Json(json!({ "methods": [] }))
            })
            .method_handler("chain_getBlockHash", async |_params| {
                Json("0x0000000000000000000000000000000000000000000000000000000000000000")
            })
            .method_handler("chain_getHeader", async |_params| {
                Json(json!(null)) // Block doesn't exist
            })
            .build();

        let state = create_test_state_with_mock(mock_client).await;

        let block_id = BlockId::Hash(H256::from_str(test_hash).unwrap());
        let result = resolve_block(&state, Some(block_id)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockResolveError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_resolve_block_number_not_found() {
        use crate::test_fixtures::mock_rpc_client_builder;
        use serde_json::value::RawValue;

        let test_number = 999999u64;

        // Use test fixtures as base, but override chain_getBlockHash to return null for test_number
        let mock_client = mock_rpc_client_builder()
            .method_handler(
                "chain_getBlockHash",
                move |params: Option<Box<RawValue>>| async move {
                    // Parse the block number from params
                    let block_num = params
                        .and_then(|p| serde_json::from_str::<serde_json::Value>(p.get()).ok())
                        .and_then(|v| v.get(0).and_then(|n| n.as_u64()));

                    match block_num {
                        // Return valid hash for genesis (block 0) - needed for OnlineClient init
                        Some(0) | None => Json(json!(
                            "0x0000000000000000000000000000000000000000000000000000000000000000"
                        )),
                        // Return null for test block number - not found
                        Some(999999) => Json(json!(null)),
                        // Return valid hash for other blocks
                        _ => Json(json!(
                            "0x1234567890123456789012345678901234567890123456789012345678901234"
                        )),
                    }
                },
            )
            .build();

        let state = create_test_state_with_mock(mock_client).await;

        let block_id = BlockId::Number(test_number);
        let result = resolve_block(&state, Some(block_id)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlockResolveError::NotFound(_)
        ));
    }
}
