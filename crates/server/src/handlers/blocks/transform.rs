//! JSON transformation utilities for block data.
//!
//! This module provides functions to transform SCALE-decoded values into
//! JSON format matching the substrate-api-sidecar response format.

use heck::ToLowerCamelCase;
use scale_info::{PortableRegistry, TypeDef};
use serde_json::Value;
use sp_core::crypto::AccountId32;

use super::types::ActualWeight;

// ================================================================================================
// Registry-Aware SCALE Value Transformation
// ================================================================================================

/// Check if a type_id refers to a sequence type (Vec<T>) in the registry.
/// This is used to distinguish between sequences (which should stay as arrays)
/// and newtype wrappers (which should be unwrapped).
pub fn is_sequence_type(type_id: u32, registry: &PortableRegistry) -> bool {
    registry
        .resolve(type_id)
        .is_some_and(|ty| matches!(ty.type_def, TypeDef::Sequence(_)))
}

/// Check if an array of scale_value::Value looks like a byte array (all u8 values 0-255).
/// Requires at least 2 elements to avoid treating single compact integers as byte arrays.
pub fn is_byte_array_scale_value(values: &[scale_value::Value<u32>]) -> bool {
    values.len() >= 2
        && values.iter().all(|v| {
            matches!(
                &v.value,
                scale_value::ValueDef::Primitive(scale_value::Primitive::U128(n)) if *n <= 255
            )
        })
}

/// Convert a slice of scale_value::Value (representing bytes) to a hex string.
pub fn bytes_to_hex_scale_value(values: &[scale_value::Value<u32>]) -> String {
    let bytes: Vec<u8> = values
        .iter()
        .filter_map(|v| match &v.value {
            scale_value::ValueDef::Primitive(scale_value::Primitive::U128(n)) => Some(*n as u8),
            _ => None,
        })
        .collect();
    format!("0x{}", hex::encode(bytes))
}

/// Check if variant name is an X1, X2, etc junction.
/// These variants need special handling to preserve array output format.
fn is_junction_variant(name: &str) -> bool {
    matches!(name, "X1" | "X2" | "X3" | "X4" | "X5" | "X6" | "X7" | "X8")
}

/// Convert a scale_value::Value<u32> to serde_json::Value with registry awareness.
///
/// This correctly handles:
/// - Vec<T> (sequences) - always keeps as array even with single element
/// - Newtype wrappers - unwraps single unnamed field
/// - Byte arrays - converts to hex strings
/// - Named structs - converts to JSON objects with camelCase keys
/// - Enum variants - converts to { "variantName": value } format
///
/// The key insight: the decision to unwrap is based on the TYPE (checking TypeDef::Sequence
/// in the registry), not the array length.
pub fn scale_value_to_json(value: scale_value::Value<u32>, registry: &PortableRegistry) -> Value {
    let type_id = value.context;
    let is_sequence = is_sequence_type(type_id, registry);

    match value.value {
        scale_value::ValueDef::Composite(composite) => match composite {
            scale_value::Composite::Named(fields) => {
                let map: serde_json::Map<String, Value> = fields
                    .into_iter()
                    .map(|(name, val)| {
                        (
                            name.to_lower_camel_case(),
                            scale_value_to_json(val, registry),
                        )
                    })
                    .collect();
                Value::Object(map)
            }
            scale_value::Composite::Unnamed(fields) => {
                let fields_vec: Vec<_> = fields.into_iter().collect();
                // Check if this looks like a byte array
                if !fields_vec.is_empty() && is_byte_array_scale_value(&fields_vec) {
                    Value::String(bytes_to_hex_scale_value(&fields_vec))
                } else if fields_vec.len() == 1 && !is_sequence {
                    // Single unnamed field that's NOT a sequence - unwrap it (newtype wrapper)
                    scale_value_to_json(fields_vec.into_iter().next().unwrap(), registry)
                } else {
                    // Sequence type or multiple elements - keep as array
                    Value::Array(
                        fields_vec
                            .into_iter()
                            .map(|v| scale_value_to_json(v, registry))
                            .collect(),
                    )
                }
            }
        },
        scale_value::ValueDef::Variant(variant) => {
            // Handle Option::None as JSON null
            if variant.name == "None" {
                return Value::Null;
            }

            let name = variant.name.to_lower_camel_case();
            let is_junction = is_junction_variant(&variant.name);

            let inner = match variant.values {
                scale_value::Composite::Named(fields) if !fields.is_empty() => {
                    let map: serde_json::Map<String, Value> = fields
                        .into_iter()
                        .map(|(n, v)| (n.to_lower_camel_case(), scale_value_to_json(v, registry)))
                        .collect();
                    Value::Object(map)
                }
                scale_value::Composite::Unnamed(fields) if !fields.is_empty() => {
                    let fields_vec: Vec<_> = fields.into_iter().collect();
                    if !fields_vec.is_empty() && is_byte_array_scale_value(&fields_vec) {
                        Value::String(bytes_to_hex_scale_value(&fields_vec))
                    } else if fields_vec.len() == 1 && !is_junction {
                        let inner_type_id = fields_vec[0].context;
                        if is_sequence_type(inner_type_id, registry) {
                            // It's a sequence, keep recursing but don't unwrap here
                            scale_value_to_json(fields_vec.into_iter().next().unwrap(), registry)
                        } else {
                            // Not a sequence, unwrap the newtype wrapper
                            scale_value_to_json(fields_vec.into_iter().next().unwrap(), registry)
                        }
                    } else {
                        // For junctions (X1, X2, etc) or multi-element, output as array
                        Value::Array(
                            fields_vec
                                .into_iter()
                                .map(|v| scale_value_to_json(v, registry))
                                .collect(),
                        )
                    }
                }
                _ => Value::Null,
            };
            let mut map = serde_json::Map::new();
            map.insert(name, inner);
            Value::Object(map)
        }
        scale_value::ValueDef::Primitive(prim) => match prim {
            scale_value::Primitive::Bool(b) => Value::Bool(b),
            scale_value::Primitive::Char(c) => Value::String(c.to_string()),
            scale_value::Primitive::String(s) => Value::String(s),
            scale_value::Primitive::U128(n) => Value::String(n.to_string()),
            scale_value::Primitive::I128(n) => Value::String(n.to_string()),
            scale_value::Primitive::U256(n) => Value::String(format!("{:?}", n)),
            scale_value::Primitive::I256(n) => Value::String(format!("{:?}", n)),
        },
        scale_value::ValueDef::BitSequence(bits) => {
            // Convert bit sequence to hex string
            let bytes: Vec<u8> = bits
                .iter()
                .collect::<Vec<_>>()
                .chunks(8)
                .map(|chunk| {
                    chunk
                        .iter()
                        .enumerate()
                        .fold(0u8, |acc, (i, &bit)| acc | ((bit as u8) << i))
                })
                .collect();
            Value::String(format!("0x{}", hex::encode(bytes)))
        }
    }
}

/// Apply SS58 encoding to account addresses in JSON.
/// This function does NOT unwrap arrays - it only converts hex addresses to SS58 format.
/// Use this after scale_value_to_json to apply SS58 encoding.
pub fn apply_ss58_encoding(value: Value, ss58_prefix: u16) -> Value {
    match value {
        Value::String(s) => {
            // Check if this looks like a 32-byte hex address (0x + 64 hex chars)
            if s.starts_with("0x") && (s.len() == 66 || s.len() == 68) {
                if let Some(ss58) = crate::utils::decode_address_to_ss58(&s, ss58_prefix) {
                    return Value::String(ss58);
                }
            }
            Value::String(s)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|v| apply_ss58_encoding(v, ss58_prefix))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, apply_ss58_encoding(v, ss58_prefix)))
                .collect(),
        ),
        other => other,
    }
}

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

            // Regular object: transform keys from snake_case to camelCase and recurse
            let transformed: serde_json::Map<String, Value> = map
                .into_iter()
                .map(|(key, val)| {
                    let camel_key = key.to_lower_camel_case();
                    (camel_key, transform_json_unified(val, ss58_prefix))
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
