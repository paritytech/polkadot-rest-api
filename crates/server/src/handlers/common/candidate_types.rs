// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared SCALE-decoded types for the `CandidateIncluded` event emitted by the
//! `ParaInclusion` pallet.
//!
//! These structs are used by:
//! - `utils::rc_block` — finding Asset Hub blocks included in a Relay Chain block.
//! - `handlers::blocks::get_block_para_inclusions` — the `/blocks/{blockId}/para-inclusions` endpoint.
//! - `handlers::paras::paras_inclusion` — the `/paras/{paraId}/inclusion` endpoint.

use scale_decode::DecodeAsType;

/// `CandidateIncluded` event fields decoded positionally via
/// [`decode_fields_unchecked_as`](subxt::events::EventDetails::decode_fields_unchecked_as).
///
/// All four fields must be present because `decode_fields_unchecked_as` uses
/// positional (index-based) decoding at the top level.
#[derive(Debug, DecodeAsType)]
pub struct CandidateIncludedEvent {
    pub candidate: CommittedCandidateReceipt,
    pub head_data: Vec<u8>,
    pub core_index: u32,
    pub group_index: u32,
}

/// `CommittedCandidateReceipt` — the first field of `CandidateIncluded`.
///
/// Inner structs are decoded via `DecodeAsType`'s named-field matching, so
/// missing fields are simply skipped. We include `commitments_hash` because the
/// `/blocks/{blockId}/para-inclusions` endpoint exposes it.
#[derive(Debug, DecodeAsType)]
pub struct CommittedCandidateReceipt {
    pub descriptor: CandidateDescriptorDecoded,
    pub commitments_hash: [u8; 32],
}

/// `CandidateDescriptor` — contains the parachain ID and various hashes.
///
/// The on-chain type also has `collator` and `signature` fields that are
/// automatically skipped by `DecodeAsType`'s named-field matching.
#[derive(Debug, DecodeAsType)]
pub struct CandidateDescriptorDecoded {
    pub para_id: u32,
    pub relay_parent: [u8; 32],
    pub persisted_validation_data_hash: [u8; 32],
    pub pov_hash: [u8; 32],
    pub erasure_root: [u8; 32],
    pub para_head: [u8; 32],
    pub validation_code_hash: [u8; 32],
}
