// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! NominationPools pallet storage query functions.
//!
//! This module provides standalone functions for querying NominationPools pallet storage items.

use parity_scale_codec::Decode;
use scale_decode::DecodeAsType;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// SCALE Decode Types
// ================================================================================================

#[derive(Debug, Clone, Decode)]
pub enum PoolState {
    Open,
    Blocked,
    Destroying,
}

impl PoolState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PoolState::Open => "Open",
            PoolState::Blocked => "Blocked",
            PoolState::Destroying => "Destroying",
        }
    }
}

/// Bonded pool storage format (modern version with commission)
#[derive(Debug, Clone, Decode)]
pub struct BondedPoolStorageV2 {
    pub commission: CommissionStorage,
    pub member_counter: u32,
    pub points: u128,
    pub roles: PoolRolesStorage,
    pub state: PoolState,
}

/// Bonded pool storage format (legacy without commission)
#[derive(Debug, Clone, Decode)]
pub struct BondedPoolStorageV1 {
    pub points: u128,
    pub state: PoolState,
    pub member_counter: u32,
    pub roles: PoolRolesStorage,
}

#[derive(Debug, Clone, Decode)]
pub struct CommissionStorage {
    pub current: Option<(u32, [u8; 32])>, // (Perbill, AccountId)
    pub max: Option<u32>,                 // Perbill
    pub change_rate: Option<CommissionChangeRate>,
    pub throttle_from: Option<u32>, // BlockNumber
    pub claim_permission: Option<CommissionClaimPermission>,
}

#[derive(Debug, Clone, Decode)]
pub struct CommissionChangeRate {
    pub max_increase: u32, // Perbill
    pub min_delay: u32,    // BlockNumber
}

#[derive(Debug, Clone, Decode)]
pub enum CommissionClaimPermission {
    Permissionless,
    #[allow(dead_code)]
    Account([u8; 32]),
}

#[derive(Debug, Clone, Decode)]
pub struct PoolRolesStorage {
    pub depositor: [u8; 32],
    pub root: Option<[u8; 32]>,
    pub nominator: Option<[u8; 32]>,
    pub bouncer: Option<[u8; 32]>,
}

/// Reward pool storage format (modern with commission)
#[derive(Debug, Clone, Decode)]
pub struct RewardPoolStorageV2 {
    pub last_recorded_reward_counter: u128,
    pub last_recorded_total_payouts: u128,
    pub total_rewards_claimed: u128,
    pub total_commission_pending: u128,
    pub total_commission_claimed: u128,
}

/// Reward pool storage format (legacy without commission)
#[derive(Debug, Clone, Decode)]
pub struct RewardPoolStorageV1 {
    pub last_recorded_reward_counter: u128,
    pub last_recorded_total_payouts: u128,
    pub total_rewards_claimed: u128,
}

// ================================================================================================
// Decoded Result Types
// ================================================================================================

/// Decoded bonded pool - either V1 or V2
#[derive(Debug, Clone)]
pub enum DecodedBondedPool {
    V1(BondedPoolStorageV1),
    V2(BondedPoolStorageV2),
}

/// Decoded reward pool - either V1 or V2
#[derive(Debug, Clone)]
pub enum DecodedRewardPool {
    V1(RewardPoolStorageV1),
    V2(RewardPoolStorageV2),
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Generic function to fetch and decode a storage value.
/// Uses `DecodeAsType` for type-guided decoding.
pub async fn get_storage_value<T>(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pallet: &str,
    entry: &str,
) -> Option<T>
where
    T: DecodeAsType,
{
    let addr = subxt::dynamic::storage::<(), scale_value::Value>(pallet, entry);
    let storage_value = client_at_block.storage().fetch(addr, ()).await.ok()?;
    storage_value.decode_as().ok()
}

/// Fetches bonded pool details from NominationPools::BondedPools storage.
/// Automatically handles V1/V2 versioning.
pub async fn get_bonded_pool(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pool_id: u32,
) -> Option<DecodedBondedPool> {
    let addr = subxt::dynamic::storage::<_, scale_value::Value>("NominationPools", "BondedPools");
    let raw_bytes = match client_at_block.storage().fetch(addr, (pool_id,)).await {
        Ok(value) => value.into_bytes(),
        Err(_) => return None,
    };

    // Try modern V2 format first (with commission)
    let mut cursor = &raw_bytes[..];
    if let Ok(storage) = BondedPoolStorageV2::decode(&mut cursor) {
        // Sanity check: ensure all bytes were consumed
        if cursor.is_empty() {
            return Some(DecodedBondedPool::V2(storage));
        }
    }

    // Fall back to V1 format (legacy without commission)
    let mut cursor = &raw_bytes[..];
    if let Ok(storage) = BondedPoolStorageV1::decode(&mut cursor) {
        // Sanity check: ensure all bytes were consumed
        if cursor.is_empty() {
            return Some(DecodedBondedPool::V1(storage));
        }
    }

    None
}

/// Fetches reward pool details from NominationPools::RewardPools storage.
/// Automatically handles V1/V2 versioning.
pub async fn get_reward_pool(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pool_id: u32,
) -> Option<DecodedRewardPool> {
    let addr = subxt::dynamic::storage::<_, scale_value::Value>("NominationPools", "RewardPools");
    let raw_bytes = match client_at_block.storage().fetch(addr, (pool_id,)).await {
        Ok(value) => value.into_bytes(),
        Err(_) => return None,
    };

    // Try modern V2 format first (with commission tracking)
    let mut cursor = &raw_bytes[..];
    if let Ok(storage) = RewardPoolStorageV2::decode(&mut cursor) {
        // Sanity check: ensure all bytes were consumed
        if cursor.is_empty() {
            return Some(DecodedRewardPool::V2(storage));
        }
    }

    // Fall back to V1 format (legacy without commission)
    let mut cursor = &raw_bytes[..];
    if let Ok(storage) = RewardPoolStorageV1::decode(&mut cursor) {
        // Sanity check: ensure all bytes were consumed
        if cursor.is_empty() {
            return Some(DecodedRewardPool::V1(storage));
        }
    }

    None
}
