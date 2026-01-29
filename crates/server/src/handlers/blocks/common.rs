//! Common block processing logic shared by block-related handlers.
//!
//! This module contains foundational utilities for block data:
//! - `BlockClient` type alias for working with blocks at specific heights
//! - Common error types used across multiple block endpoints
//! - Digest log decoding (PreRuntime, Consensus, Seal)
//! - Chain state queries (canonical hash, finalized head, validators)
//! - Block author extraction from consensus digests
//! - Documentation helpers for events

use crate::state::AppState;
use crate::utils::{self, hex_with_prefix};
use axum::{Json, http::StatusCode, response::IntoResponse};
use heck::ToUpperCamelCase;
use parity_scale_codec::Decode;
use serde_json::{Value, json};
use sp_core::crypto::{AccountId32, Ss58Codec};
use std::sync::Arc;
use subxt::config::substrate::DigestItem;
use subxt::{OnlineClientAtBlock, SubstrateConfig, error::OnlineClientAtBlockError};
use subxt_rpcs::{RpcClient, rpc_params};
use thiserror::Error;

use super::docs::Docs;
use super::types::{DigestLog, Event, GetBlockError};

/// Type alias for the ClientAtBlock type used throughout the codebase.
/// This represents a client pinned to a specific block height with access to
/// storage, extrinsics, and metadata for that block.
pub type BlockClient = OnlineClientAtBlock<SubstrateConfig>;

// ================================================================================================
// Common Error Types
// ================================================================================================

/// Common errors that appear across multiple block-related endpoints.
///
/// This enum consolidates frequently repeated error variants to reduce code duplication.
/// Endpoint-specific error types can include these variants via composition or wrapping.
#[derive(Debug, Error)]
pub enum CommonBlockError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[source] Box<OnlineClientAtBlockError>),

    #[error("Failed to fetch storage: {0}")]
    StorageFetchFailed(String),

    #[error("Failed to decode events: {0}")]
    EventsDecodeFailed(String),
}

impl IntoResponse for CommonBlockError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            CommonBlockError::InvalidBlockParam(_) | CommonBlockError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            CommonBlockError::ClientAtBlockFailed(err) => {
                if utils::is_online_client_at_block_disconnected(err.as_ref()) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Service temporarily unavailable: {}", err),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }
            CommonBlockError::StorageFetchFailed(_) | CommonBlockError::EventsDecodeFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

// ================================================================================================
// Digest Processing
// ================================================================================================

pub fn convert_digest_items_to_logs(items: &[DigestItem]) -> Vec<DigestLog> {
    items
        .iter()
        .map(|item| match item {
            DigestItem::PreRuntime(engine_id, data) => DigestLog {
                log_type: "PreRuntime".to_string(),
                index: "6".to_string(),
                value: json!([hex_with_prefix(engine_id), hex_with_prefix(data)]),
            },
            DigestItem::Consensus(engine_id, data) => DigestLog {
                log_type: "Consensus".to_string(),
                index: "4".to_string(),
                value: json!([hex_with_prefix(engine_id), hex_with_prefix(data)]),
            },
            DigestItem::Seal(engine_id, data) => DigestLog {
                log_type: "Seal".to_string(),
                index: "5".to_string(),
                value: json!([hex_with_prefix(engine_id), hex_with_prefix(data)]),
            },
            DigestItem::RuntimeEnvironmentUpdated => DigestLog {
                log_type: "RuntimeEnvironmentUpdated".to_string(),
                index: "8".to_string(),
                value: Value::Null,
            },
            DigestItem::Other(data) => DigestLog {
                log_type: "Other".to_string(),
                index: "0".to_string(),
                value: json!(hex_with_prefix(data)),
            },
        })
        .collect()
}

// ================================================================================================
// Block Header & Chain State
// ================================================================================================

/// Fetch the canonical block hash at a given block number
/// This is used to verify that a queried block hash is on the canonical chain
pub async fn get_canonical_hash_at_number(
    state: &AppState,
    block_number: u64,
) -> Result<Option<String>, GetBlockError> {
    let hash = state
        .legacy_rpc
        .chain_get_block_hash(Some(block_number.into()))
        .await
        .map_err(GetBlockError::CanonicalHashFailed)?;

    Ok(hash.map(|h| format!("0x{}", hex::encode(h.0))))
}

/// Fetch the finalized block number from the chain
pub async fn get_finalized_block_number(state: &AppState) -> Result<u64, GetBlockError> {
    let finalized_hash = state
        .legacy_rpc
        .chain_get_finalized_head()
        .await
        .map_err(GetBlockError::FinalizedHeadFailed)?;
    let finalized_hash_str = format!("0x{}", hex::encode(finalized_hash.0));
    let header_json = state
        .get_header_json(&finalized_hash_str)
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;
    let number_hex = header_json
        .get("number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("number".to_string()))?;
    let number = u64::from_str_radix(number_hex.trim_start_matches("0x"), 16)
        .map_err(|_| GetBlockError::HeaderFieldMissing("number (invalid format)".to_string()))?;

    Ok(number)
}

/// Fetch validator set from chain state at a specific block
pub async fn get_validators_at_block(
    client_at_block: &BlockClient,
) -> Result<Vec<AccountId32>, GetBlockError> {
    // Use typed dynamic storage to decode as raw account bytes, then convert to AccountId32
    // Note: AccountId32 from sp_runtime doesn't implement IntoVisitor, so we decode as [u8; 32]
    let addr = subxt::dynamic::storage::<(), Vec<[u8; 32]>>("Session", "Validators");
    let validators_raw = client_at_block
        .storage()
        .fetch(addr, ())
        .await?
        .decode()
        .map_err(|e| {
            tracing::debug!("Failed to decode validators: {}", e);
            GetBlockError::StorageDecodeFailed(parity_scale_codec::Error::from(
                "Failed to decode validators",
            ))
        })?;
    let validators: Vec<AccountId32> = validators_raw.into_iter().map(AccountId32::from).collect();

    if validators.is_empty() {
        return Err(parity_scale_codec::Error::from("no validators found in storage").into());
    }

    Ok(validators)
}

/// Extract author ID from block header digest logs by mapping authority index to validator
pub async fn extract_author(
    state: &AppState,
    client_at_block: &BlockClient,
    logs: &[DigestLog],
    block_number: u64,
) -> Option<String> {
    extract_author_with_prefix(
        client_at_block,
        logs,
        state.chain_info.ss58_prefix,
        block_number,
    )
    .await
}

/// Extract author ID from block header digest logs by mapping authority index to validator.
/// This is the core implementation that accepts ss58_prefix directly.
pub async fn extract_author_with_prefix(
    client_at_block: &BlockClient,
    logs: &[DigestLog],
    ss58_prefix: u16,
    block_number: u64,
) -> Option<String> {
    use sp_consensus_babe::digests::PreDigest;

    const BABE_ENGINE: &[u8] = b"BABE";
    const AURA_ENGINE: &[u8] = b"aura";
    const POW_ENGINE: &[u8] = b"pow_";

    // Fetch validators once for this block
    let validators = match get_validators_at_block(client_at_block).await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Failed to get validators for block {}: {}", block_number, e);
            return None;
        }
    };

    // Check PreRuntime logs for BABE/Aura
    for log in logs {
        if log.log_type == "PreRuntime"
            && let Some(arr) = log.value.as_array()
            && arr.len() >= 2
        {
            let engine_id_hex = arr[0].as_str()?;
            let payload_hex = arr[1].as_str()?;
            let payload = hex::decode(payload_hex.strip_prefix("0x")?).ok()?;

            // Decode hex-encoded engine ID to bytes for comparison
            let engine_id_bytes = hex::decode(engine_id_hex.strip_prefix("0x")?).ok()?;

            match engine_id_bytes.as_slice() {
                BABE_ENGINE => {
                    if payload.is_empty() {
                        continue;
                    }

                    // The payload has already been decoded from SCALE in decode_consensus_digest
                    // So we can decode the PreDigest directly without skipping compact length
                    let mut cursor = &payload[..];
                    let pre_digest = PreDigest::decode(&mut cursor).ok()?;
                    let authority_index = pre_digest.authority_index() as usize;
                    let author = validators.get(authority_index)?;

                    // Convert to SS58 format
                    return Some(author.clone().to_ss58check_with_version(ss58_prefix.into()));
                }
                AURA_ENGINE => {
                    // Aura: slot_number (u64 LE), calculate index = slot % validator_count
                    if payload.len() >= 8 {
                        let slot = u64::from_le_bytes([
                            payload[0], payload[1], payload[2], payload[3], payload[4], payload[5],
                            payload[6], payload[7],
                        ]) as usize;

                        let index = slot % validators.len();
                        let author = validators.get(index)?;

                        // Convert to SS58 format
                        return Some(author.clone().to_ss58check_with_version(ss58_prefix.into()));
                    }
                }
                _ => continue,
            }
        }
    }

    // Check Consensus logs for PoW
    for log in logs {
        if log.log_type == "Consensus"
            && let Some(arr) = log.value.as_array()
            && arr.len() >= 2
        {
            let engine_id_hex = arr[0].as_str()?;
            let payload_hex = arr[1].as_str()?;

            // Decode hex-encoded engine ID to bytes for comparison
            let engine_id_bytes = hex::decode(engine_id_hex.strip_prefix("0x")?).ok()?;

            if engine_id_bytes.as_slice() == POW_ENGINE {
                // PoW: author is directly in payload (32-byte AccountId)
                let payload = hex::decode(payload_hex.strip_prefix("0x")?).ok()?;
                if payload.len() == 32 {
                    // Payload is exactly 32 bytes, convert directly to AccountId32
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&payload);
                    let account_id = AccountId32::from(arr);
                    return Some(account_id.to_ss58check_with_version(ss58_prefix.into()));
                } else {
                    tracing::debug!(
                        "PoW payload has unexpected length: {} bytes (expected 32)",
                        payload.len()
                    );
                }
            }
        }
    }

    None
}

// ================================================================================================
// Documentation Helpers
// ================================================================================================

/// Add documentation to events if eventDocs is enabled
pub fn add_docs_to_events(events: &mut [Event], metadata: &subxt::Metadata) {
    for event in events.iter_mut() {
        // Pallet names in metadata are PascalCase, but our pallet names are lowerCamelCase
        // We need to convert back: "system" -> "System", "balances" -> "Balances"
        let pallet_name = event.method.pallet.to_upper_camel_case();
        event.docs = Docs::for_event_subxt(metadata, &pallet_name, &event.method.method)
            .map(|d| d.to_string());
    }
}

// ================================================================================================
// Range Parsing
// ================================================================================================

/// Error type for range parsing
#[derive(Debug, Clone, Copy)]
pub enum RangeParseError {
    InvalidFormat,
    InvalidMin,
    InvalidMax,
    MinGreaterThanOrEqualToMax,
    RangeTooLarge,
}

impl From<RangeParseError> for GetBlockError {
    fn from(err: RangeParseError) -> Self {
        match err {
            RangeParseError::InvalidFormat => GetBlockError::InvalidRangeFormat,
            RangeParseError::InvalidMin => GetBlockError::InvalidRangeMin,
            RangeParseError::InvalidMax => GetBlockError::InvalidRangeMax,
            RangeParseError::MinGreaterThanOrEqualToMax => GetBlockError::InvalidRangeMinMax,
            RangeParseError::RangeTooLarge => GetBlockError::RangeTooLarge,
        }
    }
}

pub fn parse_range(range: &str) -> Result<(u64, u64), RangeParseError> {
    let parts: Vec<_> = range.split('-').collect();
    if parts.len() != 2 {
        return Err(RangeParseError::InvalidFormat);
    }

    let start_str = parts[0].trim();
    let end_str = parts[1].trim();

    if start_str.is_empty() || end_str.is_empty() {
        return Err(RangeParseError::InvalidFormat);
    }

    let start: u64 = start_str.parse().map_err(|_| RangeParseError::InvalidMin)?;
    let end: u64 = end_str.parse().map_err(|_| RangeParseError::InvalidMax)?;

    if start >= end {
        return Err(RangeParseError::MinGreaterThanOrEqualToMax);
    }

    let count = end
        .checked_sub(start)
        .and_then(|d| d.checked_add(1))
        .ok_or(RangeParseError::RangeTooLarge)?;

    if count > 500 {
        return Err(RangeParseError::RangeTooLarge);
    }

    Ok((start, end))
}

// ================================================================================================
// Relay Chain State Queries
// ================================================================================================

pub async fn get_finalized_block_number_with_rpc(
    legacy_rpc: &Arc<crate::state::SubstrateLegacyRpc>,
    rpc_client: &Arc<RpcClient>,
) -> Result<u64, GetBlockError> {
    let finalized_hash = legacy_rpc
        .chain_get_finalized_head()
        .await
        .map_err(GetBlockError::FinalizedHeadFailed)?;

    let finalized_hash_str = format!("0x{}", hex::encode(finalized_hash.0));

    let header_json: serde_json::Value = rpc_client
        .request("chain_getHeader", rpc_params![finalized_hash_str])
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;

    let number_hex = header_json
        .get("number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("number".to_string()))?;

    let number = u64::from_str_radix(number_hex.trim_start_matches("0x"), 16)
        .map_err(|_| GetBlockError::HeaderFieldMissing("number (invalid format)".to_string()))?;

    Ok(number)
}

pub async fn get_canonical_hash_at_number_with_rpc(
    legacy_rpc: &Arc<crate::state::SubstrateLegacyRpc>,
    block_number: u64,
) -> Result<Option<String>, GetBlockError> {
    let hash = legacy_rpc
        .chain_get_block_hash(Some(block_number.into()))
        .await
        .map_err(GetBlockError::CanonicalHashFailed)?;

    Ok(hash.map(|h| format!("0x{}", hex::encode(h.0))))
}
