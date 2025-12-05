//! XCM message decoding for block extrinsics.

use super::types::{DownwardMessage, ExtrinsicInfo, HorizontalMessage, UpwardMessage, XcmMessages};
use config::ChainType;
use parity_scale_codec::Decode;
use serde_json::Value;
use staging_xcm::VersionedXcm;

/// Decode a hex-encoded XCM message into a JSON value.
/// Returns the decoded XCM instructions if successful, or the raw hex string if decoding fails.
fn decode_xcm_message(hex_str: &str) -> Value {
    // Strip 0x prefix if present
    let hex_clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    // Convert hex to bytes
    let Ok(bytes) = hex::decode(hex_clean) else {
        return Value::String(hex_str.to_string());
    };

    // Try to decode as VersionedXcm
    // Note: VersionedXcm<()> uses () as the Call type since we don't need to decode calls
    match VersionedXcm::<()>::decode(&mut &bytes[..]) {
        Ok(xcm) => {
            // Format the XCM as a debug string since VersionedXcm doesn't implement Serialize
            Value::String(format!("{:?}", xcm))
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

                // Get paraId from descriptor
                let para_id = candidate_obj
                    .get("descriptor")
                    .and_then(|d| d.get("paraId"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("0");

                // Apply paraId filter if specified
                if let Some(filter) = self.para_id_filter {
                    if para_id != filter.to_string() {
                        continue;
                    }
                }

                let Some(commitments) = candidate_obj.get("commitments") else {
                    continue;
                };

                // Extract upward messages
                if let Some(upward_msgs) =
                    commitments.get("upwardMessages").and_then(|v| v.as_array())
                {
                    for msg in upward_msgs {
                        if let Some(msg_data) = msg.as_str() {
                            if !msg_data.is_empty() {
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

            // Get inbound_messages_data (polkadot-rest-api structure)
            let Some(inbound_data) = data.get("inbound_messages_data") else {
                continue;
            };

            // Extract downward messages
            if let Some(downward) = inbound_data.get("downwardMessages") {
                // Check fullMessages array
                if let Some(full_msgs) = downward.get("fullMessages").and_then(|v| v.as_array()) {
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
            }

            // Extract horizontal messages
            if let Some(horizontal) = inbound_data.get("horizontalMessages") {
                // Check fullMessages array
                if let Some(full_msgs) = horizontal.get("fullMessages").and_then(|v| v.as_array()) {
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
                        if let Some(filter) = self.para_id_filter {
                            if origin_para_id != filter.to_string() {
                                continue;
                            }
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
        }

        messages
    }
}
