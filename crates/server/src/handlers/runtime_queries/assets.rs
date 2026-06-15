// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Assets pallet storage query functions.
//!
//! This module provides standalone functions for querying Assets pallet storage items.
//! Each function handles SCALE decoding and version compatibility automatically.
//!
//! # Storage Items Covered
//! - `Assets::Asset` - Asset details (owner, issuer, supply, etc.)
//! - `Assets::Metadata` - Asset metadata (name, symbol, decimals)
//! - `Assets::Account` - Account balances for assets
//! - `Assets::Approvals` - Approval amounts for asset transfers

use super::assets_common::{
    AssetAccount, AssetAccountLegacy, AssetApproval, AssetDetails, AssetMetadata, format_account_id,
};
use parity_scale_codec::Decode;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying Assets pallet storage.
#[derive(Debug, Error)]
pub enum AssetsStorageError {
    /// The Assets pallet is not available on this chain.
    #[error("Assets pallet not available")]
    PalletNotAvailable,

    /// The requested asset was not found.
    #[error("Asset {0} not found")]
    AssetNotFound(u32),

    /// Failed to decode storage value.
    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(String),

    /// Storage fetch failed.
    #[error("Storage fetch failed: {0}")]
    StorageFetchFailed(String),
}

// ================================================================================================
// SCALE Decode Types - Assets::Approvals
// ================================================================================================

// Note: AssetApproval is defined in assets_common module

// ================================================================================================
// Public Data Types (Decoded/Formatted)
// ================================================================================================

/// Decoded asset information ready for API response.
#[derive(Debug, Clone)]
pub struct DecodedAssetInfo {
    pub owner: String,
    pub issuer: String,
    pub admin: String,
    pub freezer: String,
    pub supply: String,
    pub deposit: String,
    pub min_balance: String,
    pub is_sufficient: bool,
    pub accounts: String,
    pub sufficients: String,
    pub approvals: String,
    pub status: String,
}

/// Decoded asset metadata ready for API response.
#[derive(Debug, Clone)]
pub struct DecodedAssetMetadata {
    pub deposit: String,
    pub name: String,
    pub symbol: String,
    pub decimals: String,
    pub is_frozen: bool,
}

/// Decoded asset balance for an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedAssetBalance {
    pub balance: String,
    pub is_frozen: bool,
    pub is_sufficient: bool,
}

/// A per-asset failure encountered while fetching balances in bulk.
///
/// This represents a *real* failure — an RPC/backend error, or storage that was
/// present but could not be decoded. It is deliberately distinct from the ordinary
/// "account holds none of this asset" case, which is an absence (omitted), not an
/// error. Surfacing these instead of dropping them is the fix for issue #342.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetBalanceFetchError {
    pub asset_id: u32,
    pub reason: String,
}

/// Result of fetching balances for multiple assets.
///
/// `balances` holds the assets we could resolve. `errors` holds the assets whose
/// per-asset query genuinely failed; a non-empty `errors` lets callers report a
/// partial result instead of silently omitting the failed assets (issue #342).
#[derive(Debug, Default)]
pub struct AssetBalancesResult {
    pub balances: Vec<(u32, DecodedAssetBalance)>,
    pub errors: Vec<AssetBalanceFetchError>,
}

/// Outcome of fetching + decoding a single asset balance, independent of the
/// storage backend. Mapping the subxt result into this enum at the async boundary
/// keeps the classification logic (`collect_asset_balances`) pure and unit-testable.
#[derive(Debug)]
enum AssetBalanceOutcome {
    /// Storage entry present and decoded successfully.
    Decoded(DecodedAssetBalance),
    /// Account holds no entry for this asset — a legitimate absence, not an error.
    Absent,
    /// Fetch or decode failed — a real error we must surface, never a zero balance.
    Failed(String),
}

/// Build a zero-balance stub for assets the account does not hold (used when the
/// caller asked to include empty balances).
fn zero_balance() -> DecodedAssetBalance {
    DecodedAssetBalance {
        balance: "0".to_string(),
        is_frozen: false,
        is_sufficient: false,
    }
}

/// Turn per-asset outcomes into balances + errors, honoring `show_empty`.
///
/// - `Decoded` is included in `balances`.
/// - `Absent` is included as a zero balance only when `show_empty` is true, otherwise
///   omitted; it is never an error.
/// - `Failed` is recorded in `errors` and is **never** turned into a (zero) balance,
///   so a failed per-asset query can never masquerade as a zero holding — the
///   ghost-zero class of bug described in issue #342.
fn collect_asset_balances(
    outcomes: Vec<(u32, AssetBalanceOutcome)>,
    show_empty: bool,
) -> AssetBalancesResult {
    let mut result = AssetBalancesResult::default();
    for (asset_id, outcome) in outcomes {
        match outcome {
            AssetBalanceOutcome::Decoded(decoded) => result.balances.push((asset_id, decoded)),
            AssetBalanceOutcome::Absent => {
                if show_empty {
                    result.balances.push((asset_id, zero_balance()));
                }
            }
            AssetBalanceOutcome::Failed(reason) => {
                result
                    .errors
                    .push(AssetBalanceFetchError { asset_id, reason });
            }
        }
    }
    result
}

/// Decoded asset approval.
#[derive(Debug, Clone)]
pub struct DecodedAssetApproval {
    pub amount: String,
    pub deposit: String,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Check if the Assets pallet exists on the chain.
pub fn is_assets_pallet_available(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> bool {
    client_at_block.storage().entry(("Assets", "Asset")).is_ok()
}

/// Fetch all asset IDs from Assets::Asset storage.
///
/// Returns a list of all asset IDs that exist on the chain.
pub async fn get_all_asset_ids(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<u32>, AssetsStorageError> {
    let storage_query = subxt::storage::dynamic::<Vec<u32>, Vec<u8>>("Assets", "Asset");
    let storage_entry = client_at_block
        .storage()
        .entry(storage_query)
        .map_err(|_| AssetsStorageError::PalletNotAvailable)?;

    let mut asset_ids = Vec::new();
    let mut values = storage_entry
        .iter(Vec::<u32>::new())
        .await
        .map_err(|e| AssetsStorageError::StorageFetchFailed(e.to_string()))?;

    while let Some(result) = values.next().await {
        let entry = result.map_err(|e| AssetsStorageError::StorageFetchFailed(e.to_string()))?;
        // Extract asset ID from storage key
        // Key structure: Twox128("Assets") + Twox128("Asset") + Blake2_128Concat(asset_id)
        // Skip 48 bytes (16+16+16) to get to the raw asset_id
        let key = entry.key_bytes();
        if key.len() >= 52
            && let Ok(asset_id) = u32::decode(&mut &key[48..])
        {
            asset_ids.push(asset_id);
        }
    }

    Ok(asset_ids)
}

/// Fetch asset details from Assets::Asset storage.
///
/// Returns decoded asset info if the asset exists, None otherwise.
pub async fn get_asset_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    asset_id: u32,
    ss58_prefix: u16,
) -> Result<Option<DecodedAssetInfo>, AssetsStorageError> {
    let storage_addr = subxt::dynamic::storage::<_, AssetDetails>("Assets", "Asset");

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (asset_id,))
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Failed to fetch asset info for asset {asset_id}: {e:?}");
            return Ok(None);
        }
    };

    let details: AssetDetails = value
        .decode()
        .map_err(|e| AssetsStorageError::DecodeFailed(e.to_string()))?;

    Ok(Some(DecodedAssetInfo {
        owner: format_account_id(&details.owner, ss58_prefix),
        issuer: format_account_id(&details.issuer, ss58_prefix),
        admin: format_account_id(&details.admin, ss58_prefix),
        freezer: format_account_id(&details.freezer, ss58_prefix),
        supply: details.supply.to_string(),
        deposit: details.deposit.to_string(),
        min_balance: details.min_balance.to_string(),
        is_sufficient: details.is_sufficient,
        accounts: details.accounts.to_string(),
        sufficients: details.sufficients.to_string(),
        approvals: details.approvals.to_string(),
        status: details.status.as_str().to_string(),
    }))
}

/// Fetch asset metadata from Assets::Metadata storage.
///
/// Returns decoded metadata if it exists, None otherwise.
pub async fn get_asset_metadata(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    asset_id: u32,
) -> Result<Option<DecodedAssetMetadata>, AssetsStorageError> {
    let storage_addr = subxt::dynamic::storage::<_, AssetMetadata>("Assets", "Metadata");

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (asset_id,))
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Failed to fetch asset metadata for asset {asset_id}: {e:?}");
            return Ok(None);
        }
    };

    let metadata: AssetMetadata = value
        .decode()
        .map_err(|e| AssetsStorageError::DecodeFailed(e.to_string()))?;

    Ok(Some(DecodedAssetMetadata {
        deposit: metadata.deposit.to_string(),
        name: format!("0x{}", hex::encode(&metadata.name)),
        symbol: format!("0x{}", hex::encode(&metadata.symbol)),
        decimals: metadata.decimals.to_string(),
        is_frozen: metadata.is_frozen,
    }))
}

/// Fetch asset balance for an account from Assets::Account storage.
///
/// Handles both modern and legacy runtime formats automatically.
pub async fn get_asset_balance(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    asset_id: u32,
    account: &AccountId32,
) -> Result<Option<DecodedAssetBalance>, AssetsStorageError> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Assets", "Account");
    let account_bytes: [u8; 32] = *account.as_ref();

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (asset_id, account_bytes))
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Failed to fetch asset balance for asset {asset_id}: {e:?}");
            return Ok(None);
        }
    };

    let raw_bytes = value.into_bytes();
    decode_asset_balance(&raw_bytes)
}

/// Fetch asset balances for multiple assets for an account.
///
/// When `show_empty` is false (default), only returns assets that have non-zero balances.
/// When `show_empty` is true, returns all requested assets including those with zero balance.
///
/// This function executes all asset queries **in parallel** for optimal performance.
/// For 100 assets, this takes ~1 network roundtrip instead of 100 sequential roundtrips.
pub async fn get_asset_balances(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    asset_ids: &[u32],
    show_empty: bool,
) -> Result<AssetBalancesResult, AssetsStorageError> {
    use futures::future::join_all;

    let account_bytes: [u8; 32] = *account.as_ref();

    // Query each asset in parallel, mapping the raw subxt result into a
    // backend-independent `AssetBalanceOutcome`. Keeping the per-asset decision out
    // of the async closure lets `collect_asset_balances` stay pure and unit-tested.
    let futures: Vec<_> = asset_ids
        .iter()
        .map(|&asset_id| {
            let storage_addr = subxt::dynamic::storage::<_, ()>("Assets", "Account");
            async move {
                // `try_fetch` returns `Ok(None)` when the account holds none of this
                // asset (a legitimate absence) and `Err` only for a genuine fetch
                // failure. The old `fetch`-based code could not tell these apart: an
                // account that does not hold an asset surfaces as
                // `StorageError::NoValueFound`, the same `Err` arm as a transient RPC
                // failure, so real failures were dropped exactly like not-held assets
                // (issue #342).
                let outcome = match client_at_block
                    .storage()
                    .try_fetch(storage_addr, (asset_id, account_bytes))
                    .await
                {
                    Ok(Some(value)) => match decode_asset_balance(&value.into_bytes()) {
                        Ok(Some(decoded)) => AssetBalanceOutcome::Decoded(decoded),
                        // `decode_asset_balance` never returns `Ok(None)`, but treat it
                        // defensively as an absence rather than a fabricated error.
                        Ok(None) => AssetBalanceOutcome::Absent,
                        // Storage that is present but undecodable is a real error, not a
                        // zero balance — surface it.
                        Err(e) => {
                            tracing::warn!(
                                "Failed to decode asset balance for asset {asset_id}: {e}"
                            );
                            AssetBalanceOutcome::Failed(e.to_string())
                        }
                    },
                    Ok(None) => AssetBalanceOutcome::Absent,
                    Err(e) => {
                        tracing::warn!("Failed to fetch asset balance for asset {asset_id}: {e:?}");
                        AssetBalanceOutcome::Failed(e.to_string())
                    }
                };
                (asset_id, outcome)
            }
        })
        .collect();

    let outcomes = join_all(futures).await;
    Ok(collect_asset_balances(outcomes, show_empty))
}

/// Fetch asset approval from Assets::Approvals storage.
///
/// Returns the approval amount and deposit if an approval exists.
pub async fn get_asset_approval(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    asset_id: u32,
    owner: &AccountId32,
    delegate: &AccountId32,
) -> Result<Option<DecodedAssetApproval>, AssetsStorageError> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("Assets", "Approvals");
    let owner_bytes: [u8; 32] = *owner.as_ref();
    let delegate_bytes: [u8; 32] = *delegate.as_ref();

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (asset_id, owner_bytes, delegate_bytes))
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Failed to fetch asset approval for asset {asset_id}: {e:?}");
            return Ok(None);
        }
    };

    let raw_bytes = value.into_bytes();
    decode_asset_approval(&raw_bytes)
}

// ================================================================================================
// Internal Decoding Functions
// ================================================================================================

/// Decode asset balance from raw SCALE bytes, handling multiple runtime versions.
fn decode_asset_balance(
    raw_bytes: &[u8],
) -> Result<Option<DecodedAssetBalance>, AssetsStorageError> {
    // Try modern format first (balance, status, reason)
    if let Ok(account) = AssetAccount::decode(&mut &raw_bytes[..]) {
        return Ok(Some(DecodedAssetBalance {
            balance: account.balance.to_string(),
            is_frozen: account.status.is_frozen(),
            is_sufficient: account.reason.is_sufficient(),
        }));
    }

    // Fall back to legacy format (balance, is_frozen, sufficient)
    if let Ok(account) = AssetAccountLegacy::decode(&mut &raw_bytes[..]) {
        return Ok(Some(DecodedAssetBalance {
            balance: account.balance.to_string(),
            is_frozen: account.is_frozen,
            is_sufficient: account.sufficient,
        }));
    }

    // If neither format works, return an error
    Err(AssetsStorageError::DecodeFailed(
        "Failed to decode asset account: unknown format".to_string(),
    ))
}

/// Decode asset approval from raw SCALE bytes.
fn decode_asset_approval(
    raw_bytes: &[u8],
) -> Result<Option<DecodedAssetApproval>, AssetsStorageError> {
    if let Ok(approval) = AssetApproval::decode(&mut &raw_bytes[..]) {
        return Ok(Some(DecodedAssetApproval {
            amount: approval.amount.to_string(),
            deposit: approval.deposit.to_string(),
        }));
    }

    Err(AssetsStorageError::DecodeFailed(
        "Failed to decode asset approval: unknown format".to_string(),
    ))
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use crate::handlers::runtime_queries::assets_common::{
        AccountStatus, AssetStatus, ExistenceReason,
    };

    #[test]
    fn test_asset_status_as_str() {
        assert_eq!(AssetStatus::Live.as_str(), "Live");
        assert_eq!(AssetStatus::Frozen.as_str(), "Frozen");
        assert_eq!(AssetStatus::Destroying.as_str(), "Destroying");
    }

    #[test]
    fn test_account_status_is_frozen() {
        assert!(!AccountStatus::Liquid.is_frozen());
        assert!(AccountStatus::Frozen.is_frozen());
        assert!(AccountStatus::Blocked.is_frozen());
    }

    #[test]
    fn test_existence_reason_is_sufficient() {
        assert!(!ExistenceReason::Consumer.is_sufficient());
        assert!(ExistenceReason::Sufficient.is_sufficient());
        assert!(!ExistenceReason::DepositRefunded.is_sufficient());
    }

    use super::{
        AssetBalanceFetchError, AssetBalanceOutcome, DecodedAssetBalance, collect_asset_balances,
    };

    fn bal(amount: &str) -> DecodedAssetBalance {
        DecodedAssetBalance {
            balance: amount.to_string(),
            is_frozen: false,
            is_sufficient: false,
        }
    }

    #[test]
    fn collect_decoded_outcomes_go_to_balances() {
        let out = vec![(1u32, AssetBalanceOutcome::Decoded(bal("100")))];
        let result = collect_asset_balances(out, false);
        assert_eq!(result.balances, vec![(1u32, bal("100"))]);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn collect_absent_is_omitted_when_show_empty_false() {
        let out = vec![(7u32, AssetBalanceOutcome::Absent)];
        let result = collect_asset_balances(out, false);
        assert!(result.balances.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn collect_absent_is_zero_stub_when_show_empty_true() {
        let out = vec![(7u32, AssetBalanceOutcome::Absent)];
        let result = collect_asset_balances(out, true);
        assert_eq!(result.balances, vec![(7u32, bal("0"))]);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn collect_failed_goes_to_errors_never_balances() {
        let out = vec![(9u32, AssetBalanceOutcome::Failed("rpc boom".to_string()))];
        let result = collect_asset_balances(out, false);
        assert!(result.balances.is_empty());
        assert_eq!(
            result.errors,
            vec![AssetBalanceFetchError {
                asset_id: 9,
                reason: "rpc boom".to_string()
            }]
        );
    }

    #[test]
    fn collect_failed_is_not_zero_stubbed_even_with_show_empty() {
        // Ghost-zero guard (issue #342): a failed fetch must never become a zero balance,
        // even when the caller asked to include empty balances.
        let out = vec![(9u32, AssetBalanceOutcome::Failed("rpc boom".to_string()))];
        let result = collect_asset_balances(out, true);
        assert!(result.balances.is_empty());
        assert_eq!(result.errors.len(), 1);
    }
}
