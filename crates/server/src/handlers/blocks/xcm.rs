//! XCM message decoding for block extrinsics.

use super::types::{ExtrinsicInfo, HorizontalMessage, UpwardMessage, XcmMessages};
use config::ChainType;
use serde_json::Value;

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
                if let Some(upward_msgs) = commitments.get("upwardMessages").and_then(|v| v.as_array()) {
                    for msg in upward_msgs {
                        if let Some(msg_data) = msg.as_str() {
                            if !msg_data.is_empty() {
                                messages.upward_messages.push(UpwardMessage {
                                    origin_para_id: para_id.to_string(),
                                    data: Value::String(msg_data.to_string()), // TODO: decode XCM in Phase 7
                                });
                            }
                        }
                    }
                }

                // Extract horizontal messages
                if let Some(horizontal_msgs) =
                    commitments.get("horizontalMessages").and_then(|v| v.as_array())
                {
                    for msg in horizontal_msgs {
                        let recipient = msg
                            .get("recipient")
                            .and_then(|r| r.as_str())
                            .unwrap_or("0");
                        let msg_data = msg.get("data").and_then(|d| d.as_str()).unwrap_or("");

                        if !msg_data.is_empty() {
                            messages.horizontal_messages.push(HorizontalMessage {
                                origin_para_id: para_id.to_string(),
                                destination_para_id: Some(recipient.to_string()),
                                sent_at: None,
                                data: Value::String(msg_data.to_string()), // TODO: decode XCM in Phase 7
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
        // TODO: implement parachain message extraction
        XcmMessages::default()
    }
}
