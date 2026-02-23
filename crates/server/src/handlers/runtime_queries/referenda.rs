// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Referenda pallet storage query functions.
//!
//! This module provides standalone functions for querying Referenda pallet storage items.

use futures::future::join_all;
use scale_decode::DecodeAsType;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// SCALE Decode Types
// ================================================================================================

/// Referendum status enum - we only care about Ongoing variant
#[derive(Debug, DecodeAsType)]
pub enum ReferendumStatus {
    Ongoing(Box<OngoingDetails>),
    #[allow(dead_code)]
    Approved(u32, Option<DepositDetails>, Option<DepositDetails>),
    #[allow(dead_code)]
    Rejected(u32, Option<DepositDetails>, Option<DepositDetails>),
    #[allow(dead_code)]
    Cancelled(u32, Option<DepositDetails>, Option<DepositDetails>),
    #[allow(dead_code)]
    TimedOut(u32, Option<DepositDetails>, Option<DepositDetails>),
    #[allow(dead_code)]
    Killed(u32),
}

/// Details for ongoing referenda - extract only what we need
#[derive(Debug, DecodeAsType)]
pub struct OngoingDetails {
    pub track: u16,
    #[allow(dead_code)]
    pub origin: scale_value::Value<()>,
    #[allow(dead_code)]
    pub proposal: scale_value::Value<()>,
    pub enactment: EnactmentType,
    pub submitted: u32,
    pub decision_deposit: Option<DepositDetails>,
    #[allow(dead_code)]
    pub submission_deposit: DepositDetails,
    pub deciding: Option<DecidingDetails>,
    #[allow(dead_code)]
    pub tally: scale_value::Value<()>,
    #[allow(dead_code)]
    pub in_queue: bool,
    #[allow(dead_code)]
    pub alarm: Option<scale_value::Value<()>>,
}

/// Enactment type enum
#[derive(Debug, DecodeAsType)]
pub enum EnactmentType {
    After(u32),
    At(u32),
}

/// Deposit details
#[derive(Debug, DecodeAsType)]
pub struct DepositDetails {
    pub who: [u8; 32],
    pub amount: u128,
}

/// Deciding status details
#[derive(Debug, DecodeAsType)]
pub struct DecidingDetails {
    pub since: u32,
    pub confirming: Option<u32>,
}

// ================================================================================================
// Decoded Result Types
// ================================================================================================

/// Decoded referendum info for ongoing referenda
#[derive(Debug, Clone)]
pub struct DecodedOngoingReferendum {
    pub id: u32,
    pub track: u16,
    pub enactment: DecodedEnactment,
    pub submitted: u32,
    pub decision_deposit: Option<DecodedDeposit>,
    pub deciding: Option<DecodedDeciding>,
}

#[derive(Debug, Clone)]
pub enum DecodedEnactment {
    After(u32),
    At(u32),
}

#[derive(Debug, Clone)]
pub struct DecodedDeposit {
    pub who: [u8; 32],
    pub amount: u128,
}

#[derive(Debug, Clone)]
pub struct DecodedDeciding {
    pub since: u32,
    pub confirming: Option<u32>,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Fetch a single referendum by ID from Referenda::ReferendumInfoFor storage.
pub async fn get_referendum_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    referendum_id: u32,
) -> Option<ReferendumStatus> {
    let storage_addr =
        subxt::dynamic::storage::<_, ReferendumStatus>("Referenda", "ReferendumInfoFor");

    let result = client_at_block
        .storage()
        .fetch(storage_addr, (referendum_id,))
        .await;

    match result {
        Ok(val) => val.decode().ok(),
        Err(_) => None,
    }
}

/// Fetch all ongoing referenda in a batch.
/// Returns a vector of (referendum_id, ReferendumStatus) pairs.
pub async fn iter_referenda_batch(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    start_id: u32,
    end_id: u32,
) -> Vec<(u32, Option<ReferendumStatus>)> {
    let futures: Vec<_> = (start_id..=end_id)
        .map(|ref_id| {
            let storage_addr =
                subxt::dynamic::storage::<_, ReferendumStatus>("Referenda", "ReferendumInfoFor");
            let client = client_at_block.clone();
            async move {
                let result = client.storage().fetch(storage_addr, (ref_id,)).await;
                let decoded: Option<ReferendumStatus> = match result {
                    Ok(val) => val.decode().ok(),
                    Err(_) => None,
                };
                (ref_id, decoded)
            }
        })
        .collect();

    join_all(futures).await
}

/// Extract ongoing referendum info from decoded ReferendumStatus.
/// Returns Some((track, DecodedOngoingReferendum)) if the referendum is ongoing.
pub fn extract_ongoing_referendum(
    status: ReferendumStatus,
    id: u32,
) -> Option<(u16, DecodedOngoingReferendum)> {
    match status {
        ReferendumStatus::Ongoing(ongoing) => {
            let ongoing = *ongoing; // Unbox
            let track = ongoing.track;

            let enactment = match ongoing.enactment {
                EnactmentType::After(blocks) => DecodedEnactment::After(blocks),
                EnactmentType::At(block) => DecodedEnactment::At(block),
            };

            let decision_deposit = ongoing.decision_deposit.map(|d| DecodedDeposit {
                who: d.who,
                amount: d.amount,
            });

            let deciding = ongoing.deciding.map(|d| DecodedDeciding {
                since: d.since,
                confirming: d.confirming,
            });

            Some((
                track,
                DecodedOngoingReferendum {
                    id,
                    track,
                    enactment,
                    submitted: ongoing.submitted,
                    decision_deposit,
                    deciding,
                },
            ))
        }
        _ => None,
    }
}
