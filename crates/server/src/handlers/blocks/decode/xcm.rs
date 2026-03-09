// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! XCM message decoding for block extrinsics.
//!
//! This module provides:
//! - `XcmDecoder` for extracting and decoding XCM messages from extrinsics
//! - `scale_value_to_json` for registry-aware conversion of SCALE values to JSON

use heck::ToLowerCamelCase;
use scale_info::{PortableRegistry, TypeDef};
use scale_value::scale::decode_as_type;
use serde_json::Value;

use super::super::types::{
    DownwardMessage, ExtrinsicInfo, HorizontalMessage, UpwardMessage, XcmMessages,
};
use polkadot_rest_api_config::ChainType;

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

/// Check if variant name is an X1-X8 junction.
/// These variants need special handling to preserve array output format.
/// Note: X1 is included here (unlike args.rs) because decoded XCM messages
/// represent X1 as an array to match sidecar's output format for XCM instructions.
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
                    match fields_vec.into_iter().next() {
                        Some(field) => scale_value_to_json(field, registry),
                        None => Value::Null,
                    }
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
                        // Single unnamed field - recurse into it
                        match fields_vec.into_iter().next() {
                            Some(field) => scale_value_to_json(field, registry),
                            None => Value::Null,
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
// XCM Decoder
// ================================================================================================

/// Build a portable registry containing just the VersionedXcm type
fn build_xcm_registry() -> (PortableRegistry, u32) {
    let mut registry = scale_info::Registry::new();
    let type_id = registry.register_type(&scale_info::meta_type::<staging_xcm::VersionedXcm<()>>());
    (registry.into(), type_id.id)
}

/// Decode a hex-encoded XCM message into a JSON value.
/// Returns the decoded XCM instructions if successful, or the raw hex string if decoding fails.
fn decode_xcm_message(hex_str: &str) -> Value {
    let hex_clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let Ok(bytes) = hex::decode(hex_clean) else {
        return Value::String(hex_str.to_string());
    };

    // Build registry with VersionedXcm type
    let (registry, type_id) = build_xcm_registry();

    // Decode using scale-value for proper JSON serialization
    match decode_as_type(&mut &bytes[..], type_id, &registry) {
        Ok(value) => {
            // Wrap in array to match sidecar format: "data": [{ "v4": [...] }]
            Value::Array(vec![scale_value_to_json(value, &registry)])
        }
        Err(_) => Value::String(hex_str.to_string()),
    }
}

/// Decodes XCM messages from block extrinsics.
pub struct XcmDecoder<'a> {
    chain_type: ChainType,
    extrinsics: &'a [ExtrinsicInfo],
    para_id_filter: Option<u32>,
}

impl<'a> XcmDecoder<'a> {
    pub fn new(
        chain_type: ChainType,
        extrinsics: &'a [ExtrinsicInfo],
        para_id_filter: Option<u32>,
    ) -> Self {
        Self {
            chain_type,
            extrinsics,
            para_id_filter,
        }
    }

    /// Decode XCM messages from the extrinsics.
    pub fn decode(&self) -> XcmMessages {
        match self.chain_type {
            ChainType::Relay => self.decode_relay_messages(),
            ChainType::Parachain | ChainType::AssetHub | ChainType::Coretime => {
                self.decode_parachain_messages()
            }
        }
    }

    /// Decode XCM messages from relay chain extrinsics.
    /// Looks for `paraInherent.enter` and extracts upward/horizontal messages from backedCandidates.
    fn decode_relay_messages(&self) -> XcmMessages {
        let mut messages = XcmMessages::default();

        for extrinsic in self.extrinsics {
            if extrinsic.method.pallet != "paraInherent" || extrinsic.method.method != "enter" {
                continue;
            }

            let Some(data) = extrinsic.args.get("data") else {
                continue;
            };

            let Some(backed_candidates) = data.get("backedCandidates").and_then(|v| v.as_array())
            else {
                continue;
            };

            for candidate in backed_candidates {
                let Some(candidate_obj) = candidate.get("candidate") else {
                    continue;
                };

                let para_id = candidate_obj
                    .get("descriptor")
                    .and_then(|d| d.get("paraId"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("0");

                if self
                    .para_id_filter
                    .is_some_and(|filter| para_id != filter.to_string())
                {
                    continue;
                }

                let Some(commitments) = candidate_obj.get("commitments") else {
                    continue;
                };

                // Extract upward messages
                // upwardMessages can be either:
                // 1. An array of hex strings (when there are multiple messages or empty)
                // 2. A single hex string (when there's one message - this is how subxt decodes it)
                if let Some(upward_value) = commitments.get("upwardMessages") {
                    if let Some(msg_data) = upward_value.as_str() {
                        // Single hex string - decode it directly
                        if !msg_data.is_empty() && msg_data != "0x" {
                            messages.upward_messages.push(UpwardMessage {
                                origin_para_id: para_id.to_string(),
                                data: decode_xcm_message(msg_data),
                            });
                        }
                    } else if let Some(upward_msgs) = upward_value.as_array() {
                        // Array of hex strings
                        for msg in upward_msgs {
                            if let Some(msg_data) = msg.as_str()
                                && !msg_data.is_empty()
                            {
                                messages.upward_messages.push(UpwardMessage {
                                    origin_para_id: para_id.to_string(),
                                    data: decode_xcm_message(msg_data),
                                });
                            }
                        }
                    }
                }

                // Extract horizontal messages
                if let Some(horizontal_msgs) = commitments
                    .get("horizontalMessages")
                    .and_then(|v| v.as_array())
                {
                    for msg in horizontal_msgs {
                        let recipient =
                            msg.get("recipient").and_then(|r| r.as_str()).unwrap_or("0");
                        let msg_data = msg.get("data").and_then(|d| d.as_str()).unwrap_or("");

                        if !msg_data.is_empty() {
                            messages.horizontal_messages.push(HorizontalMessage {
                                origin_para_id: para_id.to_string(),
                                destination_para_id: Some(recipient.to_string()),
                                sent_at: None,
                                data: decode_xcm_message(msg_data),
                            });
                        }
                    }
                }
            }
        }

        messages
    }

    /// Decode XCM messages from parachain extrinsics.
    /// Looks for `parachainSystem.setValidationData` and extracts downward/horizontal messages.
    fn decode_parachain_messages(&self) -> XcmMessages {
        let mut messages = XcmMessages::default();

        for extrinsic in self.extrinsics {
            if extrinsic.method.pallet != "parachainSystem"
                || extrinsic.method.method != "setValidationData"
            {
                continue;
            }

            let Some(data) = extrinsic.args.get("data") else {
                continue;
            };

            let Some(inbound_data) = data.get("inbound_messages_data") else {
                continue;
            };

            // Extract downward messages
            if let Some(downward) = inbound_data.get("downwardMessages")
                && let Some(full_msgs) = downward.get("fullMessages").and_then(|v| v.as_array())
            {
                for msg in full_msgs {
                    let sent_at = msg
                        .get("sentAt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0")
                        .to_string();
                    let msg_hex = msg
                        .get("msg")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if !msg_hex.is_empty() {
                        messages.downward_messages.push(DownwardMessage {
                            sent_at,
                            msg: msg_hex.clone(),
                            data: decode_xcm_message(&msg_hex),
                        });
                    }
                }
            }

            // Extract horizontal messages
            if let Some(horizontal) = inbound_data.get("horizontalMessages")
                && let Some(full_msgs) = horizontal.get("fullMessages").and_then(|v| v.as_array())
            {
                for msg in full_msgs {
                    let sent_at = msg
                        .get("sentAt")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let origin_para_id = msg
                        .get("originParaId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0")
                        .to_string();
                    let msg_data = msg
                        .get("data")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Apply paraId filter if specified
                    if self
                        .para_id_filter
                        .is_some_and(|filter| origin_para_id != filter.to_string())
                    {
                        continue;
                    }

                    if !msg_data.is_empty() {
                        messages.horizontal_messages.push(HorizontalMessage {
                            origin_para_id,
                            destination_para_id: None, // Not available for parachain perspective
                            sent_at,
                            data: decode_xcm_message(&msg_data),
                        });
                    }
                }
            }
        }

        messages
    }
}
