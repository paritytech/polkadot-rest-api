//! Common block processing logic shared by block-related handlers.
//!
//! This module contains foundational utilities for block data:
//! - `BlockClient` type alias for working with blocks at specific heights
//! - Digest log decoding (PreRuntime, Consensus, Seal)
//! - Chain state queries (canonical hash, finalized head, validators)
//! - Block author extraction from consensus digests
//! - Documentation helpers for events

use crate::state::AppState;
use crate::utils::hex_with_prefix;
use heck::ToUpperCamelCase;
use parity_scale_codec::Decode;
use serde_json::{Value, json};
use sp_core::crypto::{AccountId32, Ss58Codec};
use subxt::{OnlineClientAtBlock, SubstrateConfig};

use super::docs::Docs;
use super::types::{
    CONSENSUS_ENGINE_ID_LEN, DigestItemDiscriminant, DigestLog, Event, GetBlockError,
};

/// Type alias for the ClientAtBlock type used throughout the codebase.
/// This represents a client pinned to a specific block height with access to
/// storage, extrinsics, and metadata for that block.
pub type BlockClient = OnlineClientAtBlock<SubstrateConfig>;

// ================================================================================================
// Digest Processing
// ================================================================================================

/// Decode a consensus digest item (PreRuntime, Consensus, or Seal)
/// The data here is SCALE-encoded as: (ConsensusEngineId, Vec<u8>)
/// where ConsensusEngineId is 4 raw bytes, and Vec<u8> is compact_length + bytes
pub fn decode_consensus_digest(data: &[u8]) -> Option<Value> {
    // First 4 bytes are the consensus engine ID (not length-prefixed)
    if data.len() < CONSENSUS_ENGINE_ID_LEN {
        return None;
    }

    let engine_id = hex_with_prefix(&data[0..CONSENSUS_ENGINE_ID_LEN]);

    // The rest is a SCALE-encoded Vec<u8> (compact length + payload bytes)
    let mut remaining = &data[CONSENSUS_ENGINE_ID_LEN..];
    let payload_bytes = Vec::<u8>::decode(&mut remaining).ok()?;
    let payload = hex_with_prefix(&payload_bytes);

    Some(json!([engine_id, payload]))
}

/// Decode digest logs from hex-encoded strings in the JSON response
/// Each hex string is a SCALE-encoded DigestItem
pub fn decode_digest_logs(header_json: &Value) -> Vec<DigestLog> {
    let logs = match header_json
        .get("digest")
        .and_then(|d| d.get("logs"))
        .and_then(|l| l.as_array())
    {
        Some(logs) => logs,
        None => return Vec::new(),
    };

    logs.iter()
        .filter_map(|log_hex| {
            let hex_str = log_hex.as_str()?;
            let hex_data = hex_str.strip_prefix("0x")?;
            let bytes = hex::decode(hex_data).ok()?;

            if bytes.is_empty() {
                return None;
            }

            // The first byte is the digest item type discriminant
            let discriminant_byte = bytes[0];
            let data = &bytes[1..];

            // Try to parse the discriminant into a known type
            let discriminant = DigestItemDiscriminant::try_from(discriminant_byte)
                .unwrap_or(DigestItemDiscriminant::Other);

            let (log_type, value) = match discriminant {
                // Consensus-related digests: PreRuntime, Consensus, Seal
                // All have format: [consensus_engine_id (4 bytes), payload_data]
                DigestItemDiscriminant::PreRuntime
                | DigestItemDiscriminant::Consensus
                | DigestItemDiscriminant::Seal => match decode_consensus_digest(data) {
                    Some(val) => (discriminant.as_str().to_string(), val),
                    None => ("Other".to_string(), json!(hex_with_prefix(&bytes))),
                },
                // RuntimeEnvironmentUpdated has no associated data
                DigestItemDiscriminant::RuntimeEnvironmentUpdated => {
                    (discriminant.as_str().to_string(), Value::Null)
                }
                // Other (includes unknown discriminants that were converted to Other)
                DigestItemDiscriminant::Other => (
                    discriminant.as_str().to_string(),
                    json!(hex_with_prefix(data)),
                ),
            };

            Some(DigestLog {
                log_type,
                index: discriminant_byte.to_string(),
                value,
            })
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
    // Use dynamic storage address for Session::Validators
    // Note: For dynamic storage, we need to specify the value type
    let addr = subxt::dynamic::storage::<(), scale_value::Value>("Session", "Validators");
    let validators_value = client_at_block
        .storage()
        .fetch(addr, ())
        .await?;
    let raw_bytes = validators_value.into_bytes();
    let validators_raw: Vec<[u8; 32]> = Vec::<[u8; 32]>::decode(&mut &raw_bytes[..])?;
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
                    return Some(
                        author
                            .clone()
                            .to_ss58check_with_version(state.chain_info.ss58_prefix.into()),
                    );
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
                        return Some(
                            author
                                .clone()
                                .to_ss58check_with_version(state.chain_info.ss58_prefix.into()),
                        );
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
                    return Some(
                        account_id.to_ss58check_with_version(state.chain_info.ss58_prefix.into()),
                    );
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
        event.docs =
            Docs::for_event_subxt(metadata, &pallet_name, &event.method.method).map(|d| d.to_string());
    }
}
