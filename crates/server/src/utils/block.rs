use crate::state::AppState;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BlockResolveError {
    #[error("Invalid block parameter: {0}")]
    InvalidParam(String),

    #[error("Block not found: {0}")]
    NotFound(String),

    #[error("Failed to get finalized head: {0}")]
    FinalizedHeadFailed(String),

    #[error("Failed to get block hash: {0}")]
    BlockHashFailed(String),

    #[error("Failed to get block header: {0}")]
    BlockHeaderFailed(String),

    #[error("Block number not found in header")]
    BlockNumberNotFound,

    #[error("Failed to parse block number from header: {0}")]
    BlockNumberParseFailed(String),

    #[error("RPC error: {0}")]
    RpcError(String),
}

/// Represents a resolved block with both hash and number
#[derive(Debug, Clone)]
pub struct ResolvedBlock {
    /// Block hash as hex string (with 0x prefix)
    pub hash: String,
    /// Block number
    pub number: u64,
}

/// Resolves a block from an optional "at" parameter
///
/// # Arguments
/// * `state` - Application state containing RPC client
/// * `at` - Optional block identifier (hash as hex string, or number as string)
///
/// # Returns
/// * `ResolvedBlock` containing both hash and number
///
/// # Behavior
/// - If `at` is `None`, returns the latest finalized block
/// - If `at` starts with "0x", treats it as a block hash and fetches the block number
/// - Otherwise, treats it as a block number and fetches the block hash
pub async fn resolve_block(
    state: &AppState,
    at: Option<String>,
) -> Result<ResolvedBlock, BlockResolveError> {
    match at {
        None => {
            // Get latest finalized block header hash
            let hash = state
                .legacy_rpc
                .chain_get_finalized_head()
                .await
                .map_err(|e| BlockResolveError::FinalizedHeadFailed(e.to_string()))?;

            let hash_str = format!("{:?}", hash);

            // Make raw RPC call to get the header data as JSON
            // We need to use raw JSON because subxt-historic's RpcConfig has Header = ()
            let header_json = state
                .get_header_json(&hash_str)
                .await
                .map_err(|e| BlockResolveError::RpcError(e.to_string()))?;

            // Extract block number from the header JSON
            // The response structure is: { "number": "0x..." }
            let number_hex = header_json
                .get("number")
                .and_then(|v| v.as_str())
                .ok_or(BlockResolveError::BlockNumberNotFound)?;

            // Parse hex string to u64 (remove 0x prefix)
            let number = u64::from_str_radix(number_hex.trim_start_matches("0x"), 16)
                .map_err(|e| BlockResolveError::BlockNumberParseFailed(e.to_string()))?;

            Ok(ResolvedBlock {
                hash: hash_str,
                number,
            })
        }
        Some(param) if param.starts_with("0x") => {
            // Treat as block hash
            // TODO: Parse the hex string to the correct hash type
            // TODO: Call chain_get_header with the parsed hash
            // For now, return placeholder
            let number = 0; // TODO: Get actual block number from header

            Ok(ResolvedBlock {
                hash: param,
                number,
            })
        }
        Some(param) => {
            // Treat as block number
            let number = param
                .parse::<u64>()
                .map_err(|_| BlockResolveError::InvalidParam(param.clone()))?;

            // TODO: Get block hash at this number
            // Need to use chain_get_block_hash(Some(number))
            let hash =
                "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(); // TODO: Get actual block hash

            Ok(ResolvedBlock { hash, number })
        }
    }
}
