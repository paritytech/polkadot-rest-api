//! Common block processing logic shared by block-related handlers.
//!
//! This module contains functions for fetching and processing block data that are
//! shared between endpoints like `/blocks/{blockId}` and `/blocks/head`.

use crate::state::AppState;
use crate::utils::{self, EraInfo, hex_with_prefix};
use heck::{ToLowerCamelCase, ToUpperCamelCase};
use parity_scale_codec::Decode;
use serde_json::{Value, json};
use sp_core::crypto::{AccountId32, Ss58Codec};
use sp_runtime::traits::BlakeTwo256;
use sp_runtime::traits::Hash as HashT;
use subxt_historic::SubstrateConfig;
use subxt_historic::client::{ClientAtBlock, OnlineClientAtBlock};

/// Type alias for the ClientAtBlock type used throughout the codebase.
/// This represents a client pinned to a specific block height with access to
/// storage, extrinsics, and metadata for that block.
pub type BlockClient<'a> = ClientAtBlock<OnlineClientAtBlock<'a, SubstrateConfig>, SubstrateConfig>;

use super::docs::Docs;
use super::transform::{
    actual_weight_to_json, convert_bytes_to_hex, extract_number_as_string,
    extract_numeric_string, transform_fee_info, transform_json_unified,
    try_convert_accountid_to_ss58, JsonVisitor,
};
use super::type_name_visitor::GetTypeName;
use super::types::{
    ActualWeight, CONSENSUS_ENGINE_ID_LEN, DigestItemDiscriminant, DigestLog, Event, EventPhase,
    ExtrinsicInfo, ExtrinsicOutcome, GetBlockError, MethodInfo, MultiAddress, OnFinalize,
    OnInitialize, ParsedEvent, SignatureInfo, SignerId,
};

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
    client_at_block: &BlockClient<'_>,
) -> Result<Vec<AccountId32>, GetBlockError> {
    let storage_entry = client_at_block.storage().entry("Session", "Validators")?;
    let validators_value = storage_entry.fetch(()).await?.ok_or_else(|| {
        // Use the parity_scale_codec::Error for missing validators which will be converted to StorageDecodeFailed
        parity_scale_codec::Error::from("validators storage not found")
    })?;
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
    client_at_block: &BlockClient<'_>,
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
// Event Processing
// ================================================================================================

/// Extract `paysFee` value from DispatchInfo in event data
///
/// DispatchInfo contains: { weight, class, paysFee }
/// paysFee can be:
/// - A boolean (true/false)
/// - A string ("Yes"/"No")
/// - An object with a "name" field containing "Yes"/"No"
///
/// For ExtrinsicSuccess: event_data = [DispatchInfo]
/// For ExtrinsicFailed: event_data = [DispatchError, DispatchInfo]
pub fn extract_pays_fee_from_event_data(event_data: &[Value], is_success: bool) -> Option<bool> {
    // For ExtrinsicSuccess, DispatchInfo is the first element
    // For ExtrinsicFailed, DispatchInfo is the second element (after DispatchError)
    let dispatch_info_index = if is_success { 0 } else { 1 };

    let dispatch_info = event_data.get(dispatch_info_index)?;

    // DispatchInfo should be an object with paysFee field
    let pays_fee_value = dispatch_info.get("paysFee")?;

    match pays_fee_value {
        // Direct boolean
        Value::Bool(b) => Some(*b),
        // String "Yes" or "No"
        Value::String(s) => match s.as_str() {
            "Yes" => Some(true),
            "No" => Some(false),
            _ => {
                tracing::debug!("Unknown paysFee string value: {}", s);
                None
            }
        },
        // Object with "name" field (e.g., { "name": "Yes", "values": ... })
        Value::Object(obj) => {
            if let Some(Value::String(name)) = obj.get("name") {
                match name.as_str() {
                    "Yes" => Some(true),
                    "No" => Some(false),
                    _ => {
                        tracing::debug!("Unknown paysFee name value: {}", name);
                        None
                    }
                }
            } else {
                None
            }
        }
        _ => {
            tracing::debug!("Unexpected paysFee value type: {:?}", pays_fee_value);
            None
        }
    }
}

/// Extract fee from TransactionFeePaid event if present
///
/// TransactionFeePaid event data: [who, actualFee, tip]
/// The actualFee is the exact fee paid for the transaction
pub fn extract_fee_from_transaction_paid_event(events: &[Event]) -> Option<String> {
    for event in events {
        // Check for System.TransactionFeePaid or TransactionPayment.TransactionFeePaid
        // Use case-insensitive comparison since pallet names may vary in casing
        let pallet_lower = event.method.pallet.to_lowercase();
        let is_fee_paid = (pallet_lower == "system" || pallet_lower == "transactionpayment")
            && event.method.method == "TransactionFeePaid";

        if is_fee_paid && event.data.len() >= 2 {
            // event.data[1] is the actualFee
            if let Some(fee_value) = event.data.get(1) {
                return Some(extract_number_as_string(fee_value));
            }
        }
    }
    None
}

/// Extract actual weight from DispatchInfo in event data
///
/// DispatchInfo contains: { weight, class, paysFee }
/// Weight can be:
/// - Modern format: { refTime/ref_time: "...", proofSize/proof_size: "..." }
/// - Legacy format: a single number (just refTime)
///
/// For ExtrinsicSuccess: event_data = [DispatchInfo]
/// For ExtrinsicFailed: event_data = [DispatchError, DispatchInfo]
pub fn extract_weight_from_event_data(
    event_data: &[Value],
    is_success: bool,
) -> Option<ActualWeight> {
    let dispatch_info_index = if is_success { 0 } else { 1 };
    let dispatch_info = event_data.get(dispatch_info_index)?;
    let weight_value = dispatch_info.get("weight")?;

    match weight_value {
        Value::Object(obj) => {
            // Handle both camelCase and snake_case key variants
            let ref_time = obj
                .get("refTime")
                .or_else(|| obj.get("ref_time"))
                .map(extract_number_as_string);
            let proof_size = obj
                .get("proofSize")
                .or_else(|| obj.get("proof_size"))
                .map(extract_number_as_string);

            Some(ActualWeight {
                ref_time,
                proof_size,
            })
        }
        // Legacy weight format: single number
        Value::Number(n) => Some(ActualWeight {
            ref_time: Some(n.to_string()),
            proof_size: None,
        }),
        Value::String(s) => {
            // Could be a hex string or decimal string
            let value = if s.starts_with("0x") {
                u128::from_str_radix(s.trim_start_matches("0x"), 16)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|_| s.clone())
            } else {
                s.clone()
            };
            Some(ActualWeight {
                ref_time: Some(value),
                proof_size: None,
            })
        }
        _ => {
            tracing::debug!("Unexpected weight value type: {:?}", weight_value);
            None
        }
    }
}

/// Extract class from DispatchInfo in event data
///
/// For ExtrinsicSuccess: event_data = [DispatchInfo]
/// For ExtrinsicFailed: event_data = [DispatchError, DispatchInfo]
pub fn extract_class_from_event_data(event_data: &[Value], is_success: bool) -> Option<String> {
    let dispatch_info_index = if is_success { 0 } else { 1 };
    let dispatch_info = event_data.get(dispatch_info_index)?;
    let class_value = dispatch_info.get("class")?;

    match class_value {
        Value::String(s) => Some(s.clone()),
        Value::Object(obj) => {
            // Might be { "name": "Normal", "values": ... } format
            obj.get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }
        _ => None,
    }
}

/// Fetch and parse all events for a block
pub async fn fetch_block_events(
    state: &AppState,
    client_at_block: &BlockClient<'_>,
    block_number: u64,
) -> Result<Vec<ParsedEvent>, GetBlockError> {
    use crate::handlers::blocks::events_visitor::{EventPhase as VisitorEventPhase, EventsVisitor};

    let storage_entry = client_at_block.storage().entry("System", "Events")?;
    let events_value = storage_entry.fetch(()).await?.ok_or_else(|| {
        tracing::warn!("No events storage found for block {}", block_number);
        parity_scale_codec::Error::from("Events storage not found")
    })?;

    // Use the visitor pattern to get type information for each field
    let events_with_types = events_value.visit(EventsVisitor::new()).map_err(|e| {
        tracing::warn!(
            "Failed to decode events for block {}: {:?}",
            block_number,
            e
        );
        GetBlockError::StorageDecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode events",
        ))
    })?;

    // Also decode with scale_value to preserve structure
    let events_vec = events_value
        .decode_as::<Vec<scale_value::Value<()>>>()
        .map_err(|e| {
            tracing::warn!(
                "Failed to decode events for block {}: {:?}",
                block_number,
                e
            );
            GetBlockError::StorageDecodeFailed(parity_scale_codec::Error::from(
                "Failed to decode events",
            ))
        })?;

    let mut parsed_events = Vec::new();

    // Process each event, combining type info from visitor with structure from scale_value
    for (event_info, event_record) in events_with_types.iter().zip(events_vec.iter()) {
        let phase = match event_info.phase {
            VisitorEventPhase::Initialization => EventPhase::Initialization,
            VisitorEventPhase::ApplyExtrinsic(idx) => EventPhase::ApplyExtrinsic(idx),
            VisitorEventPhase::Finalization => EventPhase::Finalization,
        };

        // Get the event variant from scale_value (to preserve structure)
        let event_composite = match &event_record.value {
            scale_value::ValueDef::Composite(comp) => comp,
            _ => continue,
        };

        let fields: Vec<&scale_value::Value<()>> = event_composite.values().collect();
        if fields.len() < 2 {
            continue;
        }

        if let scale_value::ValueDef::Variant(pallet_variant) = &fields[1].value {
            let inner_values: Vec<&scale_value::Value<()>> =
                pallet_variant.values.values().collect();

            if let Some(inner_value) = inner_values.first()
                && let scale_value::ValueDef::Variant(event_variant) = &inner_value.value
            {
                let field_values: Vec<&scale_value::Value<()>> =
                    event_variant.values.values().collect();

                let event_data: Vec<Value> = field_values
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, field)| {
                        let json_value = serde_json::to_value(&field.value).ok()?;

                        // Type-based AccountId32 detection using type info from visitor
                        if let Some(type_name) = event_info
                            .fields
                            .get(idx)
                            .and_then(|f| f.type_name.as_ref())
                            && (type_name == "AccountId32"
                                || type_name == "MultiAddress"
                                || type_name == "AccountId")
                        {
                            // For AccountId fields, we need hex conversion first, then SS58 conversion
                            let with_hex = convert_bytes_to_hex(json_value.clone());
                            if let Some(ss58_value) = try_convert_accountid_to_ss58(
                                &with_hex,
                                state.chain_info.ss58_prefix,
                            ) {
                                return Some(ss58_value);
                            }
                            // If SS58 conversion failed, fall through to unified transformation
                        }

                        // Single-pass transformation for non-AccountId fields (or AccountId fields where conversion failed)
                        Some(transform_json_unified(json_value, None))
                    })
                    .collect();

                parsed_events.push(ParsedEvent {
                    phase,
                    pallet_name: event_info.pallet_name.clone(),
                    event_name: event_info.event_name.clone(),
                    event_data,
                });
            }
        }
    }

    Ok(parsed_events)
}

/// Categorize parsed events into onInitialize, per-extrinsic, and onFinalize arrays
/// Also extracts extrinsic outcomes (success, paysFee) from System.ExtrinsicSuccess/ExtrinsicFailed events
pub fn categorize_events(
    parsed_events: Vec<ParsedEvent>,
    num_extrinsics: usize,
) -> (
    OnInitialize,
    Vec<Vec<Event>>,
    OnFinalize,
    Vec<ExtrinsicOutcome>,
) {
    let mut on_initialize_events = Vec::new();
    let mut on_finalize_events = Vec::new();
    // Create empty event vectors for each extrinsic
    let mut per_extrinsic_events: Vec<Vec<Event>> = vec![Vec::new(); num_extrinsics];
    // Create default outcomes for each extrinsic (success=false, pays_fee=None)
    let mut extrinsic_outcomes: Vec<ExtrinsicOutcome> =
        vec![ExtrinsicOutcome::default(); num_extrinsics];

    for parsed_event in parsed_events {
        // Check for System.ExtrinsicSuccess or System.ExtrinsicFailed events
        // to determine extrinsic outcomes before consuming the event data
        // Note: pallet_name is lowercase (from events_visitor.rs which uses to_lowercase())
        let is_system_event = parsed_event.pallet_name == "system";
        let is_success_event = is_system_event && parsed_event.event_name == "ExtrinsicSuccess";
        let is_failed_event = is_system_event && parsed_event.event_name == "ExtrinsicFailed";

        // Extract outcome info if this is a success/failed event for an extrinsic
        if let EventPhase::ApplyExtrinsic(index) = &parsed_event.phase {
            let idx = *index as usize;
            if idx < num_extrinsics {
                if is_success_event {
                    extrinsic_outcomes[idx].success = true;
                    // Extract paysFee from DispatchInfo (first element in event data)
                    if let Some(pays_fee) =
                        extract_pays_fee_from_event_data(&parsed_event.event_data, true)
                    {
                        extrinsic_outcomes[idx].pays_fee = Some(pays_fee);
                    }
                    // Extract actual weight from DispatchInfo for fee calculation
                    if let Some(weight) =
                        extract_weight_from_event_data(&parsed_event.event_data, true)
                    {
                        extrinsic_outcomes[idx].actual_weight = Some(weight);
                    }
                    // Extract class from DispatchInfo
                    if let Some(class) =
                        extract_class_from_event_data(&parsed_event.event_data, true)
                    {
                        extrinsic_outcomes[idx].class = Some(class);
                    }
                } else if is_failed_event {
                    // success stays false
                    // Extract paysFee from DispatchInfo (second element in event data, after DispatchError)
                    if let Some(pays_fee) =
                        extract_pays_fee_from_event_data(&parsed_event.event_data, false)
                    {
                        extrinsic_outcomes[idx].pays_fee = Some(pays_fee);
                    }
                    // Extract actual weight from DispatchInfo for fee calculation
                    if let Some(weight) =
                        extract_weight_from_event_data(&parsed_event.event_data, false)
                    {
                        extrinsic_outcomes[idx].actual_weight = Some(weight);
                    }
                    // Extract class from DispatchInfo
                    if let Some(class) =
                        extract_class_from_event_data(&parsed_event.event_data, false)
                    {
                        extrinsic_outcomes[idx].class = Some(class);
                    }
                }
            }
        }

        let event = Event {
            method: MethodInfo {
                pallet: parsed_event.pallet_name,
                method: parsed_event.event_name,
            },
            data: parsed_event.event_data,
            docs: None, // Will be populated if eventDocs=true
        };

        match parsed_event.phase {
            EventPhase::Initialization => {
                on_initialize_events.push(event);
            }
            EventPhase::ApplyExtrinsic(index) => {
                if let Some(extrinsic_events) = per_extrinsic_events.get_mut(index as usize) {
                    extrinsic_events.push(event);
                } else {
                    tracing::warn!(
                        "Event has ApplyExtrinsic phase with index {} but only {} extrinsics exist",
                        index,
                        num_extrinsics
                    );
                }
            }
            EventPhase::Finalization => {
                on_finalize_events.push(event);
            }
        }
    }

    (
        OnInitialize {
            events: on_initialize_events,
        },
        per_extrinsic_events,
        OnFinalize {
            events: on_finalize_events,
        },
        extrinsic_outcomes,
    )
}

// ================================================================================================
// Fee Extraction
// ================================================================================================

/// Get query info from RPC or runtime API fallback
pub async fn get_query_info(
    state: &AppState,
    extrinsic_hex: &str,
    parent_hash: &str,
) -> Option<(Value, String)> {
    // Try RPC first
    if let Ok(query_info) = state.query_fee_info(extrinsic_hex, parent_hash).await
        && let Some(weight) = utils::extract_estimated_weight(&query_info)
    {
        return Some((query_info, weight));
    }

    // Fall back to runtime API for historic blocks
    let extrinsic_bytes = hex::decode(extrinsic_hex.trim_start_matches("0x")).ok()?;
    let dispatch_info = state
        .query_fee_info_via_runtime_api(&extrinsic_bytes, parent_hash)
        .await
        .ok()?;

    let query_info = dispatch_info.to_json();
    let weight = dispatch_info.weight.ref_time().to_string();
    Some((query_info, weight))
}

/// Extract fee info for a signed extrinsic using the three-priority system:
/// 1. TransactionFeePaid event (exact fee from runtime)
/// 2. queryFeeDetails + calc_partial_fee (post-dispatch calculation)
/// 3. queryInfo (pre-dispatch estimation)
pub async fn extract_fee_info_for_extrinsic(
    state: &AppState,
    extrinsic_hex: &str,
    events: &[Event],
    outcome: Option<&ExtrinsicOutcome>,
    parent_hash: &str,
    spec_version: u32,
) -> serde_json::Map<String, Value> {
    // Priority 1: TransactionFeePaid event (exact fee from runtime)
    if let Some(fee_from_event) = extract_fee_from_transaction_paid_event(events) {
        let mut info = serde_json::Map::new();

        if let Some(outcome) = outcome {
            if let Some(ref actual_weight) = outcome.actual_weight
                && let Some(weight_value) = actual_weight_to_json(actual_weight)
            {
                info.insert("weight".to_string(), weight_value);
            }
            if let Some(ref class) = outcome.class {
                info.insert("class".to_string(), Value::String(class.clone()));
            }
        }

        info.insert("partialFee".to_string(), Value::String(fee_from_event));
        info.insert("kind".to_string(), Value::String("fromEvent".to_string()));
        return info;
    }

    // Priority 2: queryFeeDetails + calc_partial_fee (post-dispatch calculation)
    let actual_weight_str = outcome
        .and_then(|o| o.actual_weight.as_ref())
        .and_then(|w| w.ref_time.clone());

    if let Some(ref actual_weight_str) = actual_weight_str {
        let use_fee_details = state
            .fee_details_cache
            .is_available(&state.chain_info.spec_name, spec_version)
            .unwrap_or(true);

        if use_fee_details {
            if let Ok(fee_details_response) =
                state.query_fee_details(extrinsic_hex, parent_hash).await
            {
                state.fee_details_cache.set_available(spec_version, true);

                if let Some(fee_details) = utils::parse_fee_details(&fee_details_response) {
                    // Get estimated weight from queryInfo (try RPC first, then runtime API)
                    let query_info_result = get_query_info(state, extrinsic_hex, parent_hash).await;

                    if let Some((query_info, estimated_weight)) = query_info_result
                        && let Ok(partial_fee) = utils::calculate_accurate_fee(
                            &fee_details,
                            &estimated_weight,
                            actual_weight_str,
                        )
                    {
                        let mut info = transform_fee_info(query_info);
                        info.insert("partialFee".to_string(), Value::String(partial_fee));
                        info.insert(
                            "kind".to_string(),
                            Value::String("postDispatch".to_string()),
                        );
                        return info;
                    }
                }
            } else {
                state.fee_details_cache.set_available(spec_version, false);
            }
        }
    }

    // Priority 3: queryInfo (pre-dispatch estimation)
    if let Some((query_info, _)) = get_query_info(state, extrinsic_hex, parent_hash).await {
        let mut info = transform_fee_info(query_info);
        info.insert("kind".to_string(), Value::String("preDispatch".to_string()));
        return info;
    }

    serde_json::Map::new()
}

// ================================================================================================
// Extrinsic Processing
// ================================================================================================

/// Extract extrinsics from a block using subxt-historic
pub async fn extract_extrinsics(
    state: &AppState,
    client_at_block: &BlockClient<'_>,
    block_number: u64,
) -> Result<Vec<ExtrinsicInfo>, GetBlockError> {
    let extrinsics = match client_at_block.extrinsics().fetch().await {
        Ok(exts) => exts,
        Err(e) => {
            // This could indicate RPC issues or network problems
            tracing::warn!(
                "Failed to fetch extrinsics for block {}: {:?}. Returning empty extrinsics.",
                block_number,
                e
            );
            return Ok(Vec::new());
        }
    };

    let mut result = Vec::new();

    for extrinsic in extrinsics.iter() {
        // Extract pallet and method name from the call, converting to lowerCamelCase
        let pallet_name = extrinsic.call().pallet_name().to_lower_camel_case();
        let method_name = extrinsic.call().name().to_lower_camel_case();

        // Extract call arguments with field-name-based AccountId32 detection
        let fields = extrinsic.call().fields();
        let mut args_map = serde_json::Map::new();

        for field in fields.iter() {
            let field_name = field.name();
            // Keep field names as-is (snake_case from SCALE metadata)
            // Only nested object keys are transformed to camelCase via transform_json_unified
            let field_key = field_name.to_string();

            // Use the visitor pattern to get type information
            // This definitively detects AccountId32 fields by their actual type!
            let type_name = field.visit(GetTypeName::new()).ok().flatten();

            // Log the type name for demonstration
            if let Some(tn) = type_name {
                tracing::debug!(
                    "Field '{}' in {}.{} has type: {}",
                    field_name,
                    pallet_name,
                    method_name,
                    tn
                );
            }

            // Try to decode as AccountId32-related types based on the detected type name
            let is_account_type = type_name == Some("AccountId32")
                || type_name == Some("MultiAddress")
                || type_name == Some("AccountId");

            if is_account_type {
                let mut decoded_account = false;
                let ss58_prefix = state.chain_info.ss58_prefix;
                let bytes_to_ss58 = |bytes: &[u8; 32]| {
                    let account_id = AccountId32::from(*bytes);
                    account_id.to_ss58check_with_version(ss58_prefix.into())
                };

                if let Ok(account_bytes) = field.decode_as::<[u8; 32]>() {
                    let ss58 = bytes_to_ss58(&account_bytes);
                    args_map.insert(field_key.clone(), json!(ss58));
                    decoded_account = true;
                } else if let Ok(accounts) = field.decode_as::<Vec<[u8; 32]>>() {
                    let ss58_addresses: Vec<String> = accounts.iter().map(&bytes_to_ss58).collect();
                    args_map.insert(field_key.clone(), json!(ss58_addresses));
                    decoded_account = true;
                } else if let Ok(multi_addr) = field.decode_as::<MultiAddress>() {
                    let value = match multi_addr {
                        MultiAddress::Id(bytes) => {
                            json!({ "id": bytes_to_ss58(&bytes) })
                        }
                        MultiAddress::Address32(bytes) => {
                            json!({ "address32": bytes_to_ss58(&bytes) })
                        }
                        MultiAddress::Index(index) => json!({ "index": index }),
                        MultiAddress::Raw(bytes) => {
                            json!({ "raw": format!("0x{}", hex::encode(bytes)) })
                        }
                        MultiAddress::Address20(bytes) => {
                            json!({ "address20": format!("0x{}", hex::encode(bytes)) })
                        }
                    };
                    args_map.insert(field_key.clone(), value);
                    decoded_account = true;
                }

                if decoded_account {
                    continue;
                }
                // If we failed to decode as account types, fall through to Value<()> decoding
            }

            // For non-account fields (or account fields that failed to decode):
            // Use the type-aware JsonVisitor which correctly handles:
            // - SS58 encoding only for AccountId32/MultiAddress/AccountId types
            // - Preserving arrays for Vec<T> sequences
            // - Converting byte arrays to hex
            // - Enum variant transformation
            match field.visit(JsonVisitor::new(state.chain_info.ss58_prefix)) {
                Ok(json_value) => {
                    args_map.insert(field_key, json_value);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to decode field '{}' in {}.{}: {}",
                        field_name,
                        pallet_name,
                        method_name,
                        e
                    );
                }
            }
        }

        // Extract signature and signer (if signed)
        let (signature_info, era_from_bytes) = if extrinsic.is_signed() {
            let sig_bytes = extrinsic
                .signature_bytes()
                .ok_or(GetBlockError::MissingSignatureBytes)?;
            let addr_bytes = extrinsic
                .address_bytes()
                .ok_or(GetBlockError::MissingAddressBytes)?;

            // Try to extract era from raw extrinsic bytes
            // Era comes right after address and signature in the SignedExtra/TransactionExtension
            let era_info = utils::extract_era_from_extrinsic_bytes(extrinsic.bytes());

            let signer_hex = format!("0x{}", hex::encode(addr_bytes));
            let signer_ss58 =
                utils::decode_address_to_ss58(&signer_hex, state.chain_info.ss58_prefix)
                    .unwrap_or_else(|| signer_hex.clone());

            // Strip the signature type prefix byte (0x00=Ed25519, 0x01=Sr25519, 0x02=Ecdsa)
            let signature_without_type_prefix = if sig_bytes.len() > 1 {
                &sig_bytes[1..]
            } else {
                sig_bytes
            };

            (
                Some(SignatureInfo {
                    signature: format!("0x{}", hex::encode(signature_without_type_prefix)),
                    signer: SignerId { id: signer_ss58 },
                }),
                era_info,
            )
        } else {
            (None, None)
        };

        // Extract nonce, tip, and era from transaction extensions (if present)
        let (nonce, tip, era_info) = if let Some(extensions) = extrinsic.transaction_extensions() {
            let mut nonce_value = None;
            let mut tip_value = None;
            let mut era_value = None;

            tracing::trace!(
                "Extrinsic {} has {} extensions",
                extrinsic.index(),
                extensions.iter().count()
            );

            for ext in extensions.iter() {
                let ext_name = ext.name();
                tracing::trace!("Extension name: {}", ext_name);

                match ext_name {
                    "CheckNonce" => {
                        // Decode as a u64/u32 compact value, then serialize to JSON
                        if let Ok(n) = ext.decode_as::<scale_value::Value>()
                            && let Ok(json_val) = serde_json::to_value(&n)
                        {
                            // The value might be nested in an object, so we need to extract it
                            // If extraction fails, nonce_value remains None (serialized as null)
                            nonce_value = extract_numeric_string(&json_val);
                        }
                    }
                    "ChargeTransactionPayment" | "ChargeAssetTxPayment" => {
                        // The tip is typically a Compact<u128>
                        if let Ok(t) = ext.decode_as::<scale_value::Value>()
                            && let Ok(json_val) = serde_json::to_value(&t)
                        {
                            // If extraction fails, tip_value remains None (serialized as null)
                            tip_value = extract_numeric_string(&json_val);
                        }
                    }
                    "CheckMortality" | "CheckEra" => {
                        // Era information - decode directly from raw bytes
                        // The JSON representation is complex (e.g., "Mortal230") and harder to parse
                        let era_bytes = ext.bytes();
                        tracing::debug!(
                            "Found CheckMortality extension, raw bytes: {}",
                            hex::encode(era_bytes)
                        );

                        let mut offset = 0;
                        if let Some(decoded_era) =
                            utils::decode_era_from_bytes(era_bytes, &mut offset)
                        {
                            tracing::debug!("Decoded era: {:?}", decoded_era);

                            // Create a JSON representation that parse_era_info can understand
                            if let Some(ref mortal) = decoded_era.mortal_era {
                                // Format: {"name": "Mortal", "values": [[period], [phase]]}
                                let mut map = serde_json::Map::new();
                                map.insert("name".to_string(), Value::String("Mortal".to_string()));

                                let values = vec![
                                    Value::Array(vec![Value::Number(
                                        mortal[0].parse::<u64>().unwrap().into(),
                                    )]),
                                    Value::Array(vec![Value::Number(
                                        mortal[1].parse::<u64>().unwrap().into(),
                                    )]),
                                ];
                                map.insert("values".to_string(), Value::Array(values));

                                era_value = Some(Value::Object(map));
                            } else if decoded_era.immortal_era.is_some() {
                                let mut map = serde_json::Map::new();
                                map.insert(
                                    "name".to_string(),
                                    Value::String("Immortal".to_string()),
                                );
                                era_value = Some(Value::Object(map));
                            }
                        }
                    }
                    _ => {
                        // Silently skip other extensions
                    }
                }
            }

            let era = if let Some(era_json) = era_value {
                // Try to parse era information from extension
                utils::parse_era_info(&era_json)
            } else if let Some(era_parsed) = era_from_bytes {
                // Use era extracted from raw bytes
                era_parsed
            } else {
                // Default to immortal era for signed transactions without explicit era
                EraInfo {
                    immortal_era: Some("0x00".to_string()),
                    mortal_era: None,
                }
            };

            (nonce_value, tip_value, era)
        } else {
            // Unsigned extrinsics are immortal
            (
                None,
                None,
                EraInfo {
                    immortal_era: Some("0x00".to_string()),
                    mortal_era: None,
                },
            )
        };

        let extrinsic_bytes = extrinsic.bytes();
        let hash_bytes = BlakeTwo256::hash(extrinsic_bytes);
        let hash = format!("0x{}", hex::encode(hash_bytes.as_ref()));
        let raw_hex = format!("0x{}", hex::encode(extrinsic_bytes));

        // Initialize pays_fee based on whether the extrinsic is signed:
        // - Unsigned extrinsics (inherents) never pay fees → Some(false)
        // - Signed extrinsics: determined from DispatchInfo in events → None (will be updated later)
        let is_signed = signature_info.is_some();
        let pays_fee = if is_signed { None } else { Some(false) };

        result.push(ExtrinsicInfo {
            method: MethodInfo {
                pallet: pallet_name,
                method: method_name,
            },
            signature: signature_info,
            nonce,
            args: args_map,
            tip,
            hash,
            info: serde_json::Map::new(),
            era: era_info,
            events: Vec::new(),
            success: false,
            pays_fee,
            docs: None, // Will be populated if extrinsicDocs=true
            raw_hex,
        });
    }

    Ok(result)
}

// ================================================================================================
// Documentation Helpers
// ================================================================================================

/// Add documentation to events if eventDocs is enabled
pub fn add_docs_to_events(events: &mut [Event], metadata: &frame_metadata::RuntimeMetadata) {
    for event in events.iter_mut() {
        // Pallet names in metadata are PascalCase, but our pallet names are lowerCamelCase
        // We need to convert back: "system" -> "System", "balances" -> "Balances"
        let pallet_name = event.method.pallet.to_upper_camel_case();
        event.docs =
            Docs::for_event(metadata, &pallet_name, &event.method.method).map(|d| d.to_string());
    }
}
