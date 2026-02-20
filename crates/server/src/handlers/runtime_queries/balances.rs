// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Balances and System pallet storage query functions.
//!
//! This module provides standalone functions for querying balance-related storage items
//! including System::Account, Balances::Locks, Proxy::Proxies, and Vesting::Vesting.

use parity_scale_codec::Decode;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// SCALE Decode Types
// ================================================================================================

/// Account data for modern runtimes (with frozen field)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct AccountDataModern {
    free: u128,
    reserved: u128,
    frozen: u128,
    flags: u128,
}

/// Account data for legacy runtimes (with misc_frozen/fee_frozen fields)
#[derive(Debug, Clone, Decode)]
struct AccountDataLegacy {
    free: u128,
    reserved: u128,
    misc_frozen: u128,
    fee_frozen: u128,
}

/// Account info structure (modern runtime)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct AccountInfoModern {
    nonce: u32,
    consumers: u32,
    providers: u32,
    sufficients: u32,
    data: AccountDataModern,
}

/// Account info structure (legacy runtime)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct AccountInfoLegacy {
    nonce: u32,
    consumers: u32,
    providers: u32,
    sufficients: u32,
    data: AccountDataLegacy,
}

/// Balance lock structure
#[derive(Debug, Clone, Decode)]
struct BalanceLock {
    id: [u8; 8],
    amount: u128,
    reasons: Reasons,
}

/// Lock reasons enum
#[derive(Debug, Clone, Decode)]
enum Reasons {
    Fee,
    Misc,
    All,
}

impl Reasons {
    fn as_str(&self) -> &'static str {
        match self {
            Reasons::Fee => "Fee",
            Reasons::Misc => "Misc",
            Reasons::All => "All",
        }
    }
}

/// Proxy definition structure
#[derive(Debug, Clone, Decode)]
struct ProxyDefinition {
    delegate: [u8; 32],
    proxy_type: u8,
    delay: u32,
}

/// Vesting info structure
#[derive(Debug, Clone, Decode)]
struct VestingInfo {
    locked: u128,
    per_block: u128,
    starting_block: u32,
}

// ================================================================================================
// Public Data Types
// ================================================================================================

/// Decoded account data
#[derive(Debug, Clone)]
pub struct DecodedAccountData {
    pub nonce: u32,
    pub free: u128,
    pub reserved: u128,
    pub misc_frozen: Option<u128>,
    pub fee_frozen: Option<u128>,
    pub frozen: Option<u128>,
}

/// Decoded balance lock
#[derive(Debug, Clone)]
pub struct DecodedBalanceLock {
    pub id: String,
    pub amount: u128,
    pub reasons: String,
}

/// Decoded proxy definition
#[derive(Debug, Clone)]
pub struct DecodedProxyDefinition {
    pub delegate: String,
    pub proxy_type: u8,
    pub delay: u32,
}

/// Decoded vesting info
#[derive(Debug, Clone)]
pub struct DecodedVestingInfo {
    pub locked: u128,
    pub per_block: u128,
    pub starting_block: u32,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Get account data from System::Account storage.
pub async fn get_account_data(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Option<DecodedAccountData> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("System", "Account");
    let account_bytes: [u8; 32] = *account.as_ref();

    let value = client_at_block
        .storage()
        .fetch(storage_addr, (account_bytes,))
        .await
        .ok()?;

    let raw_bytes = value.into_bytes();
    decode_account_info(&raw_bytes)
}

/// Get account data, returning default values if account doesn't exist.
pub async fn get_account_data_or_default(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> DecodedAccountData {
    get_account_data(client_at_block, account)
        .await
        .unwrap_or(DecodedAccountData {
            nonce: 0,
            free: 0,
            reserved: 0,
            misc_frozen: None,
            fee_frozen: None,
            frozen: Some(0),
        })
}

/// Get balance locks from Balances::Locks storage.
pub async fn get_balance_locks(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Vec<DecodedBalanceLock> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Balances", "Locks");
    let account_bytes: [u8; 32] = *account.as_ref();

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (account_bytes,))
        .await
    {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let raw_bytes = value.into_bytes();
    decode_balance_locks(&raw_bytes).unwrap_or_default()
}

/// Get proxy definitions from Proxy::Proxies storage.
pub async fn get_proxy_definitions(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    ss58_prefix: u16,
) -> Option<(Vec<DecodedProxyDefinition>, u128)> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Proxy", "Proxies");
    let account_bytes: [u8; 32] = *account.as_ref();

    let value = client_at_block
        .storage()
        .fetch(storage_addr, (account_bytes,))
        .await
        .ok()?;

    let raw_bytes = value.into_bytes();
    decode_proxy_definitions(&raw_bytes, ss58_prefix)
}

/// Get vesting schedules from Vesting::Vesting storage.
pub async fn get_vesting_schedules(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Vec<DecodedVestingInfo> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Vesting", "Vesting");
    let account_bytes: [u8; 32] = *account.as_ref();

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (account_bytes,))
        .await
    {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let raw_bytes = value.into_bytes();
    decode_vesting_schedules(&raw_bytes).unwrap_or_default()
}

// ================================================================================================
// Decoding Functions
// ================================================================================================

fn decode_account_info(raw_bytes: &[u8]) -> Option<DecodedAccountData> {
    // Try modern format first (with frozen field)
    if let Ok(account_info) = AccountInfoModern::decode(&mut &raw_bytes[..]) {
        return Some(DecodedAccountData {
            nonce: account_info.nonce,
            free: account_info.data.free,
            reserved: account_info.data.reserved,
            misc_frozen: None,
            fee_frozen: None,
            frozen: Some(account_info.data.frozen),
        });
    }

    // Fall back to legacy format (with misc_frozen/fee_frozen fields)
    if let Ok(account_info) = AccountInfoLegacy::decode(&mut &raw_bytes[..]) {
        return Some(DecodedAccountData {
            nonce: account_info.nonce,
            free: account_info.data.free,
            reserved: account_info.data.reserved,
            misc_frozen: Some(account_info.data.misc_frozen),
            fee_frozen: Some(account_info.data.fee_frozen),
            frozen: None,
        });
    }

    None
}

fn decode_balance_locks(raw_bytes: &[u8]) -> Option<Vec<DecodedBalanceLock>> {
    let locks = Vec::<BalanceLock>::decode(&mut &raw_bytes[..]).ok()?;
    
    Some(
        locks
            .into_iter()
            .map(|lock| {
                let id = format!("0x{}", hex::encode(lock.id));
                DecodedBalanceLock {
                    id,
                    amount: lock.amount,
                    reasons: lock.reasons.as_str().to_string(),
                }
            })
            .collect()
    )
}

fn decode_proxy_definitions(
    raw_bytes: &[u8],
    ss58_prefix: u16,
) -> Option<(Vec<DecodedProxyDefinition>, u128)> {
    use sp_core::crypto::Ss58Codec;
    
    // Proxy storage is (Vec<ProxyDefinition>, deposit)
    // Try decoding the tuple
    let (proxies, deposit): (Vec<ProxyDefinition>, u128) =
        Decode::decode(&mut &raw_bytes[..]).ok()?;

    let decoded_proxies = proxies
        .into_iter()
        .map(|p| {
            let delegate = AccountId32::from(p.delegate)
                .to_ss58check_with_version(ss58_prefix.into());
            DecodedProxyDefinition {
                delegate,
                proxy_type: p.proxy_type,
                delay: p.delay,
            }
        })
        .collect();

    Some((decoded_proxies, deposit))
}

fn decode_vesting_schedules(raw_bytes: &[u8]) -> Option<Vec<DecodedVestingInfo>> {
    // Vesting storage is Option<Vec<VestingInfo>> or Vec<VestingInfo>
    // Try Vec first
    if let Ok(schedules) = Vec::<VestingInfo>::decode(&mut &raw_bytes[..]) {
        return Some(
            schedules
                .into_iter()
                .map(|v| DecodedVestingInfo {
                    locked: v.locked,
                    per_block: v.per_block,
                    starting_block: v.starting_block,
                })
                .collect()
        );
    }

    // Try Option<Vec<VestingInfo>>
    if let Ok(Some(schedules)) = Option::<Vec<VestingInfo>>::decode(&mut &raw_bytes[..]) {
        return Some(
            schedules
                .into_iter()
                .map(|v| DecodedVestingInfo {
                    locked: v.locked,
                    per_block: v.per_block,
                    starting_block: v.starting_block,
                })
                .collect()
        );
    }

    None
}
