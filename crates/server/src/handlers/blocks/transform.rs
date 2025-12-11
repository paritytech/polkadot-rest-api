//! JSON transformation utilities for block data.
//!
//! This module provides functions to transform SCALE-decoded values into
//! JSON format matching the substrate-api-sidecar response format.

use heck::ToLowerCamelCase;
use serde_json::Value;
use sp_core::crypto::AccountId32;

use super::types::ActualWeight;

// ================================================================================================
// Numeric Extraction
// ================================================================================================

/// Extract a numeric value from a JSON value as a string
/// Handles direct numbers, nested objects, or string representations
///
/// Returns None if the value cannot be extracted, which will serialize as null
/// in the JSON response (matching sidecar's behavior for missing/unextractable values)
pub fn extract_numeric_string(value: &Value) -> Option<String> {
    match value {
        // Direct number
        Value::Number(n) => Some(n.to_string()),
        // Direct string
        Value::String(s) => {
            // Remove parentheses if present: "(23)" -> "23"
            // This was present with Nonce values
            Some(s.trim_matches(|c| c == '(' || c == ')').to_string())
        }
        // Object - might be {"primitive": 23} or similar
        Value::Object(map) => {
            // Try to find a numeric field
            if let Some(val) = map.get("primitive") {
                return extract_numeric_string(val);
            }
            // Try other common field names
            for key in ["value", "0"] {
                if let Some(val) = map.get(key) {
                    return extract_numeric_string(val);
                }
            }
            // Could not find expected numeric field
            tracing::warn!(
                "Could not extract numeric value from object with keys: {:?}",
                map.keys().collect::<Vec<_>>()
            );
            None
        }
        // Array - take first element
        Value::Array(arr) => {
            if let Some(first) = arr.first() {
                extract_numeric_string(first)
            } else {
                tracing::warn!("Cannot extract numeric value from empty array");
                None
            }
        }
        _ => {
            tracing::warn!("Unexpected JSON type for numeric extraction: {:?}", value);
            None
        }
    }
}

/// Extract a number from a JSON value and return it as a string
/// Handles: numbers, hex strings (0x...), and string numbers
pub fn extract_number_as_string(value: &Value) -> String {
    match value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.starts_with("0x") {
                if let Ok(n) = u128::from_str_radix(s.trim_start_matches("0x"), 16) {
                    n.to_string()
                } else {
                    s.clone()
                }
            } else {
                s.clone()
            }
        }
        _ => "0".to_string(),
    }
}

// ================================================================================================
// JSON Transformation
// ================================================================================================

/// Convert JSON value, replacing byte arrays with hex strings and all numbers with strings recursively
///
/// This matches substrate-api-sidecar's behavior of returning all numeric values as strings
/// for consistency across the API.
pub fn convert_bytes_to_hex(value: Value) -> Value {
    match value {
        Value::Number(n) => {
            // Convert all numbers to strings to match substrate-api-sidecar behavior
            Value::String(n.to_string())
        }
        Value::Array(arr) => {
            // Check if this is a byte array (non-empty and all elements are numbers 0-255)
            // We must check !arr.is_empty() to avoid converting empty arrays to "0x"
            let is_byte_array = !arr.is_empty()
                && arr.iter().all(|v| match v {
                    Value::Number(n) => n.as_u64().is_some_and(|val| val <= 255),
                    _ => false,
                });

            if is_byte_array {
                // Convert to hex string
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                Value::String(format!("0x{}", hex::encode(&bytes)))
            } else {
                // Recurse into array elements
                let converted: Vec<Value> = arr.into_iter().map(convert_bytes_to_hex).collect();

                // If array has single element, unwrap it (this handles cases like ["0x..."] -> "0x...")
                // This is specific to how the data is formatted in substrate-api-sidecar
                match converted.len() {
                    1 => converted.into_iter().next().unwrap(),
                    _ => Value::Array(converted),
                }
            }
        }
        Value::Object(mut map) => {
            // Check if this is a bitvec object (scale-value represents bitvecs specially)
            if let Some(Value::Array(bits)) = map.get("__bitvec__values__") {
                // Convert boolean array to bytes, then to hex
                // BitVec uses LSB0 ordering (least significant bit first within each byte)
                let mut bytes = Vec::new();
                let mut current_byte = 0u8;

                for (i, bit) in bits.iter().enumerate() {
                    if let Some(true) = bit.as_bool() {
                        current_byte |= 1 << (i % 8);
                    }

                    // Every 8 bits, push the byte and reset
                    if (i + 1) % 8 == 0 {
                        bytes.push(current_byte);
                        current_byte = 0;
                    }
                }

                // Push any remaining bits
                if bits.len() % 8 != 0 {
                    bytes.push(current_byte);
                }

                return Value::String(format!("0x{}", hex::encode(&bytes)));
            }

            // Recurse into object values
            for (_, v) in map.iter_mut() {
                *v = convert_bytes_to_hex(v.clone());
            }
            Value::Object(map)
        }
        other => other,
    }
}

/// Unified transformation function that combines byte-to-hex conversion and structural transformations
/// in a single pass through the JSON tree.
///
/// This performs all of the following transformations in one traversal:
/// - Converts byte arrays to hex strings
/// - Converts numbers to strings
/// - Handles bitvec special encoding
/// - Transforms snake_case keys to camelCase
/// - Simplifies SCALE enum variants
/// - Optionally decodes AccountId32 to SS58 format
/// - Unwraps single-element arrays
pub fn transform_json_unified(value: Value, ss58_prefix: Option<u16>) -> Value {
    match value {
        Value::Number(n) => {
            // Convert all numbers to strings to match substrate-api-sidecar behavior
            Value::String(n.to_string())
        }
        Value::Array(arr) => {
            // Check if this is a byte array (all elements are numbers 0-255)
            // Require at least 2 elements - single-element arrays are typically newtype wrappers
            // (e.g., ValidatorIndex(32) -> [32]), not actual byte data
            let is_byte_array = arr.len() > 1
                && arr.iter().all(|v| match v {
                    Value::Number(n) => n.as_u64().is_some_and(|val| val <= 255),
                    _ => false,
                });

            if is_byte_array {
                // Convert to hex string
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                Value::String(format!("0x{}", hex::encode(&bytes)))
            } else {
                // Recurse into array elements
                let converted: Vec<Value> = arr
                    .into_iter()
                    .map(|v| transform_json_unified(v, ss58_prefix))
                    .collect();

                // If array has single element, unwrap it
                match converted.len() {
                    1 => converted.into_iter().next().unwrap(),
                    _ => Value::Array(converted),
                }
            }
        }
        Value::Object(map) => {
            // Check if this is a bitvec object (scale-value represents bitvecs specially)
            if let Some(Value::Array(bits)) = map.get("__bitvec__values__") {
                // Convert boolean array to bytes, then to hex
                let mut bytes = Vec::new();
                let mut current_byte = 0u8;

                for (i, bit) in bits.iter().enumerate() {
                    if let Some(true) = bit.as_bool() {
                        current_byte |= 1 << (i % 8);
                    }

                    if (i + 1) % 8 == 0 {
                        bytes.push(current_byte);
                        current_byte = 0;
                    }
                }

                if bits.len() % 8 != 0 {
                    bytes.push(current_byte);
                }

                return Value::String(format!("0x{}", hex::encode(&bytes)));
            }

            // Check if this is a SCALE enum variant: {"name": "X", "values": Y}
            if map.len() == 2
                && let (Some(Value::String(name)), Some(values)) =
                    (map.get("name"), map.get("values"))
            {
                // If values is "0x" (empty string) or [] (empty array), return just the name as string
                // This is evident in class, and paysFee
                let is_empty = match values {
                    Value::String(v) => v == "0x",
                    Value::Array(arr) => arr.is_empty(),
                    _ => false,
                };

                if is_empty {
                    // Special case: "None" variant should serialize as JSON null
                    if name == "None" {
                        return Value::Null;
                    }
                    return Value::String(name.clone());
                }

                // For args (when ss58_prefix is Some), transform to {"<name>": <transformed_values>}
                if ss58_prefix.is_some() {
                    // Only lowercase the first letter for CamelCase names (e.g., "PreRuntime" -> "preRuntime")
                    // Keep snake_case names as-is (e.g., "inbound_messages_data" stays unchanged)
                    let key = crate::utils::lowercase_first_char(name);
                    let transformed_value = transform_json_unified(values.clone(), ss58_prefix);

                    let mut result = serde_json::Map::new();
                    result.insert(key, transformed_value);
                    return Value::Object(result);
                }
                // For events (when ss58_prefix is None), we don't transform the enum further
                // Fall through to regular object handling
            }

            // Detect if this object is a "commitments" context by checking for characteristic sibling keys.
            let is_commitments_context = map.contains_key("upward_messages")
                || map.contains_key("upwardMessages")
                || map.contains_key("hrmp_watermark")
                || map.contains_key("hrmpWatermark")
                || map.contains_key("head_data")
                || map.contains_key("headData")
                || map.contains_key("processed_downward_messages")
                || map.contains_key("processedDownwardMessages");

            // Regular object: transform keys from snake_case to camelCase and recurse
            let transformed: serde_json::Map<String, Value> = map
                .into_iter()
                .map(|(key, val)| {
                    let camel_key = key.to_lower_camel_case();
                    let mut transformed_val = transform_json_unified(val, ss58_prefix);
                    // horizontalMessages can be either:
                    // 1. A Map (object) in inherent data (extrinsics[].args.data.horizontalMessages) - scale_value represents as [] or [[key, value], ...]
                    // 2. A Vec (array) in CandidateCommitments (candidate.commitments.horizontalMessages) - should remain as array
                    // 3. An Array in decoded XCM messages (decodedXcmMsgs.horizontalMessages) - should remain as array
                    //
                    // We detect the context:
                    // - If in commitments context (has sibling keys like upwardMessages, hrmpWatermark), keep as array
                    // - If array of tuples [[key, value], ...], it's a Map and should be converted to object
                    // - If empty array in inherent data context (ss58_prefix.is_some() and not commitments), convert to {}
                    if camel_key == "horizontalMessages"
                        && !is_commitments_context
                        && let Value::Array(arr) = &transformed_val
                    {
                        let is_map_format = !arr.is_empty()
                            && arr.iter().any(
                                |item| matches!(item, Value::Array(tuple) if tuple.len() == 2),
                            );

                        if is_map_format {
                            let mut obj = serde_json::Map::new();
                            for item in arr {
                                if let Value::Array(tuple) = item
                                    && tuple.len() == 2
                                {
                                    let key = match &tuple[0] {
                                        Value::String(s) => s.clone(),
                                        Value::Number(n) => n.to_string(),
                                        _ => continue,
                                    };
                                    obj.insert(
                                        key,
                                        transform_json_unified(tuple[1].clone(), ss58_prefix),
                                    );
                                }
                            }
                            transformed_val = Value::Object(obj);
                        } else if arr.is_empty() && ss58_prefix.is_some() {
                            transformed_val = Value::Object(serde_json::Map::new());
                        }
                        // If array is not empty and not Map format, leave as array (decodedXcmMsgs or commitments)
                    }
                    (camel_key, transformed_val)
                })
                .collect();
            Value::Object(transformed)
        }
        Value::String(s) => {
            // Try to decode as SS58 address if ss58_prefix is provided
            if let Some(prefix) = ss58_prefix
                && s.starts_with("0x")
                && (s.len() == 66 || s.len() == 68)
                && let Some(ss58_addr) = crate::utils::decode_address_to_ss58(&s, prefix)
            {
                return Value::String(ss58_addr);
            }

            Value::String(s)
        }
        other => other,
    }
}

/// Convert AccountId32 (as hex or array) to SS58 format
pub fn try_convert_accountid_to_ss58(value: &Value, ss58_prefix: u16) -> Option<Value> {
    use sp_core::crypto::Ss58Codec;

    if let Some(hex_str) = value.as_str()
        && hex_str.starts_with("0x")
        && hex_str.len() == 66
    {
        match hex::decode(&hex_str[2..]) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                let account_id = AccountId32::from(arr);
                let ss58 = account_id.to_ss58check_with_version(ss58_prefix.into());
                return Some(Value::String(ss58));
            }
            _ => {}
        }
    }

    if let Some(arr) = value.as_array()
        && arr.len() == 32
    {
        let mut bytes = [0u8; 32];
        for (i, val) in arr.iter().enumerate() {
            if let Some(byte) = val.as_u64() {
                bytes[i] = byte as u8;
            } else {
                return None;
            }
        }
        let account_id = AccountId32::from(bytes);
        let ss58 = account_id.to_ss58check_with_version(ss58_prefix.into());
        return Some(Value::String(ss58));
    }

    None
}

// ================================================================================================
// Fee Info Transformation
// ================================================================================================

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
