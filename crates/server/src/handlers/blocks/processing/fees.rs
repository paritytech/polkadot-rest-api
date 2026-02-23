// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fee extraction for extrinsics.
//!
//! This module handles extracting fee information for signed extrinsics using
//! a three-priority system:
//! 1. TransactionFeePaid event (exact fee from runtime)
//! 2. queryFeeDetails + calc_partial_fee (post-dispatch calculation)
//! 3. queryInfo (pre-dispatch estimation)

use crate::state::AppState;
use crate::utils::{self, decode_runtime_dispatch_info};
use parity_scale_codec::{Decode, Encode};
use serde_json::Value;
use subxt::SubstrateConfig;
use subxt::client::OnlineClientAtBlock;

use super::super::types::{Event, ExtrinsicOutcome};
use super::super::utils::{actual_weight_to_json, transform_fee_info};
use super::events::extract_fee_from_transaction_paid_event;

/// Query fee info via runtime API using subxt's high-level API.
///
/// This uses `client_at_parent.runtime_apis().call_raw()` which handles
/// the RPC call and block hash automatically.
async fn query_fee_info_via_runtime_api(
    client_at_parent: &OnlineClientAtBlock<SubstrateConfig>,
    extrinsic_bytes: &[u8],
) -> Option<(Value, String)> {
    let mut params = extrinsic_bytes.to_vec();
    let len = extrinsic_bytes.len() as u32;
    len.encode_to(&mut params);

    let result_bytes = client_at_parent
        .runtime_apis()
        .call_raw("TransactionPaymentApi_query_info", Some(&params))
        .await
        .ok()?;

    let dispatch_info = decode_runtime_dispatch_info(&result_bytes)?;

    let query_info = dispatch_info.to_json();
    let weight = dispatch_info.weight.ref_time().to_string();
    Some((query_info, weight))
}

async fn query_fee_details_via_runtime_api(
    client_at_parent: &OnlineClientAtBlock<SubstrateConfig>,
    extrinsic_bytes: &[u8],
) -> Option<Value> {
    let mut params = extrinsic_bytes.to_vec();
    let len = extrinsic_bytes.len() as u32;
    len.encode_to(&mut params);

    let result_bytes = client_at_parent
        .runtime_apis()
        .call_raw("TransactionPaymentApi_query_fee_details", Some(&params))
        .await
        .ok()?;

    decode_fee_details(&result_bytes)
}

fn decode_fee_details(bytes: &[u8]) -> Option<Value> {
    if bytes.is_empty() {
        return None;
    }

    let (inclusion_fee, _tip): (Option<(u128, u128, u128)>, u128) =
        Decode::decode(&mut &bytes[..]).ok()?;

    let inclusion_fee_json = inclusion_fee.map(|(base_fee, len_fee, adjusted_weight_fee)| {
        serde_json::json!({
            "baseFee": base_fee.to_string(),
            "lenFee": len_fee.to_string(),
            "adjustedWeightFee": adjusted_weight_fee.to_string()
        })
    });

    Some(serde_json::json!({ "inclusionFee": inclusion_fee_json }))
}

/// Extract fee info for a signed extrinsic using the three-priority system:
/// 1. TransactionFeePaid event (exact fee from runtime)
/// 2. queryFeeDetails + calc_partial_fee (post-dispatch calculation)
/// 3. queryInfo (pre-dispatch estimation)
///
pub async fn extract_fee_info_for_extrinsic(
    state: &AppState,
    client_at_parent: &OnlineClientAtBlock<SubstrateConfig>,
    extrinsic_hex: &str,
    events: &[Event],
    outcome: Option<&ExtrinsicOutcome>,
    spec_version: u32,
    spec_name: &str,
) -> serde_json::Map<String, Value> {
    // Priority 1: TransactionFeePaid event (exact fee from runtime)
    // This avoids any RPC calls when the event is present.
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

    let extrinsic_bytes = match hex::decode(extrinsic_hex.trim_start_matches("0x")) {
        Ok(bytes) => bytes,
        Err(_) => return serde_json::Map::new(),
    };

    // Check if we need fee details for Priority 2
    let actual_weight_str = outcome
        .and_then(|o| o.actual_weight.as_ref())
        .and_then(|w| w.ref_time.clone());

    let needs_fee_details = actual_weight_str.is_some()
        && state
            .fee_details_cache
            .is_available(spec_name, spec_version)
            .unwrap_or(true);

    // Run query_info and query_fee_details in parallel when both are needed.
    // These are independent RPC calls to the runtime API.
    let (query_info_result, fee_details_result) = if needs_fee_details {
        let (info, details) = tokio::join!(
            query_fee_info_via_runtime_api(client_at_parent, &extrinsic_bytes),
            query_fee_details_via_runtime_api(client_at_parent, &extrinsic_bytes),
        );
        (info, Some(details))
    } else {
        let info = query_fee_info_via_runtime_api(client_at_parent, &extrinsic_bytes).await;
        (info, None)
    };

    // Priority 2: queryFeeDetails + calc_partial_fee (post-dispatch calculation)
    if let Some(ref actual_weight_str) = actual_weight_str {
        if let Some(fee_details_opt) = fee_details_result {
            if let Some(fee_details_response) = fee_details_opt {
                state.fee_details_cache.set_available(spec_version, true);

                if let Some(fee_details) = utils::parse_fee_details(&fee_details_response)
                    && let Some((ref query_info, ref estimated_weight)) = query_info_result
                    && let Ok(partial_fee) = utils::calculate_accurate_fee(
                        &fee_details,
                        estimated_weight,
                        actual_weight_str,
                    )
                {
                    let mut info = transform_fee_info(query_info.clone());

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

                    info.insert("partialFee".to_string(), Value::String(partial_fee));
                    info.insert(
                        "kind".to_string(),
                        Value::String("postDispatch".to_string()),
                    );
                    return info;
                }
            } else {
                state.fee_details_cache.set_available(spec_version, false);
            }
        }
    }

    // Priority 3: queryInfo (pre-dispatch estimation) - reuse cached result
    if let Some((query_info, _)) = query_info_result {
        let mut info = transform_fee_info(query_info);

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

        info.insert("kind".to_string(), Value::String("preDispatch".to_string()));
        return info;
    }

    serde_json::Map::new()
}
