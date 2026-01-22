use crate::handlers::blocks::common::{
    BlockClient, add_docs_to_events, decode_digest_logs, get_validators_at_block,
};
use crate::handlers::blocks::docs::Docs;
use crate::handlers::blocks::processing::{
    categorize_events, extract_extrinsics_with_prefix, extract_fee_info_for_extrinsic_with_client,
    fetch_block_events_with_prefix,
};
use crate::handlers::blocks::types::{
    BlockQueryParams, BlockResponse, DigestLog, GetBlockError,
};
use crate::state::AppState;
use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use futures::stream::{self, StreamExt, TryStreamExt};
use heck::{ToSnakeCase, ToUpperCamelCase};
use parity_scale_codec::Decode;
use serde::Deserialize;
use serde_json::json;
use sp_consensus_babe::digests::PreDigest;
use sp_core::crypto::Ss58Codec;
use std::sync::Arc;
use subxt::error::OnlineClientAtBlockError;
use subxt_rpcs::{RpcClient, rpc_params};

/// Query parameters for GET /rc/blocks endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlocksRangeQueryParams {
    /// Range of blocks to fetch
    pub range: Option<String>,
    /// Include documentation for events
    #[serde(default)]
    pub event_docs: bool,
    /// Include documentation for extrinsics
    #[serde(default)]
    pub extrinsic_docs: bool,
    /// Skip fee calculation
    #[serde(default)]
    pub no_fees: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum GetRcBlocksError {
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

    #[error(
        "Relay chain API is not available. Please configure SAS_SUBSTRATE_MULTI_CHAIN_URL"
    )]
    RelayChainNotConfigured,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[source] Box<OnlineClientAtBlockError>),

    #[error("Failed to get block header")]
    HeaderFetchFailed(#[source] subxt_rpcs::Error),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),

    #[error(transparent)]
    BlockError(#[from] GetBlockError),
}

impl IntoResponse for GetRcBlocksError {
    fn into_response(self) -> Response {
        match self {
            GetRcBlocksError::MissingRange
            | GetRcBlocksError::InvalidRangeFormat
            | GetRcBlocksError::InvalidRangeMin
            | GetRcBlocksError::InvalidRangeMax
            | GetRcBlocksError::InvalidRangeMinMax
            | GetRcBlocksError::RangeTooLarge
            | GetRcBlocksError::RelayChainNotConfigured => {
                let msg = self.to_string();
                let body = Json(json!({ "error": msg }));
                (axum::http::StatusCode::BAD_REQUEST, body).into_response()
            }
            GetRcBlocksError::ClientAtBlockFailed(_)
            | GetRcBlocksError::HeaderFetchFailed(_)
            | GetRcBlocksError::HeaderFieldMissing(_) => {
                let msg = self.to_string();
                let body = Json(json!({ "error": msg }));
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
            GetRcBlocksError::BlockError(err) => err.into_response(),
        }
    }
}

/// Handler for GET /rc/blocks
///
/// Returns a collection of Relay Chain blocks given a numeric range.
///
/// Query Parameters:
/// - `range` (required): Range of block numbers (e.g., "100-200"). Max 500 blocks.
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
/// - `noFees` (boolean, default: false): Skip fee calculation
pub async fn get_rc_blocks(
    State(state): State<AppState>,
    Query(params): Query<RcBlocksRangeQueryParams>,
) -> Result<Response, GetRcBlocksError> {
    let range_str = params.range.clone().ok_or(GetRcBlocksError::MissingRange)?;

    let (start, end) = parse_range(&range_str)?;

    let relay_client = state
        .get_relay_chain_client()
        .ok_or(GetRcBlocksError::RelayChainNotConfigured)?
        .clone();
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(GetRcBlocksError::RelayChainNotConfigured)?
        .clone();
    let relay_chain_rpc = state
        .get_relay_chain_rpc()
        .ok_or(GetRcBlocksError::RelayChainNotConfigured)?
        .clone();

    let relay_chain_info = state
        .relay_chain_info
        .clone()
        .ok_or(GetRcBlocksError::RelayChainNotConfigured)?;

    let base_params = BlockQueryParams {
        event_docs: params.event_docs,
        extrinsic_docs: params.extrinsic_docs,
        no_fees: params.no_fees,
        finalized_key: true,
        ..BlockQueryParams::default()
    };

    let finalized_block_number = get_relay_finalized_block_number(&relay_chain_rpc, &relay_rpc_client).await?;

    let concurrency = state.config.express.block_fetch_concurrency;

    let blocks: Vec<BlockResponse> = stream::iter(start..=end)
        .map(|number| {
            let relay_client = relay_client.clone();
            let relay_rpc_client = relay_rpc_client.clone();
            let relay_chain_info = relay_chain_info.clone();
            let params = base_params.clone();
            let state = state.clone();
            async move {
                let client_at_block = relay_client
                    .at_block(number)
                    .await
                    .map_err(|e| GetRcBlocksError::ClientAtBlockFailed(Box::new(e)))?;

                let block_hash = format!("{:#x}", client_at_block.block_hash());

                let is_finalized = number <= finalized_block_number;

                build_rc_block_response(
                    &state,
                    &relay_rpc_client,
                    &relay_chain_info,
                    &block_hash,
                    number,
                    &client_at_block,
                    &params,
                    is_finalized,
                )
                .await
            }
        })
        .buffered(concurrency)
        .try_collect()
        .await?;

    Ok(Json(blocks).into_response())
}

async fn get_relay_finalized_block_number(
    relay_chain_rpc: &Arc<crate::state::SubstrateLegacyRpc>,
    relay_rpc_client: &Arc<RpcClient>,
) -> Result<u64, GetRcBlocksError> {
    let finalized_hash = relay_chain_rpc
        .chain_get_finalized_head()
        .await
        .map_err(|e| GetRcBlocksError::BlockError(GetBlockError::FinalizedHeadFailed(e)))?;

    let finalized_hash_str = format!("0x{}", hex::encode(finalized_hash.0));

    let header_json: serde_json::Value = relay_rpc_client
        .request("chain_getHeader", rpc_params![finalized_hash_str])
        .await
        .map_err(GetRcBlocksError::HeaderFetchFailed)?;

    let number_hex = header_json
        .get("number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetRcBlocksError::HeaderFieldMissing("number".to_string()))?;

    let number = u64::from_str_radix(number_hex.trim_start_matches("0x"), 16)
        .map_err(|_| GetRcBlocksError::HeaderFieldMissing("number (invalid format)".to_string()))?;

    Ok(number)
}

async fn build_rc_block_response(
    state: &AppState,
    relay_rpc_client: &Arc<RpcClient>,
    relay_chain_info: &crate::state::ChainInfo,
    block_hash: &str,
    block_number: u64,
    client_at_block: &BlockClient,
    params: &BlockQueryParams,
    is_finalized: bool,
) -> Result<BlockResponse, GetRcBlocksError> {
    let ss58_prefix = relay_chain_info.ss58_prefix;

    let header_json: serde_json::Value = relay_rpc_client
        .request("chain_getHeader", rpc_params![block_hash])
        .await
        .map_err(GetRcBlocksError::HeaderFetchFailed)?;

    let parent_hash = header_json
        .get("parentHash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetRcBlocksError::HeaderFieldMissing("parentHash".to_string()))?
        .to_string();

    let state_root = header_json
        .get("stateRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetRcBlocksError::HeaderFieldMissing("stateRoot".to_string()))?
        .to_string();

    let extrinsics_root = header_json
        .get("extrinsicsRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetRcBlocksError::HeaderFieldMissing("extrinsicsRoot".to_string()))?
        .to_string();

    let logs = decode_digest_logs(&header_json);

    let author_id =
        extract_author_for_relay(relay_chain_info, client_at_block, &logs, block_number).await;

    let (extrinsics_result, events_result) = tokio::join!(
        extract_extrinsics_with_prefix(ss58_prefix, client_at_block, block_number),
        fetch_block_events_with_prefix(ss58_prefix, client_at_block, block_number),
    );

    let extrinsics = extrinsics_result.map_err(GetRcBlocksError::BlockError)?;
    let block_events = events_result.map_err(GetRcBlocksError::BlockError)?;

    let (on_initialize, per_extrinsic_events, on_finalize, extrinsic_outcomes) =
        categorize_events(block_events, extrinsics.len());

    let mut extrinsics_with_events = extrinsics;
    for (i, (extrinsic_events, outcome)) in per_extrinsic_events
        .iter()
        .zip(extrinsic_outcomes.iter())
        .enumerate()
    {
        if let Some(extrinsic) = extrinsics_with_events.get_mut(i) {
            extrinsic.events = extrinsic_events.clone();
            extrinsic.success = outcome.success;
            if extrinsic.signature.is_some() && outcome.pays_fee.is_some() {
                extrinsic.pays_fee = outcome.pays_fee;
            }
        }
    }

    if !params.no_fees {
        let fee_indices: Vec<usize> = extrinsics_with_events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.signature.is_some() && e.pays_fee == Some(true))
            .map(|(i, _)| i)
            .collect();

        if !fee_indices.is_empty() {
            let spec_version: serde_json::Value = relay_rpc_client
                .request("state_getRuntimeVersion", rpc_params![block_hash])
                .await
                .map_err(|e| GetRcBlocksError::BlockError(GetBlockError::RuntimeVersionFailed(e)))?;

            let spec_version = spec_version
                .get("specVersion")
                .and_then(|sv| sv.as_u64())
                .map(|v| v as u32)
                .ok_or_else(|| GetRcBlocksError::HeaderFieldMissing("specVersion".to_string()))?;

            let fee_futures: Vec<_> = fee_indices
                .iter()
                .map(|&i| {
                    let extrinsic = &extrinsics_with_events[i];
                    extract_fee_info_for_extrinsic_with_client(
                        state,
                        relay_rpc_client,
                        &extrinsic.raw_hex,
                        &extrinsic.events,
                        extrinsic_outcomes.get(i),
                        &parent_hash,
                        spec_version,
                    )
                })
                .collect();

            let fee_results = futures::future::join_all(fee_futures).await;

            for (idx, fee_info) in fee_indices.into_iter().zip(fee_results.into_iter()) {
                extrinsics_with_events[idx].info = fee_info;
            }
        }
    }

    let (mut on_initialize, mut on_finalize) = (on_initialize, on_finalize);

    if params.event_docs || params.extrinsic_docs {
        let metadata = client_at_block.metadata();

        if params.event_docs {
            add_docs_to_events(&mut on_initialize.events, &metadata);
            add_docs_to_events(&mut on_finalize.events, &metadata);

            for extrinsic in extrinsics_with_events.iter_mut() {
                add_docs_to_events(&mut extrinsic.events, &metadata);
            }
        }

        if params.extrinsic_docs {
            for extrinsic in extrinsics_with_events.iter_mut() {
                let pallet_name = extrinsic.method.pallet.to_upper_camel_case();
                let method_name = extrinsic.method.method.to_snake_case();
                extrinsic.docs = Docs::for_call_subxt(&metadata, &pallet_name, &method_name)
                    .map(|d| d.to_string());
            }
        }
    }

    Ok(BlockResponse {
        number: block_number.to_string(),
        hash: block_hash.to_string(),
        parent_hash,
        state_root,
        extrinsics_root,
        author_id,
        logs,
        on_initialize,
        extrinsics: extrinsics_with_events,
        on_finalize,
        finalized: Some(is_finalized),
        decoded_xcm_msgs: None,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

/// Extract author from relay chain block's digest logs
async fn extract_author_for_relay(
    relay_chain_info: &crate::state::ChainInfo,
    client_at_block: &BlockClient,
    logs: &[DigestLog],
    block_number: u64,
) -> Option<String> {
    const BABE_ENGINE: &[u8] = b"BABE";
    const AURA_ENGINE: &[u8] = b"aura";

    let validators = match get_validators_at_block(client_at_block).await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Failed to get validators for block {}: {}", block_number, e);
            return None;
        }
    };

    for log in logs {
        if log.log_type == "PreRuntime"
            && let Some(arr) = log.value.as_array()
            && arr.len() >= 2
        {
            let engine_id_hex = arr[0].as_str()?;
            let payload_hex = arr[1].as_str()?;
            let payload = hex::decode(payload_hex.strip_prefix("0x")?).ok()?;

            let engine_id_bytes = hex::decode(engine_id_hex.strip_prefix("0x")?).ok()?;

            match engine_id_bytes.as_slice() {
                BABE_ENGINE => {
                    if payload.is_empty() {
                        continue;
                    }

                    let mut cursor = &payload[..];
                    let pre_digest = PreDigest::decode(&mut cursor).ok()?;
                    let authority_index = pre_digest.authority_index() as usize;
                    let author = validators.get(authority_index)?;

                    return Some(
                        author
                            .clone()
                            .to_ss58check_with_version(relay_chain_info.ss58_prefix.into()),
                    );
                }
                AURA_ENGINE => {
                    if payload.len() >= 8 {
                        let slot = u64::from_le_bytes([
                            payload[0], payload[1], payload[2], payload[3], payload[4], payload[5],
                            payload[6], payload[7],
                        ]) as usize;

                        let index = slot % validators.len();
                        let author = validators.get(index)?;

                        return Some(
                            author
                                .clone()
                                .to_ss58check_with_version(relay_chain_info.ss58_prefix.into()),
                        );
                    }
                }
                _ => continue,
            }
        }
    }

    None
}

fn parse_range(range: &str) -> Result<(u64, u64), GetRcBlocksError> {
    let parts: Vec<_> = range.split('-').collect();
    if parts.len() != 2 {
        return Err(GetRcBlocksError::InvalidRangeFormat);
    }

    let start_str = parts[0].trim();
    let end_str = parts[1].trim();

    if start_str.is_empty() || end_str.is_empty() {
        return Err(GetRcBlocksError::InvalidRangeFormat);
    }

    let start: u64 = start_str
        .parse()
        .map_err(|_| GetRcBlocksError::InvalidRangeMin)?;
    let end: u64 = end_str
        .parse()
        .map_err(|_| GetRcBlocksError::InvalidRangeMax)?;

    if start >= end {
        return Err(GetRcBlocksError::InvalidRangeMinMax);
    }

    let count = end
        .checked_sub(start)
        .and_then(|d| d.checked_add(1))
        .ok_or(GetRcBlocksError::RangeTooLarge)?;

    if count > 500 {
        return Err(GetRcBlocksError::RangeTooLarge);
    }

    Ok((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range_valid() {
        assert_eq!(parse_range("0-10").unwrap(), (0, 10));
        assert_eq!(parse_range("100-200").unwrap(), (100, 200));
        assert_eq!(parse_range("0-499").unwrap(), (0, 499));
    }

    #[test]
    fn test_parse_range_max_limit() {
        assert_eq!(parse_range("0-499").unwrap(), (0, 499));
        assert!(matches!(
            parse_range("0-500"),
            Err(GetRcBlocksError::RangeTooLarge)
        ));
    }

    #[test]
    fn test_parse_range_invalid_format() {
        assert!(matches!(
            parse_range("10"),
            Err(GetRcBlocksError::InvalidRangeFormat)
        ));
        assert!(matches!(
            parse_range("10-"),
            Err(GetRcBlocksError::InvalidRangeFormat)
        ));
        assert!(matches!(
            parse_range("-10"),
            Err(GetRcBlocksError::InvalidRangeFormat)
        ));
        assert!(matches!(
            parse_range(""),
            Err(GetRcBlocksError::InvalidRangeFormat)
        ));
    }

    #[test]
    fn test_parse_range_invalid_values() {
        assert!(matches!(
            parse_range("a-b"),
            Err(GetRcBlocksError::InvalidRangeMin)
        ));
        assert!(matches!(
            parse_range("10-9"),
            Err(GetRcBlocksError::InvalidRangeMinMax)
        ));
        assert!(matches!(
            parse_range("10-10"),
            Err(GetRcBlocksError::InvalidRangeMinMax)
        ));
    }
}
