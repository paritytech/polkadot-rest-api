use crate::state::AppState;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BlockResolveError {
    #[error("Invalid block parameter: {0}")]
    InvalidParam(String),

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
                .map_err(BlockResolveError::FinalizedHeadFailed)?;

            let hash_str = format!("{:?}", hash);
            let number = get_block_number_from_hash(state, &hash_str).await?;

            Ok(ResolvedBlock {
                hash: hash_str,
                number,
            })
        }
        Some(param) if param.starts_with("0x") => {
            // Treat as block hash
            let number = get_block_number_from_hash(state, &param).await?;

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

            // Get block hash at this number
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
