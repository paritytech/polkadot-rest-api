// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! XCM message decoding for block extrinsics.
//!
//! This module provides:
//! - `XcmDecoder` for extracting and decoding XCM messages from extrinsics
//! - `scale_value_to_json` for registry-aware conversion of SCALE values to JSON

use std::sync::LazyLock;

use heck::ToLowerCamelCase;
use parity_scale_codec::Encode;
use polkadot_parachain_primitives::primitives::XcmpMessageFormat;
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

static XCM_REGISTRY: LazyLock<(PortableRegistry, u32)> = LazyLock::new(|| {
    let mut registry = scale_info::Registry::new();
    let type_id = registry.register_type(&scale_info::meta_type::<staging_xcm::VersionedXcm<()>>());
    (registry.into(), type_id.id)
});

/// XCMP format byte for `ConcatenatedVersionedXcm`, derived from the canonical enum.
/// Uses the SCALE-encoded discriminant of the first variant.
fn xcmp_format_concatenated_versioned_xcm() -> u8 {
    XcmpMessageFormat::ConcatenatedVersionedXcm.encode()[0]
}

/// Decode a hex-encoded XCM message into a JSON value.
/// Returns the decoded XCM instructions if successful, or the raw hex string if decoding fails.
fn decode_xcm_message(hex_str: &str) -> Value {
    let hex_clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let Ok(bytes) = hex::decode(hex_clean) else {
        return Value::String(hex_str.to_string());
    };

    if bytes.is_empty() {
        return Value::String(hex_str.to_string());
    }

    let (registry, type_id) = &*XCM_REGISTRY;

    // Try direct decode first
    if let Ok(value) = decode_as_type(&mut &bytes[..], *type_id, registry) {
        return Value::Array(vec![scale_value_to_json(value, registry)]);
    }

    // Strip XCMP ConcatenatedVersionedXcm prefix and decode concatenated messages.
    if bytes[0] == xcmp_format_concatenated_versioned_xcm() && bytes.len() > 1 {
        let payload = &bytes[1..];
        let mut decoded_messages = Vec::new();
        let mut remaining = payload;

        while !remaining.is_empty() {
            match decode_as_type(&mut remaining, *type_id, registry) {
                Ok(value) => {
                    decoded_messages.push(scale_value_to_json(value, registry));
                }
                Err(e) => {
                    tracing::debug!(
                        remaining_bytes = remaining.len(),
                        "Failed to decode concatenated XCM message: {e:?}"
                    );
                    break;
                }
            }
        }

        if !decoded_messages.is_empty() {
            return Value::Array(decoded_messages);
        }
    }

    // All decode attempts failed — return raw hex
    tracing::debug!("Failed to decode XCM message, returning raw hex");
    Value::String(hex_str.to_string())
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

            let Some(inbound_data) = extrinsic.args.get("inbound_messages_data") else {
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
                    // HRMP fullMessages are tuples: [originParaId, { sentAt, data }]
                    let (origin_para_id, sent_at, msg_data) = if let Some(tuple) = msg.as_array()
                        && tuple.len() == 2
                    {
                        let origin = tuple[0].as_str().unwrap_or("0").to_string();
                        let inner = &tuple[1];
                        let sent = inner
                            .get("sentAt")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let data = inner
                            .get("data")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        (origin, sent, data)
                    } else {
                        // Fallback: object format
                        let origin = msg
                            .get("originParaId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0")
                            .to_string();
                        let sent = msg
                            .get("sentAt")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let data = msg
                            .get("data")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        (origin, sent, data)
                    };

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
                            destination_para_id: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use parity_scale_codec::Encode;
    use staging_xcm::VersionedXcm;
    use staging_xcm::v4 as xcm_v4;
    use staging_xcm::v5 as xcm_v5;

    /// Helper: SCALE-encode a VersionedXcm and return it as a "0x"-prefixed hex string.
    fn encode_versioned_xcm(msg: VersionedXcm<()>) -> String {
        let bytes = msg.encode();
        format!("0x{}", hex::encode(bytes))
    }

    #[test]
    fn test_decode_v4_xcm_withdraw_and_deposit() {
        // Build a V4 XCM: WithdrawAsset + BuyExecution + DepositAsset
        let xcm = VersionedXcm::V4(xcm_v4::Xcm(vec![
            xcm_v4::Instruction::WithdrawAsset(
                vec![xcm_v4::Asset {
                    id: xcm_v4::AssetId(xcm_v4::Location::here()),
                    fun: xcm_v4::Fungibility::Fungible(1_000_000_000),
                }]
                .into(),
            ),
            xcm_v4::Instruction::BuyExecution {
                fees: xcm_v4::Asset {
                    id: xcm_v4::AssetId(xcm_v4::Location::here()),
                    fun: xcm_v4::Fungibility::Fungible(500_000_000),
                },
                weight_limit: xcm_v4::WeightLimit::Unlimited,
            },
            xcm_v4::Instruction::DepositAsset {
                assets: xcm_v4::AssetFilter::Wild(xcm_v4::WildAsset::AllCounted(1)),
                beneficiary: xcm_v4::Location {
                    parents: 0,
                    interior: xcm_v4::Junctions::X1(
                        [xcm_v4::Junction::AccountId32 {
                            network: None,
                            id: [1u8; 32],
                        }]
                        .into(),
                    ),
                },
            },
        ]));

        let hex_str = encode_versioned_xcm(xcm);
        let result = decode_xcm_message(&hex_str);

        // Should be an array with one element containing "v4" key
        let arr = result.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 1, "should have exactly one decoded message");
        let msg = arr[0].as_object().expect("message should be an object");
        assert!(
            msg.contains_key("v4"),
            "should contain 'v4' key, got keys: {:?}",
            msg.keys().collect::<Vec<_>>()
        );

        // The V4 value should be an array of 3 instructions
        let instructions = msg["v4"]
            .as_array()
            .expect("v4 should contain an array of instructions");
        assert_eq!(instructions.len(), 3, "should have 3 instructions");

        // Check instruction names
        let first = instructions[0].as_object().unwrap();
        assert!(
            first.contains_key("withdrawAsset"),
            "first instruction should be withdrawAsset"
        );
        let second = instructions[1].as_object().unwrap();
        assert!(
            second.contains_key("buyExecution"),
            "second instruction should be buyExecution"
        );
        let third = instructions[2].as_object().unwrap();
        assert!(
            third.contains_key("depositAsset"),
            "third instruction should be depositAsset"
        );
    }

    #[test]
    fn test_decode_v5_xcm_withdraw_and_deposit() {
        // Build a V5 XCM: WithdrawAsset + BuyExecution + DepositAsset
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![
            xcm_v5::Instruction::WithdrawAsset(
                vec![xcm_v5::Asset {
                    id: xcm_v5::AssetId(xcm_v5::Location::here()),
                    fun: xcm_v5::Fungibility::Fungible(1_000_000_000),
                }]
                .into(),
            ),
            xcm_v5::Instruction::BuyExecution {
                fees: xcm_v5::Asset {
                    id: xcm_v5::AssetId(xcm_v5::Location::here()),
                    fun: xcm_v5::Fungibility::Fungible(500_000_000),
                },
                weight_limit: xcm_v5::WeightLimit::Unlimited,
            },
            xcm_v5::Instruction::DepositAsset {
                assets: xcm_v5::AssetFilter::Wild(xcm_v5::WildAsset::AllCounted(1)),
                beneficiary: xcm_v5::Location {
                    parents: 0,
                    interior: xcm_v5::Junctions::X1(
                        [xcm_v5::Junction::AccountId32 {
                            network: None,
                            id: [1u8; 32],
                        }]
                        .into(),
                    ),
                },
            },
        ]));

        let hex_str = encode_versioned_xcm(xcm);
        let result = decode_xcm_message(&hex_str);

        // Should be an array with one element containing "v5" key
        let arr = result.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 1, "should have exactly one decoded message");
        let msg = arr[0].as_object().expect("message should be an object");
        assert!(
            msg.contains_key("v5"),
            "should contain 'v5' key, got keys: {:?}",
            msg.keys().collect::<Vec<_>>()
        );

        // The V5 value should be an array of 3 instructions
        let instructions = msg["v5"]
            .as_array()
            .expect("v5 should contain an array of instructions");
        assert_eq!(instructions.len(), 3, "should have 3 instructions");

        // Check instruction names
        let first = instructions[0].as_object().unwrap();
        assert!(
            first.contains_key("withdrawAsset"),
            "first instruction should be withdrawAsset"
        );
        let second = instructions[1].as_object().unwrap();
        assert!(
            second.contains_key("buyExecution"),
            "second instruction should be buyExecution"
        );
        let third = instructions[2].as_object().unwrap();
        assert!(
            third.contains_key("depositAsset"),
            "third instruction should be depositAsset"
        );
    }

    #[test]
    fn test_decode_v5_xcm_with_set_topic() {
        // V5 XCM with ClearOrigin and SetTopic (common pattern)
        let topic = [0xABu8; 32];
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![
            xcm_v5::Instruction::ClearOrigin,
            xcm_v5::Instruction::SetTopic(topic),
        ]));

        let hex_str = encode_versioned_xcm(xcm);
        let result = decode_xcm_message(&hex_str);

        let arr = result.as_array().unwrap();
        let msg = arr[0].as_object().unwrap();
        assert!(msg.contains_key("v5"), "should decode as v5");

        let instructions = msg["v5"].as_array().unwrap();
        assert_eq!(instructions.len(), 2);

        let first = instructions[0].as_object().unwrap();
        assert!(first.contains_key("clearOrigin"));
        let second = instructions[1].as_object().unwrap();
        assert!(second.contains_key("setTopic"));
    }

    #[test]
    fn test_decode_v5_xcm_reserve_asset_deposited() {
        // V5 XCM: ReserveAssetDeposited + ClearOrigin + BuyExecution + DepositAsset
        // This is a common pattern for cross-chain transfers
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![
            xcm_v5::Instruction::ReserveAssetDeposited(
                vec![xcm_v5::Asset {
                    id: xcm_v5::AssetId(xcm_v5::Location {
                        parents: 1,
                        interior: xcm_v5::Junctions::Here,
                    }),
                    fun: xcm_v5::Fungibility::Fungible(5_000_000_000),
                }]
                .into(),
            ),
            xcm_v5::Instruction::ClearOrigin,
            xcm_v5::Instruction::BuyExecution {
                fees: xcm_v5::Asset {
                    id: xcm_v5::AssetId(xcm_v5::Location {
                        parents: 1,
                        interior: xcm_v5::Junctions::Here,
                    }),
                    fun: xcm_v5::Fungibility::Fungible(500_000_000),
                },
                weight_limit: xcm_v5::WeightLimit::Unlimited,
            },
            xcm_v5::Instruction::DepositAsset {
                assets: xcm_v5::AssetFilter::Wild(xcm_v5::WildAsset::All),
                beneficiary: xcm_v5::Location {
                    parents: 0,
                    interior: xcm_v5::Junctions::X1(
                        [xcm_v5::Junction::AccountId32 {
                            network: None,
                            id: [2u8; 32],
                        }]
                        .into(),
                    ),
                },
            },
        ]));

        let hex_str = encode_versioned_xcm(xcm);
        let result = decode_xcm_message(&hex_str);

        let arr = result.as_array().unwrap();
        let msg = arr[0].as_object().unwrap();
        assert!(msg.contains_key("v5"));

        let instructions = msg["v5"].as_array().unwrap();
        assert_eq!(instructions.len(), 4);

        assert!(
            instructions[0]
                .as_object()
                .unwrap()
                .contains_key("reserveAssetDeposited")
        );
        assert!(
            instructions[1]
                .as_object()
                .unwrap()
                .contains_key("clearOrigin")
        );
        assert!(
            instructions[2]
                .as_object()
                .unwrap()
                .contains_key("buyExecution")
        );
        assert!(
            instructions[3]
                .as_object()
                .unwrap()
                .contains_key("depositAsset")
        );
    }

    #[test]
    fn test_decode_invalid_hex_returns_raw_string() {
        // Invalid hex should return the raw string
        let result = decode_xcm_message("not_valid_hex");
        assert_eq!(result, Value::String("not_valid_hex".to_string()));
    }

    #[test]
    fn test_decode_malformed_xcm_returns_raw_hex() {
        // Valid hex but not a valid XCM message - should return raw hex
        let result = decode_xcm_message("0xdeadbeef");
        // The decode should fail and return the raw hex string
        assert!(
            result.is_string(),
            "malformed XCM should return raw hex string"
        );
        assert_eq!(result.as_str().unwrap(), "0xdeadbeef");
    }

    #[test]
    fn test_v4_and_v5_produce_different_encodings() {
        // Same logical message encoded as V4 vs V5 should produce different hex
        // (because the version discriminant byte differs: V4=0x04, V5=0x05)
        let instructions_v4 = vec![xcm_v4::Instruction::ClearOrigin];
        let instructions_v5 = vec![xcm_v5::Instruction::ClearOrigin];

        let v4_hex = encode_versioned_xcm(VersionedXcm::V4(xcm_v4::Xcm(instructions_v4)));
        let v5_hex = encode_versioned_xcm(VersionedXcm::V5(xcm_v5::Xcm(instructions_v5)));

        assert_ne!(v4_hex, v5_hex, "V4 and V5 should have different encodings");

        // V4 should start with "0x04", V5 with "0x05"
        assert!(
            v4_hex.starts_with("0x04"),
            "V4 should start with 0x04, got {}",
            &v4_hex[..6]
        );
        assert!(
            v5_hex.starts_with("0x05"),
            "V5 should start with 0x05, got {}",
            &v5_hex[..6]
        );

        // Both should decode successfully
        let r4 = decode_xcm_message(&v4_hex);
        let r5 = decode_xcm_message(&v5_hex);

        let r4_obj = r4.as_array().unwrap()[0].as_object().unwrap();
        let r5_obj = r5.as_array().unwrap()[0].as_object().unwrap();
        assert!(r4_obj.contains_key("v4"));
        assert!(r5_obj.contains_key("v5"));
    }

    // Parachain decode path tests

    /// Build a minimal ExtrinsicInfo for parachainSystem.setValidationData.
    fn build_parachain_system_extrinsic(args: serde_json::Map<String, Value>) -> ExtrinsicInfo {
        use crate::handlers::blocks::types::MethodInfo;
        use crate::utils::EraInfo;

        ExtrinsicInfo {
            method: MethodInfo {
                pallet: "parachainSystem".to_string(),
                method: "setValidationData".to_string(),
            },
            signature: None,
            nonce: None,
            args,
            tip: None,
            hash: "0x00".to_string(),
            info: serde_json::Map::new(),
            era: EraInfo {
                immortal_era: Some("true".to_string()),
                mortal_era: None,
            },
            events: vec![],
            success: true,
            pays_fee: None,
            docs: None,
            raw_hex: String::new(),
        }
    }

    #[test]
    fn test_parachain_hrmp_tuple_format() {
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![xcm_v5::Instruction::ClearOrigin]));
        let hex_str = encode_versioned_xcm(xcm);

        let hrmp_msg = Value::Array(vec![
            Value::String("2034".to_string()),
            serde_json::json!({
                "sentAt": "30441355",
                "data": hex_str,
            }),
        ]);

        let mut args = serde_json::Map::new();
        args.insert(
            "data".to_string(),
            serde_json::json!({ "validationData": {} }),
        );
        args.insert(
            "inbound_messages_data".to_string(),
            serde_json::json!({
                "downwardMessages": { "fullMessages": [], "hashedMessages": [] },
                "horizontalMessages": { "fullMessages": [hrmp_msg], "hashedMessages": [] }
            }),
        );

        let extrinsics = vec![build_parachain_system_extrinsic(args)];
        let decoder = XcmDecoder::new(ChainType::AssetHub, &extrinsics, None);
        let result = decoder.decode();

        assert!(
            result.downward_messages.is_empty(),
            "should have no downward messages"
        );
        assert_eq!(
            result.horizontal_messages.len(),
            1,
            "should have 1 horizontal message"
        );

        let msg = &result.horizontal_messages[0];
        assert_eq!(msg.origin_para_id, "2034");
        assert_eq!(msg.sent_at, Some("30441355".to_string()));

        let data_arr = msg.data.as_array().expect("should decode to array");
        assert_eq!(data_arr.len(), 1);
        assert!(
            data_arr[0].as_object().unwrap().contains_key("v5"),
            "should decode as V5 XCM"
        );
    }

    #[test]
    fn test_parachain_inbound_messages_data_is_top_level_arg() {
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![xcm_v5::Instruction::ClearOrigin]));
        let hex_str = encode_versioned_xcm(xcm);

        let hrmp_msg = Value::Array(vec![
            Value::String("1000".to_string()),
            serde_json::json!({
                "sentAt": "100",
                "data": hex_str,
            }),
        ]);

        // Wrong: inbound_messages_data nested inside data — should find nothing
        let mut args_wrong = serde_json::Map::new();
        args_wrong.insert(
            "data".to_string(),
            serde_json::json!({
                "inbound_messages_data": {
                    "downwardMessages": { "fullMessages": [], "hashedMessages": [] },
                    "horizontalMessages": { "fullMessages": [hrmp_msg.clone()], "hashedMessages": [] }
                }
            }),
        );

        let extrinsics_wrong = vec![build_parachain_system_extrinsic(args_wrong)];
        let decoder = XcmDecoder::new(ChainType::AssetHub, &extrinsics_wrong, None);
        let result = decoder.decode();
        assert!(
            result.horizontal_messages.is_empty(),
            "nested inbound_messages_data under data should NOT be found"
        );

        // Correct: inbound_messages_data at top level
        let mut args_correct = serde_json::Map::new();
        args_correct.insert(
            "data".to_string(),
            serde_json::json!({ "validationData": {} }),
        );
        args_correct.insert(
            "inbound_messages_data".to_string(),
            serde_json::json!({
                "downwardMessages": { "fullMessages": [], "hashedMessages": [] },
                "horizontalMessages": { "fullMessages": [hrmp_msg], "hashedMessages": [] }
            }),
        );

        let extrinsics_correct = vec![build_parachain_system_extrinsic(args_correct)];
        let decoder = XcmDecoder::new(ChainType::AssetHub, &extrinsics_correct, None);
        let result = decoder.decode();
        assert_eq!(
            result.horizontal_messages.len(),
            1,
            "top-level inbound_messages_data should be found"
        );
    }

    #[test]
    fn test_parachain_para_id_filter_on_hrmp() {
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![xcm_v5::Instruction::ClearOrigin]));
        let hex_str = encode_versioned_xcm(xcm);

        let hrmp_msgs = vec![
            Value::Array(vec![
                Value::String("1000".to_string()),
                serde_json::json!({ "sentAt": "100", "data": &hex_str }),
            ]),
            Value::Array(vec![
                Value::String("2034".to_string()),
                serde_json::json!({ "sentAt": "101", "data": &hex_str }),
            ]),
        ];

        let mut args = serde_json::Map::new();
        args.insert("data".to_string(), serde_json::json!({}));
        args.insert(
            "inbound_messages_data".to_string(),
            serde_json::json!({
                "downwardMessages": { "fullMessages": [] },
                "horizontalMessages": { "fullMessages": hrmp_msgs }
            }),
        );

        let extrinsics = vec![build_parachain_system_extrinsic(args)];

        // Filter for para 2034 only
        let decoder = XcmDecoder::new(ChainType::Parachain, &extrinsics, Some(2034));
        let result = decoder.decode();
        assert_eq!(result.horizontal_messages.len(), 1);
        assert_eq!(result.horizontal_messages[0].origin_para_id, "2034");
    }

    // XCMP format prefix tests

    #[test]
    fn test_xcmp_format_prefix_stripped_for_hrmp() {
        let xcm = VersionedXcm::<()>::V5(xcm_v5::Xcm(vec![xcm_v5::Instruction::ClearOrigin]));
        let xcm_bytes = xcm.encode();

        // Prepend 0x00 format byte
        let mut prefixed = vec![0x00u8];
        prefixed.extend_from_slice(&xcm_bytes);
        let hex_str = format!("0x{}", hex::encode(&prefixed));

        let result = decode_xcm_message(&hex_str);

        let arr = result.as_array().expect("should decode to array");
        assert!(!arr.is_empty());
        assert!(arr[0].as_object().unwrap().contains_key("v5"));
    }

    #[test]
    fn test_xcmp_concatenated_multiple_messages() {
        let xcm1 = VersionedXcm::<()>::V5(xcm_v5::Xcm(vec![xcm_v5::Instruction::ClearOrigin]));
        let xcm2 = VersionedXcm::<()>::V5(xcm_v5::Xcm(vec![xcm_v5::Instruction::RefundSurplus]));

        let mut prefixed = vec![0x00u8];
        prefixed.extend_from_slice(&xcm1.encode());
        prefixed.extend_from_slice(&xcm2.encode());
        let hex_str = format!("0x{}", hex::encode(&prefixed));

        let result = decode_xcm_message(&hex_str);

        let arr = result.as_array().expect("should decode to array");
        assert_eq!(arr.len(), 2);
        assert!(arr[0].as_object().unwrap().contains_key("v5"));
        assert!(arr[1].as_object().unwrap().contains_key("v5"));
    }

    #[test]
    fn test_direct_versioned_xcm_still_works() {
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![xcm_v5::Instruction::ClearOrigin]));
        let hex_str = encode_versioned_xcm(xcm);

        let result = decode_xcm_message(&hex_str);

        let arr = result.as_array().expect("should decode to array");
        assert_eq!(arr.len(), 1);
        assert!(arr[0].as_object().unwrap().contains_key("v5"));
    }

    #[test]
    fn test_real_asset_hub_hrmp_message() {
        // Real HRMP message from Hydration → Asset Hub block 13619496
        let hex_str = "0x00051400040002043205e5140007e72ce8f0020a130002043205e5140007e72ce8f002000d01020400010100\
            74a9accd4e9b0d530c7047e0ede0a6b1d1d8ba5ccc8827c47f09ffa7fe95c33c2c92c56616c34b1e62b561ae924cb1623aa4325b89d1bce94de1bdd2d4f7506c20";

        let result = decode_xcm_message(hex_str);

        let arr = result.as_array().expect("should decode to array");
        assert!(!arr.is_empty());
        let msg = arr[0].as_object().expect("should be an object");
        assert!(msg.contains_key("v5"));

        let instructions = msg["v5"]
            .as_array()
            .expect("v5 should be an array of instructions");
        assert!(!instructions.is_empty());
    }

    #[test]
    fn test_build_xcm_registry_contains_versioned_xcm() {
        let (registry, type_id) = &*XCM_REGISTRY;
        let ty = registry
            .resolve(*type_id)
            .expect("VersionedXcm type should be in registry");
        // VersionedXcm is an enum with V3, V4, V5 variants
        match &ty.type_def {
            scale_info::TypeDef::Variant(v) => {
                let names: Vec<&str> = v.variants.iter().map(|v| v.name.as_str()).collect();
                assert!(names.contains(&"V3"), "missing V3 variant");
                assert!(names.contains(&"V4"), "missing V4 variant");
                assert!(names.contains(&"V5"), "missing V5 variant");
            }
            _ => panic!("VersionedXcm should be a Variant type"),
        }
    }

    #[test]
    fn test_decode_v3_xcm() {
        use staging_xcm::v3::{self as xcm_v3, Instruction as V3Instruction};
        let xcm = VersionedXcm::<()>::V3(xcm_v3::Xcm(vec![V3Instruction::ClearOrigin]));
        let hex_str = format!("0x{}", hex::encode(xcm.encode()));
        let result = decode_xcm_message(&hex_str);
        let arr = result.as_array().expect("should decode to array");
        assert!(arr[0].as_object().unwrap().contains_key("v3"));
    }

    #[test]
    fn test_decode_empty_bytes_returns_raw_hex() {
        let result = decode_xcm_message("0x");
        assert!(result.is_string(), "empty bytes should return raw hex");
    }

    #[test]
    fn test_decode_non_xcm_discriminant_returns_raw_hex() {
        // 0x99 is not a valid VersionedXcm discriminant (3, 4, or 5)
        let result = decode_xcm_message("0x99aabbcc");
        assert!(
            result.is_string(),
            "invalid discriminant should return raw hex"
        );
    }

    #[test]
    fn test_xcmp_prefix_only_returns_raw_hex() {
        // Just the 0x00 prefix byte with nothing after it
        let result = decode_xcm_message("0x00");
        assert!(result.is_string(), "lone XCMP prefix should return raw hex");
    }

    #[test]
    fn test_decode_truncated_xcm_returns_raw_hex() {
        // Valid V5 discriminant but truncated payload
        let result = decode_xcm_message("0x0504");
        assert!(result.is_string(), "truncated XCM should return raw hex");
    }
}
