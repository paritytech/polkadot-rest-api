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

// ================================================================================================
// Type-Aware JSON Visitor
// ================================================================================================

use scale_decode::visitor::{self, TypeIdFor};
use sp_core::crypto::Ss58Codec;

/// A visitor that decodes SCALE values directly to JSON with type-aware transformations.
///
/// This handles:
/// - SS58 encoding ONLY for AccountId32/MultiAddress/AccountId types (not hashes)
/// - Preserving arrays for sequence types (Vec<T>) - never unwraps single-element sequences
/// - Unwrapping newtype wrappers (single unnamed field composites)
/// - Converting byte arrays to hex strings
/// - Transforming enum variants to {"variantName": value} format
/// - Converting all numbers to strings (matching sidecar behavior)
///
/// The key advantage over `transform_json_unified` is that this visitor has access to
/// type information at every nesting level, allowing it to make correct decisions about
/// SS58 encoding and array unwrapping.
pub struct JsonVisitor<R> {
    ss58_prefix: u16,
    _marker: core::marker::PhantomData<R>,
}

impl<R> JsonVisitor<R> {
    pub fn new(ss58_prefix: u16) -> Self {
        JsonVisitor {
            ss58_prefix,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> scale_decode::Visitor for JsonVisitor<R>
where
    R: scale_type_resolver::TypeResolver,
{
    type Value<'scale, 'resolver> = Value;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_bool<'scale, 'resolver>(
        self,
        value: bool,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::Bool(value))
    }

    fn visit_char<'scale, 'resolver>(
        self,
        value: char,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u8<'scale, 'resolver>(
        self,
        value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u16<'scale, 'resolver>(
        self,
        value: u16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u32<'scale, 'resolver>(
        self,
        value: u32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u64<'scale, 'resolver>(
        self,
        value: u64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u128<'scale, 'resolver>(
        self,
        value: u128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_u256<'resolver>(
        self,
        value: &[u8; 32],
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'_, 'resolver>, Self::Error> {
        // Convert to hex for u256
        Ok(Value::String(format!("0x{}", hex::encode(value))))
    }

    fn visit_i8<'scale, 'resolver>(
        self,
        value: i8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i16<'scale, 'resolver>(
        self,
        value: i16,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i32<'scale, 'resolver>(
        self,
        value: i32,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i64<'scale, 'resolver>(
        self,
        value: i64,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i128<'scale, 'resolver>(
        self,
        value: i128,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_i256<'resolver>(
        self,
        value: &[u8; 32],
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'_, 'resolver>, Self::Error> {
        Ok(Value::String(format!("0x{}", hex::encode(value))))
    }

    fn visit_sequence<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Sequence<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut items = Vec::new();
        while let Some(item) = value.decode_item(JsonVisitor::new(self.ss58_prefix)) {
            items.push(item?);
        }

        // Check if this is a Vec<u8> - all items are string representations of bytes
        if items.len() >= 2 {
            let mut is_byte_sequence = true;
            let mut bytes = Vec::with_capacity(items.len());

            for item in &items {
                if let Value::String(s) = item {
                    if let Ok(n) = s.parse::<u64>() {
                        if n <= 255 {
                            bytes.push(n as u8);
                            continue;
                        }
                    }
                }
                is_byte_sequence = false;
                break;
            }

            if is_byte_sequence && bytes.len() == items.len() {
                return Ok(Value::String(format!("0x{}", hex::encode(&bytes))));
            }
        }

        Ok(Value::Array(items))
    }

    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let path_segments: Vec<_> = value.path().collect();
        let is_account_type = path_segments.iter().any(|s| {
            *s == "AccountId32" || *s == "MultiAddress" || *s == "AccountId"
        });

        // If it's an account type, try to extract bytes and convert to SS58
        // Otherwise fall through to regular composite handling
        if is_account_type {
            let mut bytes = Vec::new();
            let field_count = value.remaining();

            // For AccountId32, it's typically a single unnamed field containing 32 bytes
            // For MultiAddress, it's an enum (handled in visit_variant)
            if field_count > 0 {
                for field in value.by_ref() {
                    let field = field?;
                    match field.decode_with_visitor(ByteCollector::<R>::new()) {
                        Ok(field_bytes) => bytes.extend(field_bytes),
                        Err(_) => {
                            bytes.clear();
                            break;
                        }
                    }
                }
            }

            // If we got exactly 32 bytes, convert to SS58
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                let account_id = AccountId32::from(arr);
                let ss58 = account_id.to_ss58check_with_version(self.ss58_prefix.into());
                return Ok(Value::String(ss58));
            }
        }

        // Check if all fields are unnamed (tuple/array-like)
        let fields: Vec<_> = value.collect::<Result<Vec<_>, _>>()?;

        if fields.is_empty() {
            return Ok(Value::Null);
        }

        // We check if the field is named or unnamed
        if fields[0].name().is_some() {
            // Deal with named fields and return a JSON object
            let mut map = serde_json::Map::new();
            for field in fields {
                let key = field.name().unwrap().to_lower_camel_case();
                let val = field.decode_with_visitor(JsonVisitor::new(self.ss58_prefix))?;
                map.insert(key, val);
            }
            Ok(Value::Object(map))
        } else {
            let field_count = fields.len();
            if field_count >= 2 {
                let mut is_byte_array = true;
                let mut bytes = Vec::with_capacity(field_count);

                for field in &fields {
                    match field.clone().decode_with_visitor(ByteValueVisitor::<R>::new()) {
                        Ok(Some(byte)) => bytes.push(byte),
                        _ => {
                            is_byte_array = false;
                            break;
                        }
                    }
                }

                if is_byte_array && bytes.len() == field_count {
                    return Ok(Value::String(format!("0x{}", hex::encode(&bytes))));
                }
            }

            // We deal with a single unnamed field.
            // TODO: Unsafe unwrap?
            // Note: We already handled sequences in visit_sequence, so this is safe
            if field_count == 1 {
                return fields
                    .into_iter()
                    .next()
                    .unwrap()
                    .decode_with_visitor(JsonVisitor::new(self.ss58_prefix));
            }

            // Deal with multiple unnamed fields
            let arr: Result<Vec<_>, _> = fields
                .into_iter()
                .map(|f| f.decode_with_visitor(JsonVisitor::new(self.ss58_prefix)))
                .collect();
            Ok(Value::Array(arr?))
        }
    }

    fn visit_variant<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Variant<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let name = value.name();

        // Handle Option::None as JSON null
        if name == "None" {
            // Consume the fields even though we're returning null
            for field in value.fields() {
                let _ = field?.decode_with_visitor(SkipVisitor::<R>::new())?;
            }
            return Ok(Value::Null);
        }

        let is_junction = is_junction_variant(name);
        // Convert variant name, ex: "PreRuntime" -> "preRuntime"
        let variant_name = crate::utils::lowercase_first_char(name);
        let fields: Vec<_> = value.fields().collect::<Result<Vec<_>, _>>()?;

        let inner = if fields.is_empty() {
            Value::Null
        } else if fields[0].name().is_some() {
            // Deal with named fields
            let mut map = serde_json::Map::new();
            for field in fields {
                let key = field.name().unwrap().to_lower_camel_case();
                let val = field.decode_with_visitor(JsonVisitor::new(self.ss58_prefix))?;
                map.insert(key, val);
            }
            Value::Object(map)
        } else if fields.len() == 1 && !is_junction {
            // Deal with a single unnamed field
            fields
                .into_iter()
                .next()
                .unwrap()
                .decode_with_visitor(JsonVisitor::new(self.ss58_prefix))?
        } else {
            // Deal with multiple unnamed fields or Junction types
            let arr: Result<Vec<_>, _> = fields
                .into_iter()
                .map(|f| f.decode_with_visitor(JsonVisitor::new(self.ss58_prefix)))
                .collect();
            Value::Array(arr?)
        };

        let mut map = serde_json::Map::new();
        map.insert(variant_name, inner);
        Ok(Value::Object(map))
    }

    fn visit_array<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Array<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {

        // Decode all elements first with JsonVisitor
        let mut items = Vec::new();
        while let Some(item) = value.decode_item(JsonVisitor::new(self.ss58_prefix)) {
            items.push(item?);
        }

        // Check if all items are string representations of u8 values (0-255)
        // This happens when we have a fixed-size byte array [u8; N]
        if items.len() >= 2 {
            let mut is_byte_array = true;
            let mut bytes = Vec::with_capacity(items.len());

            for item in &items {
                if let Value::String(s) = item {
                    if let Ok(n) = s.parse::<u64>() {
                        if n <= 255 {
                            bytes.push(n as u8);
                            continue;
                        }
                    }
                }
                is_byte_array = false;
                break;
            }

            if is_byte_array && bytes.len() == items.len() {
                return Ok(Value::String(format!("0x{}", hex::encode(&bytes))));
            }
        }

        Ok(Value::Array(items))
    }

    fn visit_tuple<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Tuple<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut items = Vec::new();
        while let Some(item) = value.decode_item(JsonVisitor::new(self.ss58_prefix)) {
            items.push(item?);
        }

        // TODO: We should handle this rouge unwrap.
        if items.len() == 1 {
            return Ok(items.into_iter().next().unwrap());
        }

        Ok(Value::Array(items))
    }

    fn visit_str<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Str<'scale>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Value::String(value.as_str()?.to_string()))
    }

    fn visit_bitsequence<'scale, 'resolver>(
        self,
        value: &mut visitor::types::BitSequence<'scale>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let bits: Vec<bool> = value
            .decode()?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| scale_decode::Error::custom(e))?;
        let bytes: Vec<u8> = bits
            .chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0u8, |acc, (i, &bit)| acc | ((bit as u8) << i))
            })
            .collect();
        Ok(Value::String(format!("0x{}", hex::encode(bytes))))
    }
}

/// Helper visitor that collects bytes from a composite (for AccountId32 extraction)
struct ByteCollector<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> ByteCollector<R> {
    fn new() -> Self {
        ByteCollector {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> scale_decode::Visitor for ByteCollector<R>
where
    R: scale_type_resolver::TypeResolver,
{
    type Value<'scale, 'resolver> = Vec<u8>;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_u8<'scale, 'resolver>(
        self,
        value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(vec![value])
    }

    fn visit_array<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Array<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut bytes = Vec::new();
        while let Some(item) = value.decode_item(ByteCollector::<R>::new()) {
            bytes.extend(item?);
        }
        Ok(bytes)
    }

    fn visit_composite<'scale, 'resolver>(
        self,
        value: &mut visitor::types::Composite<'scale, 'resolver, Self::TypeResolver>,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        let mut bytes = Vec::new();
        for field in value {
            let field = field?;
            bytes.extend(field.decode_with_visitor(ByteCollector::<R>::new())?);
        }
        Ok(bytes)
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: visitor::Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        // Return empty vec instead of error for unexpected types
        // TODO: Maybe we should at the very least have some warning here
        // with some detailed info.
        Ok(Vec::new())
    }
}

/// Helper visitor that checks if a value is a single u8 byte
struct ByteValueVisitor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> ByteValueVisitor<R> {
    fn new() -> Self {
        ByteValueVisitor {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> scale_decode::Visitor for ByteValueVisitor<R>
where
    R: scale_type_resolver::TypeResolver,
{
    type Value<'scale, 'resolver> = Option<u8>;
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_u8<'scale, 'resolver>(
        self,
        value: u8,
        _type_id: TypeIdFor<Self>,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(Some(value))
    }

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: visitor::Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(None)
    }
}

/// Helper visitor that skips/ignores a value (for consuming Option::None fields)
struct SkipVisitor<R> {
    _marker: core::marker::PhantomData<R>,
}

impl<R> SkipVisitor<R> {
    fn new() -> Self {
        SkipVisitor {
            _marker: core::marker::PhantomData,
        }
    }
}

impl<R> scale_decode::Visitor for SkipVisitor<R>
where
    R: scale_type_resolver::TypeResolver,
{
    type Value<'scale, 'resolver> = ();
    type Error = scale_decode::Error;
    type TypeResolver = R;

    fn visit_unexpected<'scale, 'resolver>(
        self,
        _unexpected: visitor::Unexpected,
    ) -> Result<Self::Value<'scale, 'resolver>, Self::Error> {
        Ok(())
    }
}
