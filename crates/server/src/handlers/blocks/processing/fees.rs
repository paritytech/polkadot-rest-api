//! Fee extraction for extrinsics.
//!
//! This module handles extracting fee information for signed extrinsics using
//! a three-priority system:
//! 1. TransactionFeePaid event (exact fee from runtime)
//! 2. queryFeeDetails + calc_partial_fee (post-dispatch calculation)
//! 3. queryInfo (pre-dispatch estimation)

use crate::state::AppState;
use crate::utils;
use serde_json::Value;

use super::super::types::{Event, ExtrinsicOutcome};
use super::super::utils::{actual_weight_to_json, transform_fee_info};
use super::events::extract_fee_from_transaction_paid_event;

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
