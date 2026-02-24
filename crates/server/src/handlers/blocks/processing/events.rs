// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Event fetching and categorization.
//!
//! This module handles:
//! - Fetching events from block storage
//! - Categorizing events by phase (onInitialize, per-extrinsic, onFinalize)
//! - Extracting extrinsic outcomes (success/failure, fees, weights) from events

// TODO: Consider using `client_at_block.events()` API from subxt 0.50 for fetching and working
// with events. This would handle decoding automatically without needing custom visitors/handlers,
// and could potentially simplify most of the code in this module.
// See: https://github.com/polkadot-api/polkadot-rest-api/pull/XXX#discussion_rXXXXXXXXX

use crate::state::AppState;
use serde_json::Value;

use super::super::common::BlockClient;
use super::super::decode::{
    EventPhase as VisitorEventPhase, EventsVisitor, convert_bytes_to_hex, transform_json_unified,
    try_convert_accountid_to_ss58,
};
use super::super::types::{
    ActualWeight, Event, EventPhase, ExtrinsicOutcome, GetBlockError, MethodInfo, OnFinalize,
    OnInitialize, ParsedEvent,
};
use super::super::utils::extract_number_as_string;

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

/// Fetch and parse all events for a block with explicit ss58_prefix
///
/// This version allows specifying the ss58_prefix explicitly, useful for
/// processing blocks from different chains (e.g., relay chain blocks).
pub async fn fetch_block_events_with_prefix(
    ss58_prefix: u16,
    client_at_block: &BlockClient,
    block_number: u64,
) -> Result<Vec<ParsedEvent>, GetBlockError> {
    fetch_block_events_impl(ss58_prefix, client_at_block, block_number).await
}

/// Fetch and parse all events for a block using the client_at_block only
///
/// This version is useful when you don't have access to AppState.
/// It uses ss58_prefix 0 (Polkadot) as default.
pub async fn fetch_block_events_with_client(
    client_at_block: &BlockClient,
    block_number: u64,
) -> Result<Vec<ParsedEvent>, GetBlockError> {
    // Use default Polkadot prefix - callers should use fetch_block_events_with_prefix
    // if they need a specific prefix
    fetch_block_events_impl(0, client_at_block, block_number).await
}

/// Fetch and parse all events for a block
pub async fn fetch_block_events(
    state: &AppState,
    client_at_block: &BlockClient,
    block_number: u64,
) -> Result<Vec<ParsedEvent>, GetBlockError> {
    fetch_block_events_impl(state.chain_info.ss58_prefix, client_at_block, block_number).await
}

/// Internal implementation for fetching block events
async fn fetch_block_events_impl(
    ss58_prefix: u16,
    client_at_block: &BlockClient,
    block_number: u64,
) -> Result<Vec<ParsedEvent>, GetBlockError> {
    // Get the type resolver from metadata for type-aware enum serialization
    let metadata = client_at_block.metadata();
    let resolver = metadata.types();

    // Use dynamic storage address for System::Events
    let addr = subxt::dynamic::storage::<(), scale_value::Value>("System", "Events");
    let events_value = client_at_block.storage().fetch(addr, ()).await?;

    // Decode events once using the visitor pattern which provides all needed data:
    // phase, pallet_name, event_name, and typed fields
    let events_with_types = events_value
        .visit(EventsVisitor::new(resolver))
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

    let mut parsed_events = Vec::with_capacity(events_with_types.len());

    for event_info in events_with_types {
        let phase = match event_info.phase {
            VisitorEventPhase::Initialization => EventPhase::Initialization,
            VisitorEventPhase::ApplyExtrinsic(idx) => EventPhase::ApplyExtrinsic(idx),
            VisitorEventPhase::Finalization => EventPhase::Finalization,
        };

        // Use the visitor's field values which have proper type-level enum serialization
        // (basic enums as strings, non-basic enums as objects)
        let event_data: Vec<Value> = event_info
            .fields
            .into_iter()
            .map(|event_field| {
                let json_value = event_field.value;
                let type_name = event_field.type_name;
                let type_name_ref = type_name.as_deref();

                if let Some(tn) = type_name_ref {
                    if tn == "AccountId32" || tn == "MultiAddress" || tn == "AccountId" {
                        let with_hex = convert_bytes_to_hex(json_value.clone());
                        if let Some(ss58_value) =
                            try_convert_accountid_to_ss58(&with_hex, ss58_prefix)
                        {
                            return ss58_value;
                        }
                    } else if tn == "RewardDestination"
                        && let Some(account_value) = json_value.get("account")
                    {
                        let with_hex = convert_bytes_to_hex(account_value.clone());
                        if let Some(ss58_value) =
                            try_convert_accountid_to_ss58(&with_hex, ss58_prefix)
                        {
                            return serde_json::json!({
                                "account": ss58_value
                            });
                        }
                    }
                }
                // Apply remaining transformations (bytes to hex, numbers to strings, camelCase keys)
                transform_json_unified(json_value, None)
            })
            .collect();

        parsed_events.push(ParsedEvent {
            phase,
            pallet_name: event_info.pallet_name,
            event_name: event_info.event_name,
            event_data,
        });
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
    // Create event vectors for each extrinsic with pre-allocated capacity
    let avg_events_per_ext = if num_extrinsics > 0 {
        (parsed_events.len() / num_extrinsics).max(4)
    } else {
        4
    };
    let mut per_extrinsic_events: Vec<Vec<Event>> = (0..num_extrinsics)
        .map(|_| Vec::with_capacity(avg_events_per_ext))
        .collect();
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
