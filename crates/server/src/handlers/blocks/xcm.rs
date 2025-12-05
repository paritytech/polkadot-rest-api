//! XCM message decoding for block extrinsics.

use super::types::{ExtrinsicInfo, XcmMessages};
use config::ChainType;

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
    /// Looks for `paraInherent.enter` and extracts upward/horizontal messages.
    fn decode_relay_messages(&self) -> XcmMessages {
        // TODO: implement relay chain message extraction
        XcmMessages::default()
    }

    /// Decode XCM messages from parachain extrinsics.
    /// Looks for `parachainSystem.setValidationData` and extracts downward/horizontal messages.
    fn decode_parachain_messages(&self) -> XcmMessages {
        // TODO: implement parachain message extraction
        XcmMessages::default()
    }
}
