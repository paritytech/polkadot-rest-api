//! Fee information transformation utilities.
//!
//! This module provides functions for transforming fee-related data from
//! RPC responses into the expected JSON format.

use serde_json::Value;

use super::super::types::ActualWeight;
use super::numeric::extract_number_as_string;

/// Transform fee info from payment_queryInfo RPC response into the expected format
///
/// The RPC returns RuntimeDispatchInfo with:
/// - weight: either { refTime/ref_time, proofSize/proof_size } (modern) or a single number (legacy)
/// - class: "Normal", "Operational", or "Mandatory"
/// - partialFee: fee amount (usually as hex string from RPC)
///
/// We transform this to match sidecar's format with string values
pub fn transform_fee_info(fee_info: Value) -> serde_json::Map<String, Value> {
    let mut result = serde_json::Map::new();

    if let Some(weight) = fee_info.get("weight") {
        if weight.is_object() {
            // Handle both camelCase and snake_case key variants from different node versions
            let mut weight_map = serde_json::Map::new();

            let ref_time = weight.get("refTime").or_else(|| weight.get("ref_time"));
            let proof_size = weight.get("proofSize").or_else(|| weight.get("proof_size"));

            if let Some(rt) = ref_time {
                weight_map.insert(
                    "refTime".to_string(),
                    Value::String(extract_number_as_string(rt)),
                );
            }
            if let Some(ps) = proof_size {
                weight_map.insert(
                    "proofSize".to_string(),
                    Value::String(extract_number_as_string(ps)),
                );
            }

            if !weight_map.is_empty() {
                result.insert("weight".to_string(), Value::Object(weight_map));
            }
        } else {
            result.insert(
                "weight".to_string(),
                Value::String(extract_number_as_string(weight)),
            );
        }
    }

    if let Some(class) = fee_info.get("class") {
        result.insert("class".to_string(), class.clone());
    }

    if let Some(partial_fee) = fee_info.get("partialFee") {
        result.insert(
            "partialFee".to_string(),
            Value::String(extract_number_as_string(partial_fee)),
        );
    }

    result
}

/// Convert ActualWeight to JSON value (V1: string, V2: object)
pub fn actual_weight_to_json(actual_weight: &ActualWeight) -> Option<Value> {
    use serde_json::json;

    let ref_time = actual_weight.ref_time.as_ref()?;
    Some(if let Some(ref proof_size) = actual_weight.proof_size {
        json!({ "refTime": ref_time, "proofSize": proof_size })
    } else {
        Value::String(ref_time.clone())
    })
}
