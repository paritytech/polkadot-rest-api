//! Handler for GET /rc/blocks/{blockId}/extrinsics-raw endpoint.
//!
//! This module provides a handler for fetching raw block data with hex-encoded extrinsics
//! from the relay chain. Unlike the main /blocks/{blockId}/extrinsics-raw endpoint,
//! this queries the relay chain connection instead of the primary chain.

use crate::handlers::blocks::get_block_extrinsics_raw::{BlockRawDigest, BlockRawResponse};
use crate::handlers::blocks::types::{DigestItemDiscriminant, DigestLog, GetBlockError};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Handler for GET /rc/blocks/{blockId}/extrinsics-raw
///
/// Returns raw block data with hex-encoded extrinsics for a given block identifier (hash or number)
/// from the relay chain. The extrinsics are returned as raw hex strings without decoding.
///
/// # Path Parameters
/// - `blockId`: Block identifier (height number or block hash)
pub async fn get_rc_block_extrinsics_raw(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
) -> Result<Response, GetBlockError> {
    let response: BlockRawResponse = build_rc_block_raw_response(&state, block_id).await?;
    Ok(Json(response).into_response())
}

async fn build_rc_block_raw_response(
    state: &AppState,
    block_id: String,
) -> Result<BlockRawResponse, GetBlockError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or_else(|| GetBlockError::RelayChainNotConfigured)?;

    let block_id_parsed = block_id.parse::<utils::BlockId>()?;

    let client_at_block = match &block_id_parsed {
        utils::BlockId::Number(n) => relay_client.at_block(*n).await?,
        utils::BlockId::Hash(h) => relay_client.at_block(*h).await?,
    };

    let (header, extrinsics) = tokio::try_join!(
        async {
            client_at_block.block_header().await.map_err(|e| {
                GetBlockError::ExtrinsicsFetchFailed(format!("Header fetch failed: {}", e))
            })
        },
        async {
            client_at_block
                .extrinsics()
                .fetch()
                .await
                .map_err(|e| GetBlockError::ExtrinsicsFetchFailed(e.to_string()))
        }
    )?;

    let raw_extrinsics: Vec<String> = extrinsics
        .iter()
        .filter_map(|ext_result| {
            ext_result
                .ok()
                .map(|ext| format!("0x{}", hex::encode(ext.bytes())))
        })
        .collect();

    let parent_hash = format!("{:#x}", header.parent_hash);
    let number = format!("0x{:08x}", header.number);
    let state_root = format!("{:#x}", header.state_root);
    let extrinsic_root = format!("{:#x}", header.extrinsics_root);

    let logs = decode_digest_logs_from_header(&header);

    Ok(BlockRawResponse {
        parent_hash,
        number,
        state_root,
        extrinsic_root,
        digest: BlockRawDigest { logs },
        extrinsics: raw_extrinsics,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

fn decode_digest_logs_from_header(
    header: &subxt::config::substrate::SubstrateHeader<subxt::utils::H256>,
) -> Vec<DigestLog> {
    header
        .digest
        .logs
        .iter()
        .map(|log| {
            use subxt::config::substrate::DigestItem;

            match log {
                DigestItem::PreRuntime(engine_id, data) => DigestLog {
                    log_type: DigestItemDiscriminant::PreRuntime.as_str().to_string(),
                    index: (DigestItemDiscriminant::PreRuntime as u8).to_string(),
                    value: json!([
                        format!("0x{}", hex::encode(engine_id)),
                        format!("0x{}", hex::encode(data))
                    ]),
                },
                DigestItem::Consensus(engine_id, data) => DigestLog {
                    log_type: DigestItemDiscriminant::Consensus.as_str().to_string(),
                    index: (DigestItemDiscriminant::Consensus as u8).to_string(),
                    value: json!([
                        format!("0x{}", hex::encode(engine_id)),
                        format!("0x{}", hex::encode(data))
                    ]),
                },
                DigestItem::Seal(engine_id, data) => DigestLog {
                    log_type: DigestItemDiscriminant::Seal.as_str().to_string(),
                    index: (DigestItemDiscriminant::Seal as u8).to_string(),
                    value: json!([
                        format!("0x{}", hex::encode(engine_id)),
                        format!("0x{}", hex::encode(data))
                    ]),
                },
                DigestItem::RuntimeEnvironmentUpdated => DigestLog {
                    log_type: DigestItemDiscriminant::RuntimeEnvironmentUpdated
                        .as_str()
                        .to_string(),
                    index: (DigestItemDiscriminant::RuntimeEnvironmentUpdated as u8).to_string(),
                    value: serde_json::Value::Null,
                },
                DigestItem::Other(data) => DigestLog {
                    log_type: DigestItemDiscriminant::Other.as_str().to_string(),
                    index: (DigestItemDiscriminant::Other as u8).to_string(),
                    value: json!(format!("0x{}", hex::encode(data))),
                },
            }
        })
        .collect()
}
