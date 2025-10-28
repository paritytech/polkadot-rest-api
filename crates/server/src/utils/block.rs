use crate::state::AppState;
use primitive_types::H256;
use std::str::FromStr;
use thiserror::Error;

/// Represents a block identifier that can be either a hash or a number
#[derive(Debug, Clone)]
pub enum BlockId {
    /// Block hash (32 bytes)
    Hash(H256),
    /// Block number
    Number(u64),
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

            let hash_str = format!("{:?}", hash);
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
