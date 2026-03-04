// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::AppState;
use crate::utils::ResolvedBlock;
use scale_decode::DecodeAsType;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

const ASSET_HUB_PARA_ID: u32 = 1000;
pub type RcClientAtBlock = OnlineClientAtBlock<SubstrateConfig>;

// ============================================================================
// SCALE Decode Types for CandidateIncluded event
// ============================================================================

/// CandidateIncluded event fields
#[derive(Debug, DecodeAsType)]
struct CandidateIncludedEvent {
    candidate: CommittedCandidateReceipt,
    head_data: Vec<u8>,
}

/// CommittedCandidateReceipt — only descriptor is needed
#[derive(Debug, DecodeAsType)]
struct CommittedCandidateReceipt {
    descriptor: CandidateDescriptorDecoded,
}

/// CandidateDescriptor — only para_id is needed for filtering.
/// Fields not listed here (collator, signature, relay_parent, etc.) are
/// automatically skipped by DecodeAsType's named-field matching.
#[derive(Debug, DecodeAsType)]
struct CandidateDescriptorDecoded {
    para_id: u32,
}

#[derive(Debug, Clone)]
pub struct AhBlockInfo {
    pub hash: String,
    pub number: u64,
}

#[derive(Debug, Error)]
pub enum RcBlockError {
    #[error("Block not found: {0}")]
    BlockNotFound(String),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[source] Box<subxt::error::OnlineClientAtBlockError>),

    #[error("Failed to fetch events")]
    EventsFetchFailed(String),

    #[error("Relay Chain client not available")]
    RelayChainClientNotAvailable,
}

impl From<super::AtBlockError> for RcBlockError {
    fn from(err: super::AtBlockError) -> Self {
        match err {
            super::AtBlockError::BlockNotFound(msg) => RcBlockError::BlockNotFound(msg),
            super::AtBlockError::Client(e) => RcBlockError::ClientAtBlockFailed(Box::new(e)),
        }
    }
}

impl From<subxt::error::OnlineClientAtBlockError> for RcBlockError {
    fn from(err: subxt::error::OnlineClientAtBlockError) -> Self {
        RcBlockError::from(super::AtBlockError::from(err))
    }
}

pub async fn find_ah_blocks_in_rc_block(
    state: &AppState,
    rc_block: &ResolvedBlock,
) -> Result<Vec<AhBlockInfo>, RcBlockError> {
    let rc_client = state
        .get_relay_chain_client()
        .await
        .map_err(|_| RcBlockError::RelayChainClientNotAvailable)?;

    let rc_client_at_block = rc_client.at_block(rc_block.number).await?;

    find_ah_blocks_in_rc_block_at(&rc_client_at_block).await
}

/// Find Asset Hub blocks included in a Relay Chain block.
///
/// Uses Subxt ClientAtBlock directly - callers should use `at_block()` to get
/// the client at the desired RC block, then pass it to this function.
/// This avoids an extra RPC call when you already have the ClientAtBlock.
pub async fn find_ah_blocks_in_rc_block_at(
    rc_client_at_block: &RcClientAtBlock,
) -> Result<Vec<AhBlockInfo>, RcBlockError> {
    use sp_runtime::traits::BlakeTwo256;
    use sp_runtime::traits::Hash as HashT;

    let events = rc_client_at_block
        .events()
        .fetch()
        .await
        .map_err(|e| RcBlockError::EventsFetchFailed(e.to_string()))?;

    let mut ah_blocks = Vec::new();

    for event_result in events.iter() {
        let event = match event_result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to decode event: {:?}", e);
                continue;
            }
        };

        let pallet_name = event.pallet_name();
        let event_name = event.event_name();

        if !pallet_name.to_lowercase().contains("parainclusion") {
            continue;
        }

        if event_name != "CandidateIncluded" {
            continue;
        }

        let event_fields: CandidateIncludedEvent = match event.decode_fields_unchecked_as() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to decode CandidateIncluded event fields: {:?}", e);
                continue;
            }
        };

        let para_id = event_fields.candidate.descriptor.para_id;

        if para_id != ASSET_HUB_PARA_ID {
            continue;
        }

        let head_data_bytes = &event_fields.head_data;
        let block_number = match extract_block_number_from_header(head_data_bytes) {
            Some(n) => n,
            None => {
                tracing::debug!("Failed to extract block number from head data");
                continue;
            }
        };

        let block_hash = BlakeTwo256::hash(head_data_bytes);
        let block_hash_hex = format!("0x{}", hex::encode(block_hash.as_ref()));

        ah_blocks.push(AhBlockInfo {
            hash: block_hash_hex,
            number: block_number,
        });
    }

    Ok(ah_blocks)
}

pub fn extract_block_number_from_header(header_bytes: &[u8]) -> Option<u64> {
    use parity_scale_codec::Decode;

    if header_bytes.len() < 32 {
        return None;
    }

    let mut cursor = &header_bytes[32..];

    let number_compact = parity_scale_codec::Compact::<u32>::decode(&mut cursor).ok()?;
    Some(number_compact.0 as u64)
}
