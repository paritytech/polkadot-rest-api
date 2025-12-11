//! XCM message decoding for block extrinsics.

use super::transform::scale_value_to_json;
use super::types::{DownwardMessage, ExtrinsicInfo, HorizontalMessage, UpwardMessage, XcmMessages};
use config::ChainType;
use scale_info::PortableRegistry;
use scale_value::scale::decode_as_type;
use serde_json::Value;

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
            ChainType::Parachain | ChainType::AssetHub => self.decode_parachain_messages(),
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
