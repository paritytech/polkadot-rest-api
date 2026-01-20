use crate::state::AppState;
use crate::utils::{self, RcBlockError, ResolvedBlock, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use futures::stream::{self, StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use subxt_historic::error::OnlineClientAtBlockError;

use super::get_block::build_block_response_for_hash;
use super::types::{BlockQueryParams, BlockResponse, GetBlockError};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlocksRangeQueryParams {
    pub range: Option<String>,
    #[serde(default)]
    pub event_docs: bool,
    #[serde(default)]
    pub extrinsic_docs: bool,
    #[serde(default)]
    pub no_fees: bool,
    #[serde(default)]
    pub use_rc_block: bool,
    #[serde(default)]
    pub use_evm_format: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum GetBlocksError {
    #[error("range query parameter must be inputted.")]
    MissingRange,

    #[error("Incorrect range format. Expected example: 0-999")]
    InvalidRangeFormat,

    #[error("Inputted min value for range must be an unsigned integer.")]
    InvalidRangeMin,

    #[error("Inputted max value for range must be an unsigned non zero integer.")]
    InvalidRangeMax,

    #[error("Inputted min value cannot be greater than or equal to the max value.")]
    InvalidRangeMinMax,

    #[error("Inputted range is greater than the 500 range limit.")]
    RangeTooLarge,

    #[error("useRcBlock parameter is only supported for Asset Hub endpoints")]
    UseRcBlockNotSupported,

    #[error(
        "useRcBlock parameter requires relay chain API to be available. Please configure SAS_SUBSTRATE_MULTI_CHAIN_URL"
    )]
    RelayChainNotConfigured,

    #[error("Failed to find Asset Hub blocks in Relay Chain block")]
    RcBlockError(#[from] RcBlockError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[from] OnlineClientAtBlockError),

    #[error(transparent)]
    BlockError(#[from] GetBlockError),
}

impl IntoResponse for GetBlocksError {
    fn into_response(self) -> Response {
        match self {
            GetBlocksError::MissingRange
            | GetBlocksError::InvalidRangeFormat
            | GetBlocksError::InvalidRangeMin
            | GetBlocksError::InvalidRangeMax
            | GetBlocksError::InvalidRangeMinMax
            | GetBlocksError::RangeTooLarge
            | GetBlocksError::UseRcBlockNotSupported
            | GetBlocksError::RelayChainNotConfigured => {
                let msg = self.to_string();
                let body = Json(json!({ "error": msg }));
                (axum::http::StatusCode::BAD_REQUEST, body).into_response()
            }
            GetBlocksError::RcBlockError(_)
            | GetBlocksError::BlockResolveFailed(_)
            | GetBlocksError::ClientAtBlockFailed(_) => {
                let msg = self.to_string();
                let body = Json(json!({ "error": msg }));
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
            GetBlocksError::BlockError(err) => err.into_response(),
        }
    }
}

/// Handler for GET /blocks
///
/// Returns a collection of blocks given a numeric range.
///
/// Notes:
/// - Range is inclusive and limited to 500 blocks (matching Sidecar).
/// - Blocks are returned as an array of `BlockResponse`.
pub async fn get_blocks(
    State(state): State<AppState>,
    Query(params): Query<BlocksRangeQueryParams>,
) -> Result<Response, GetBlocksError> {
    let range_str = params.range.clone().ok_or(GetBlocksError::MissingRange)?;

    let (start, end) = parse_range(&range_str)?;

    let base_block_params = BlockQueryParams {
        event_docs: params.event_docs,
        extrinsic_docs: params.extrinsic_docs,
        no_fees: params.no_fees,
        ..BlockQueryParams::default()
    };

    if params.use_rc_block {
        let blocks =
            handle_use_rc_block_range(state.clone(), &base_block_params, start, end).await?;
        return Ok(Json(blocks).into_response());
    }

    let concurrency = state.config.express.block_fetch_concurrency;

    let blocks: Vec<BlockResponse> = stream::iter(start..=end)
        .map(|number| {
            let state = state.clone();
            let params = base_block_params.clone();
            async move {
                let block_hash = state
                    .get_block_hash_at_number(number)
                    .await
                    .map_err(GetBlockError::HeaderFetchFailed)?
                    .ok_or_else(|| {
                        GetBlocksError::BlockError(GetBlockError::BlockResolveFailed(
                            utils::BlockResolveError::NotFound(format!(
                                "Block at height {} not found",
                                number
                            )),
                        ))
                    })?;

                build_block_response_for_hash(&state, &block_hash, number, false, &params)
                    .await
                    .map_err(GetBlocksError::from)
            }
        })
        .buffered(concurrency)
        .try_collect()
        .await?;

    Ok(Json(blocks).into_response())
}

async fn handle_use_rc_block_range(
    state: AppState,
    params: &BlockQueryParams,
    start: u64,
    end: u64,
) -> Result<Vec<BlockResponse>, GetBlocksError> {
    use config::ChainType;
    use parity_scale_codec::Decode;

    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(GetBlocksError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(GetBlocksError::RelayChainNotConfigured);
    }

    let rc_rpc = state
        .get_relay_chain_rpc_client()
        .ok_or(GetBlocksError::RelayChainNotConfigured)?
        .clone();
    let rc_legacy_rpc = state
        .get_relay_chain_rpc()
        .ok_or(GetBlocksError::RelayChainNotConfigured)?
        .clone();

    let concurrency = state.config.express.block_fetch_concurrency;

    // Fetch RC blocks in parallel, each returning a Vec of AH BlockResponses
    let nested_results: Vec<Vec<BlockResponse>> = stream::iter(start..=end)
        .map(|rc_number| {
            let state = state.clone();
            let params = params.clone();
            let rc_rpc = rc_rpc.clone();
            let rc_legacy_rpc = rc_legacy_rpc.clone();
            async move {
                let rc_resolved_block =
                    resolve_rc_block(&rc_rpc, &rc_legacy_rpc, rc_number).await?;

                let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

                if ah_blocks.is_empty() {
                    return Ok::<Vec<BlockResponse>, GetBlocksError>(Vec::new());
                }

                let rc_block_number = rc_resolved_block.number.to_string();
                let rc_block_hash = rc_resolved_block.hash.clone();

                let mut responses = Vec::with_capacity(ah_blocks.len());
                for ah_block in ah_blocks {
                    let mut response = build_block_response_for_hash(
                        &state,
                        &ah_block.hash,
                        ah_block.number,
                        true,
                        &params,
                    )
                    .await?;

                    response.rc_block_hash = Some(rc_block_hash.clone());
                    response.rc_block_number = Some(rc_block_number.clone());

                    let client_at_block = state.client.at(ah_block.number).await?;
                    if let Ok(timestamp_entry) = client_at_block.storage().entry("Timestamp", "Now")
                        && let Ok(Some(timestamp)) = timestamp_entry.fetch(()).await
                    {
                        let timestamp_bytes = timestamp.into_bytes();
                        let mut cursor = &timestamp_bytes[..];
                        if let Ok(timestamp_value) = u64::decode(&mut cursor) {
                            response.ah_timestamp = Some(timestamp_value.to_string());
                        }
                    }

                    responses.push(response);
                }

                Ok(responses)
            }
        })
        .buffered(concurrency)
        .try_collect()
        .await?;

    // Flatten and sort results
    let mut results: Vec<BlockResponse> = nested_results.into_iter().flatten().collect();

    results.sort_by(|a, b| {
        let a_num = a.number.parse::<u64>().unwrap_or_default();
        let b_num = b.number.parse::<u64>().unwrap_or_default();
        a_num.cmp(&b_num)
    });

    Ok(results)
}

async fn resolve_rc_block(
    rc_rpc_client: &Arc<subxt_rpcs::RpcClient>,
    rc_legacy_rpc: &Arc<subxt_rpcs::LegacyRpcMethods<subxt_historic::SubstrateConfig>>,
    rc_number: u64,
) -> Result<ResolvedBlock, GetBlocksError> {
    let block_id = utils::BlockId::Number(rc_number);
    let resolved =
        utils::resolve_block_with_rpc(rc_rpc_client, rc_legacy_rpc, Some(block_id)).await?;
    Ok(resolved)
}

fn parse_range(range: &str) -> Result<(u64, u64), GetBlocksError> {
    let parts: Vec<_> = range.split('-').collect();
    if parts.len() != 2 {
        return Err(GetBlocksError::InvalidRangeFormat);
    }

    let start_str = parts[0].trim();
    let end_str = parts[1].trim();

    if start_str.is_empty() || end_str.is_empty() {
        return Err(GetBlocksError::InvalidRangeFormat);
    }

    let start: u64 = start_str
        .parse()
        .map_err(|_| GetBlocksError::InvalidRangeMin)?;
    let end: u64 = end_str
        .parse()
        .map_err(|_| GetBlocksError::InvalidRangeMax)?;

    if start > end {
        return Err(GetBlocksError::InvalidRangeMinMax);
    }

    let count = end
        .checked_sub(start)
        .and_then(|d| d.checked_add(1))
        .ok_or(GetBlocksError::RangeTooLarge)?;

    if count > 500 {
        return Err(GetBlocksError::RangeTooLarge);
    }

    Ok((start, end))
}
