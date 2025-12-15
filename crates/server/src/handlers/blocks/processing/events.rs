//! Event fetching and categorization.
//!
//! This module handles:
//! - Fetching events from block storage
//! - Categorizing events by phase (onInitialize, per-extrinsic, onFinalize)
//! - Extracting extrinsic outcomes (success/failure, fees, weights) from events

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

/// Fetch and parse all events for a block
pub async fn fetch_block_events(
    state: &AppState,
    client_at_block: &BlockClient<'_>,
    block_number: u64,
) -> Result<Vec<ParsedEvent>, GetBlockError> {
    // Get the resolver for type-aware enum serialization
    let resolver = client_at_block.resolver();

    let storage_entry = client_at_block.storage().entry("System", "Events")?;
    let events_value = storage_entry.fetch(()).await?.ok_or_else(|| {
        tracing::warn!("No events storage found for block {}", block_number);
        parity_scale_codec::Error::from("Events storage not found")
    })?;

    // Use the visitor pattern to get type information for each field
    let events_with_types = events_value
        .visit(EventsVisitor::new(&resolver))
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
                let _field_values: Vec<&scale_value::Value<()>> =
                    event_variant.values.values().collect();

                // Use the visitor's field values which have proper type-level enum serialization
                // (basic enums as strings, non-basic enums as objects)
                let event_data: Vec<Value> = event_info
                    .fields
                    .iter()
                    .map(|event_field| {
                        let json_value = event_field.value.clone();
                        let type_name = event_field.type_name.as_ref();

                        if let Some(tn) = type_name
                            && (tn == "AccountId32" || tn == "MultiAddress" || tn == "AccountId")
                        {
                            // For AccountId fields, try SS58 conversion
                            let with_hex = convert_bytes_to_hex(json_value.clone());
                            if let Some(ss58_value) = try_convert_accountid_to_ss58(
                                &with_hex,
                                state.chain_info.ss58_prefix,
                            ) {
                                return ss58_value;
                            }
                        }

                        // Apply remaining transformations (bytes to hex, numbers to strings, camelCase keys)
                        transform_json_unified(json_value.clone(), None)
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
