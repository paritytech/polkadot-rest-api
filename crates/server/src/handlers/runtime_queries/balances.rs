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

/// Account info structure for pre-sufficients era (before runtime v29).
/// Layout: nonce(4) + consumers(4) + providers(4) + AccountDataLegacy(64) = 76 bytes.
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct AccountInfoPreSufficients {
    nonce: u32,
    consumers: u32,
    providers: u32,
    data: AccountDataLegacy,
}

/// Account info structure for earliest runtimes (single refcount field).
/// Layout: nonce(4) + refcount(u8) + AccountDataLegacy(64) = 69 bytes.
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
struct AccountInfoRefcount {
    nonce: u32,
    refcount: u8,
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

/// Error reading an account's `System::Account` entry.
#[derive(Debug, thiserror::Error)]
pub enum AccountDataError {
    /// The storage fetch failed (transient / retryable).
    #[error(transparent)]
    Fetch(#[from] subxt::error::StorageError),

    /// Bytes were fetched but matched no known `AccountInfo` layout.
    #[error("failed to decode AccountInfo storage value ({0} bytes)")]
    Decode(usize),
}

/// Fetch the raw SCALE bytes of an account-keyed map storage entry.
///
/// `Ok(Some(bytes))` if present, `Ok(None)` if genuinely absent, `Err` if the fetch failed.
async fn try_fetch_storage(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pallet: &str,
    entry: &str,
    account: &AccountId32,
) -> Result<Option<Vec<u8>>, subxt::error::StorageError> {
    let storage_addr = subxt::dynamic::storage::<_, ()>(pallet, entry);
    let account_bytes: [u8; 32] = *account.as_ref();

    Ok(client_at_block
        .storage()
        .try_fetch(storage_addr, (account_bytes,))
        .await?
        .map(|value| value.into_bytes()))
}

/// Read and decode an account's `System::Account` entry.
///
/// `Ok(Some)` if present and decoded, `Ok(None)` if genuinely absent, `Err(Fetch)` on a fetch
/// failure, `Err(Decode)` if the bytes matched no known layout.
pub async fn get_account_data(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Result<Option<DecodedAccountData>, AccountDataError> {
    let Some(raw_bytes) = try_fetch_storage(client_at_block, "System", "Account", account).await?
    else {
        return Ok(None);
    };

    // Bytes present but undecodable is an error, not an absent account.
    match decode_account_info(&raw_bytes) {
        Some(data) => Ok(Some(data)),
        None => Err(AccountDataError::Decode(raw_bytes.len())),
    }
}

/// Like [`get_account_data`], but returns zeroed balances for a genuinely-absent account.
/// Fetch and decode errors still propagate — never a fabricated `free: 0`.
pub async fn get_account_data_or_default(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Result<DecodedAccountData, AccountDataError> {
    Ok(get_account_data(client_at_block, account)
        .await?
        .unwrap_or(DecodedAccountData {
            nonce: 0,
            free: 0,
            reserved: 0,
            misc_frozen: None,
            fee_frozen: None,
            frozen: Some(0),
        }))
}

/// Get balance locks from `Balances::Locks` (empty when absent; `Err` on fetch failure).
pub async fn get_balance_locks(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Result<Vec<DecodedBalanceLock>, subxt::error::StorageError> {
    let Some(raw_bytes) = try_fetch_storage(client_at_block, "Balances", "Locks", account).await?
    else {
        return Ok(Vec::new());
    };

    Ok(decode_balance_locks(&raw_bytes).unwrap_or_default())
}

/// Get proxy definitions from `Proxy::Proxies` (`None` when absent; `Err` on fetch failure).
pub async fn get_proxy_definitions(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    ss58_prefix: u16,
) -> Result<Option<(Vec<DecodedProxyDefinition>, u128)>, subxt::error::StorageError> {
    let Some(raw_bytes) = try_fetch_storage(client_at_block, "Proxy", "Proxies", account).await?
    else {
        return Ok(None);
    };

    Ok(decode_proxy_definitions(&raw_bytes, ss58_prefix))
}

/// Get vesting schedules from `Vesting::Vesting` (empty when absent; `Err` on fetch failure).
pub async fn get_vesting_schedules(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Result<Vec<DecodedVestingInfo>, subxt::error::StorageError> {
    let Some(raw_bytes) = try_fetch_storage(client_at_block, "Vesting", "Vesting", account).await?
    else {
        return Ok(Vec::new());
    };

    Ok(decode_vesting_schedules(&raw_bytes).unwrap_or_default())
}

// ================================================================================================
// Decoding Functions
// ================================================================================================

fn decode_account_info(raw_bytes: &[u8]) -> Option<DecodedAccountData> {
    let len = raw_bytes.len();

    /// Helper: build `DecodedAccountData` for modern runtimes (frozen field, no misc/fee frozen).
    fn modern_data(nonce: u32, data: &AccountDataModern) -> DecodedAccountData {
        DecodedAccountData {
            nonce,
            free: data.free,
            reserved: data.reserved,
            misc_frozen: None,
            fee_frozen: None,
            frozen: Some(data.frozen),
        }
    }

    /// Helper: build `DecodedAccountData` for legacy runtimes (misc_frozen/fee_frozen, no frozen).
    fn legacy_data(nonce: u32, data: &AccountDataLegacy) -> DecodedAccountData {
        DecodedAccountData {
            nonce,
            free: data.free,
            reserved: data.reserved,
            misc_frozen: Some(data.misc_frozen),
            fee_frozen: Some(data.fee_frozen),
            frozen: None,
        }
    }

    // Modern format: nonce + consumers + providers + sufficients + {free, reserved, frozen, flags}
    // Layout: 4+4+4+4 + 16+16+16+16 = 80 bytes
    //
    // NOTE: Both AccountInfoModern and AccountInfoLegacy are 80 bytes with identical SCALE
    // layouts — the last two u128 fields are (frozen, flags) vs (misc_frozen, fee_frozen).
    // We try Modern first because the `frozen` field was introduced in the same runtime
    // upgrade that added `sufficients` (making the struct 80 bytes). Any 80-byte payload
    // from a runtime that still used misc_frozen/fee_frozen would also have `sufficients`,
    // which didn't exist before `frozen` — so Modern-first is correct in practice.
    // The Legacy fallback exists only as a safety net for unexpected edge cases.
    if len == 80 {
        if let Ok(info) = AccountInfoModern::decode(&mut &raw_bytes[..]) {
            return Some(modern_data(info.nonce, &info.data));
        }
        if let Ok(info) = AccountInfoLegacy::decode(&mut &raw_bytes[..]) {
            return Some(legacy_data(info.nonce, &info.data));
        }
    }

    // Pre-sufficients format (before runtime v29): nonce + consumers + providers + {free, reserved, misc_frozen, fee_frozen}
    // Layout: 4+4+4 + 16+16+16+16 = 76 bytes
    if len == 76
        && let Ok(info) = AccountInfoPreSufficients::decode(&mut &raw_bytes[..])
    {
        return Some(legacy_data(info.nonce, &info.data));
    }

    // Earliest format (single refcount): nonce + refcount(u8) + {free, reserved, misc_frozen, fee_frozen}
    // Layout: 4+1 + 16+16+16+16 = 69 bytes
    if len == 69
        && let Ok(info) = AccountInfoRefcount::decode(&mut &raw_bytes[..])
    {
        return Some(legacy_data(info.nonce, &info.data));
    }

    // No fallback: we only decode sizes we explicitly understand (69, 76, 80).
    // SCALE Decode will happily consume a prefix of a larger buffer, which would silently
    // return wrong data for future runtime layouts with a different size. If we encounter
    // an unknown size, log it and return None so the caller can handle it gracefully.
    tracing::warn!(
        "Failed to decode AccountInfo ({} bytes): 0x{}",
        len,
        hex::encode(&raw_bytes[..len.min(32)])
    );
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
            .collect(),
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
            let delegate =
                AccountId32::from(p.delegate).to_ss58check_with_version(ss58_prefix.into());
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
                .collect(),
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
                .collect(),
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manually build a raw 80-byte modern AccountInfo payload (SCALE little-endian):
    /// nonce(4) + consumers(4) + providers(4) + sufficients(4) + free(16) + reserved(16) + frozen(16) + flags(16)
    fn build_modern_bytes(nonce: u32, free: u128, reserved: u128, frozen: u128) -> Vec<u8> {
        let mut buf = Vec::with_capacity(80);
        buf.extend_from_slice(&nonce.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes()); // consumers
        buf.extend_from_slice(&1u32.to_le_bytes()); // providers
        buf.extend_from_slice(&0u32.to_le_bytes()); // sufficients
        buf.extend_from_slice(&free.to_le_bytes());
        buf.extend_from_slice(&reserved.to_le_bytes());
        buf.extend_from_slice(&frozen.to_le_bytes());
        buf.extend_from_slice(&0u128.to_le_bytes()); // flags
        assert_eq!(buf.len(), 80);
        buf
    }

    /// Manually build a raw 76-byte pre-sufficients AccountInfo payload:
    /// nonce(4) + consumers(4) + providers(4) + free(16) + reserved(16) + misc_frozen(16) + fee_frozen(16)
    fn build_pre_sufficients_bytes(
        nonce: u32,
        free: u128,
        reserved: u128,
        misc_frozen: u128,
        fee_frozen: u128,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(76);
        buf.extend_from_slice(&nonce.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes()); // consumers
        buf.extend_from_slice(&1u32.to_le_bytes()); // providers
        buf.extend_from_slice(&free.to_le_bytes());
        buf.extend_from_slice(&reserved.to_le_bytes());
        buf.extend_from_slice(&misc_frozen.to_le_bytes());
        buf.extend_from_slice(&fee_frozen.to_le_bytes());
        assert_eq!(buf.len(), 76);
        buf
    }

    /// Manually build a raw 69-byte refcount-era AccountInfo payload:
    /// nonce(4) + refcount(1) + free(16) + reserved(16) + misc_frozen(16) + fee_frozen(16)
    fn build_refcount_bytes(
        nonce: u32,
        refcount: u8,
        free: u128,
        reserved: u128,
        misc_frozen: u128,
        fee_frozen: u128,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(69);
        buf.extend_from_slice(&nonce.to_le_bytes());
        buf.push(refcount);
        buf.extend_from_slice(&free.to_le_bytes());
        buf.extend_from_slice(&reserved.to_le_bytes());
        buf.extend_from_slice(&misc_frozen.to_le_bytes());
        buf.extend_from_slice(&fee_frozen.to_le_bytes());
        assert_eq!(buf.len(), 69);
        buf
    }

    #[test]
    fn test_decode_modern_account_info_80_bytes() {
        let bytes = build_modern_bytes(42, 1_000_000_000_000, 500_000, 100_000);

        let result = decode_account_info(&bytes).expect("should decode modern format");
        assert_eq!(result.nonce, 42);
        assert_eq!(result.free, 1_000_000_000_000);
        assert_eq!(result.reserved, 500_000);
        assert_eq!(result.frozen, Some(100_000));
        assert_eq!(result.misc_frozen, None);
        assert_eq!(result.fee_frozen, None);
    }

    #[test]
    fn test_decode_pre_sufficients_account_info_76_bytes() {
        // Simulates Polkadot blocks between runtime v29 introduction and sufficients addition
        let bytes = build_pre_sufficients_bytes(79219, 466_398_602_382_908_664, 0, 0, 0);

        let result =
            decode_account_info(&bytes).expect("should decode pre-sufficients format (76 bytes)");
        assert_eq!(result.nonce, 79219);
        assert_eq!(result.free, 466_398_602_382_908_664);
        assert_eq!(result.reserved, 0);
        assert_eq!(result.misc_frozen, Some(0));
        assert_eq!(result.fee_frozen, Some(0));
        assert_eq!(result.frozen, None);
    }

    #[test]
    fn test_decode_refcount_account_info_69_bytes() {
        // Simulates earliest Polkadot blocks (e.g. block 20000)
        let bytes = build_refcount_bytes(1, 0, 1_000_000_000_000_000, 0, 0, 0);

        let result = decode_account_info(&bytes).expect("should decode refcount format (69 bytes)");
        assert_eq!(result.nonce, 1);
        assert_eq!(result.free, 1_000_000_000_000_000);
        assert_eq!(result.reserved, 0);
        assert_eq!(result.misc_frozen, Some(0));
        assert_eq!(result.fee_frozen, Some(0));
        assert_eq!(result.frozen, None);
    }

    #[test]
    fn test_decode_empty_bytes_returns_none() {
        let result = decode_account_info(&[]);
        assert!(result.is_none(), "empty bytes should return None");
    }

    #[test]
    fn test_decode_short_bytes_returns_none() {
        let result = decode_account_info(&[0u8; 10]);
        assert!(result.is_none(), "too-short bytes should return None");
    }

    #[test]
    fn test_decode_pre_sufficients_with_nonzero_frozen() {
        // Polkadot block ~3574738 era: pre-sufficients with actual frozen balances
        let bytes = build_pre_sufficients_bytes(
            47101,
            4_533_720_292_342_876_436_692_992u128,
            0,
            100_000,
            50_000,
        );

        let result =
            decode_account_info(&bytes).expect("should decode pre-sufficients with nonzero frozen");
        assert_eq!(result.nonce, 47101);
        assert_eq!(result.free, 4_533_720_292_342_876_436_692_992u128);
        assert_eq!(result.misc_frozen, Some(100_000));
        assert_eq!(result.fee_frozen, Some(50_000));
        assert_eq!(result.frozen, None);
    }

    #[test]
    fn test_decode_refcount_with_nonzero_refcount() {
        // Earliest Polkadot blocks had refcount > 0 for active accounts
        let bytes = build_refcount_bytes(5, 2, 500_000_000_000, 100_000_000, 0, 0);

        let result =
            decode_account_info(&bytes).expect("should decode refcount format with refcount > 0");
        assert_eq!(result.nonce, 5);
        assert_eq!(result.free, 500_000_000_000);
        assert_eq!(result.reserved, 100_000_000);
    }

    #[test]
    fn test_size_invariants() {
        // Verify the struct sizes match our expectations
        assert_eq!(build_modern_bytes(0, 0, 0, 0).len(), 80);
        assert_eq!(build_pre_sufficients_bytes(0, 0, 0, 0, 0).len(), 76);
        assert_eq!(build_refcount_bytes(0, 0, 0, 0, 0, 0).len(), 69);
    }
}

/// End-to-end tests for `get_account_data_or_default` driven over a mock RPC client.
///
/// These guard the false-zero regression: a failed `System::Account` fetch must surface
/// as an error, while a present or genuinely-absent account must decode correctly.
#[cfg(test)]
mod rpc_fetch_tests {
    use super::*;
    use crate::test_fixtures::{TEST_BLOCK_NUMBER, mock_rpc_client_builder};
    use subxt::{OnlineClient, SubstrateConfig};
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    async fn online_client(mock: MockRpcClient) -> OnlineClient<SubstrateConfig> {
        let rpc_client = RpcClient::new(mock);
        OnlineClient::<SubstrateConfig>::from_rpc_client(rpc_client)
            .await
            .expect("failed to build OnlineClient over mock")
    }

    fn test_account() -> AccountId32 {
        AccountId32::new([1u8; 32])
    }

    /// Build an 80-byte modern `AccountInfo` payload with the given free balance.
    fn modern_account_bytes(free: u128) -> Vec<u8> {
        let mut buf = Vec::with_capacity(80);
        buf.extend_from_slice(&0u32.to_le_bytes()); // nonce
        buf.extend_from_slice(&1u32.to_le_bytes()); // consumers
        buf.extend_from_slice(&1u32.to_le_bytes()); // providers
        buf.extend_from_slice(&0u32.to_le_bytes()); // sufficients
        buf.extend_from_slice(&free.to_le_bytes()); // free
        buf.extend_from_slice(&0u128.to_le_bytes()); // reserved
        buf.extend_from_slice(&0u128.to_le_bytes()); // frozen
        buf.extend_from_slice(&0u128.to_le_bytes()); // flags
        buf
    }

    #[tokio::test]
    async fn present_account_returns_real_balance() {
        let free = 100_000_000u128;
        let hex = format!("0x{}", hex::encode(modern_account_bytes(free)));
        let mock = mock_rpc_client_builder()
            .method_handler("state_getStorage", move |_params| {
                let hex = hex.clone();
                async move { MockJson(hex) }
            })
            .build();

        let client = online_client(mock).await;
        let at = client
            .at_block(TEST_BLOCK_NUMBER)
            .await
            .expect("at_block failed");

        let data = get_account_data_or_default(&at, &test_account())
            .await
            .expect("a successful fetch must not error");
        assert_eq!(data.free, free);
    }

    #[tokio::test]
    async fn absent_account_yields_zero() {
        // `null` response => no value at the key => legitimate zero balance.
        let mock = mock_rpc_client_builder()
            .method_handler("state_getStorage", async |_params| {
                MockJson(Option::<String>::None)
            })
            .build();

        let client = online_client(mock).await;
        let at = client
            .at_block(TEST_BLOCK_NUMBER)
            .await
            .expect("at_block failed");

        let data = get_account_data_or_default(&at, &test_account())
            .await
            .expect("an absent account is not an error");
        assert_eq!(data.free, 0);
    }

    /// Regression guard for the Binance false-zero incident: a transient fetch failure
    /// must be an error, NOT a fabricated `free: 0`. This fails against the previous
    /// `.ok()?` implementation (which returned zeroed data) and passes after the fix.
    #[tokio::test]
    async fn transient_fetch_error_is_not_false_zero() {
        let mock = mock_rpc_client_builder()
            .method_handler("state_getStorage", async |_params| {
                Err::<MockJson<String>, subxt_rpcs::Error>(subxt_rpcs::Error::Client(Box::new(
                    std::io::Error::other("simulated transient RPC failure"),
                )))
            })
            .build();

        let client = online_client(mock).await;
        let at = client
            .at_block(TEST_BLOCK_NUMBER)
            .await
            .expect("at_block failed");

        let result = get_account_data_or_default(&at, &test_account()).await;
        assert!(
            result.is_err(),
            "a failed storage fetch must surface as an error, not free:0"
        );
    }

    /// Present-but-undecodable bytes must error, not become `free: 0`.
    #[tokio::test]
    async fn undecodable_bytes_are_not_false_zero() {
        // 10 bytes: matches no known layout.
        let hex = format!("0x{}", hex::encode([0u8; 10]));
        let mock = mock_rpc_client_builder()
            .method_handler("state_getStorage", move |_params| {
                let hex = hex.clone();
                async move { MockJson(hex) }
            })
            .build();

        let client = online_client(mock).await;
        let at = client
            .at_block(TEST_BLOCK_NUMBER)
            .await
            .expect("at_block failed");

        let result = get_account_data_or_default(&at, &test_account()).await;
        assert!(
            result.is_err(),
            "present-but-undecodable bytes must surface as an error, not free:0"
        );
    }
}
