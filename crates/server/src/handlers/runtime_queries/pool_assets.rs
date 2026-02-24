// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! PoolAssets pallet storage query functions.
//!
//! This module provides standalone functions for querying PoolAssets pallet storage items.
//! Pool assets are LP (liquidity pool) tokens created by the AssetConversion pallet.
//!
//! # Storage Items Covered
//! - `PoolAssets::Asset` - Pool asset details (owner, issuer, supply, etc.)
//! - `PoolAssets::Metadata` - Pool asset metadata (name, symbol, decimals)
//! - `PoolAssets::Account` - Account balances for pool assets
//! - `PoolAssets::Approvals` - Approval amounts for pool asset transfers

use parity_scale_codec::Decode;
use sp_core::crypto::{AccountId32, Ss58Codec};
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Errors that can occur when querying PoolAssets pallet storage.
#[derive(Debug, Error)]
pub enum PoolAssetsStorageError {
    /// The PoolAssets pallet is not available on this chain.
    #[error("PoolAssets pallet not available")]
    PalletNotAvailable,

    /// The requested pool asset was not found.
    #[error("Pool asset {0} not found")]
    PoolAssetNotFound(u32),

    /// Failed to decode storage value.
    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(String),

    /// Storage fetch failed.
    #[error("Storage fetch failed: {0}")]
    StorageFetchFailed(String),
}

// ================================================================================================
// SCALE Decode Types - PoolAssets::Asset (with DecodeAsType for dynamic storage)
// ================================================================================================

/// Asset status enum for modern runtimes.
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub enum AssetStatus {
    Live,
    Frozen,
    Destroying,
}

impl AssetStatus {
    /// Returns the status as a string for API responses.
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetStatus::Live => "Live",
            AssetStatus::Frozen => "Frozen",
            AssetStatus::Destroying => "Destroying",
        }
    }
}

/// Asset details from PoolAssets::Asset storage.
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct AssetDetails {
    pub owner: [u8; 32],
    pub issuer: [u8; 32],
    pub admin: [u8; 32],
    pub freezer: [u8; 32],
    pub supply: u128,
    pub deposit: u128,
    pub min_balance: u128,
    pub is_sufficient: bool,
    pub accounts: u32,
    pub sufficients: u32,
    pub approvals: u32,
    pub status: AssetStatus,
}

// ================================================================================================
// SCALE Decode Types - PoolAssets::Metadata (with DecodeAsType for dynamic storage)
// ================================================================================================

/// Asset metadata from PoolAssets::Metadata storage.
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct AssetMetadataStorage {
    pub deposit: u128,
    pub name: Vec<u8>,
    pub symbol: Vec<u8>,
    pub decimals: u8,
    pub is_frozen: bool,
}

// ================================================================================================
// SCALE Decode Types - PoolAssets::Account
// ================================================================================================

/// Account status for a pool asset account (modern runtimes).
#[derive(Debug, Clone, Decode)]
enum AccountStatus {
    Liquid,
    Frozen,
    Blocked,
}

impl AccountStatus {
    fn is_frozen(&self) -> bool {
        matches!(self, AccountStatus::Frozen | AccountStatus::Blocked)
    }
}

/// Existence reason for a pool asset account (modern runtimes).
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)]
enum ExistenceReason {
    Consumer,
    Sufficient,
    DepositHeld(u128),
    DepositRefunded,
    DepositFrom([u8; 32], u128),
}

impl ExistenceReason {
    fn is_sufficient(&self) -> bool {
        matches!(self, ExistenceReason::Sufficient)
    }
}

/// Modern PoolAssetAccount structure (current runtimes with status/reason fields).
#[derive(Debug, Clone, Decode)]
struct PoolAssetAccountModern {
    balance: u128,
    status: AccountStatus,
    reason: ExistenceReason,
}

/// Legacy PoolAssetAccount structure (older runtimes with is_frozen/sufficient booleans).
#[derive(Debug, Clone, Decode)]
struct PoolAssetAccountLegacy {
    balance: u128,
    is_frozen: bool,
    sufficient: bool,
}

// ================================================================================================
// SCALE Decode Types - PoolAssets::Approvals
// ================================================================================================

/// Pool asset approval from PoolAssets::Approvals storage.
#[derive(Debug, Clone, Decode)]
struct PoolAssetApprovalStorage {
    #[codec(compact)]
    amount: u128,
    #[codec(compact)]
    deposit: u128,
}

// ================================================================================================
// Public Data Types (Decoded/Formatted)
// ================================================================================================

/// Decoded pool asset information ready for API response.
#[derive(Debug, Clone)]
pub struct DecodedPoolAssetInfo {
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

/// Decoded pool asset metadata ready for API response.
#[derive(Debug, Clone)]
pub struct DecodedPoolAssetMetadata {
    pub deposit: String,
    pub name: String,
    pub symbol: String,
    pub decimals: String,
    pub is_frozen: bool,
}

/// Decoded pool asset balance for an account.
#[derive(Debug, Clone)]
pub struct DecodedPoolAssetBalance {
    pub balance: String,
    pub is_frozen: bool,
    pub is_sufficient: bool,
}

/// Decoded pool asset approval.
#[derive(Debug, Clone)]
pub struct DecodedPoolAssetApproval {
    pub amount: String,
    pub deposit: String,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Check if the PoolAssets pallet exists on the chain.
pub fn is_pool_assets_pallet_available(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> bool {
    client_at_block
        .storage()
        .entry(("PoolAssets", "Asset"))
        .is_ok()
}

/// Fetch all pool asset IDs from PoolAssets::Asset storage.
///
/// Returns a list of all pool asset IDs that exist on the chain.
pub async fn get_all_pool_asset_ids(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<u32>, PoolAssetsStorageError> {
    let storage_query = subxt::storage::dynamic::<Vec<u32>, Vec<u8>>("PoolAssets", "Asset");
    let storage_entry = client_at_block
        .storage()
        .entry(storage_query)
        .map_err(|_| PoolAssetsStorageError::PalletNotAvailable)?;

    let mut asset_ids = Vec::new();
    let mut values = storage_entry
        .iter(Vec::<u32>::new())
        .await
        .map_err(|e| PoolAssetsStorageError::StorageFetchFailed(e.to_string()))?;

    while let Some(result) = values.next().await {
        let entry =
            result.map_err(|e| PoolAssetsStorageError::StorageFetchFailed(e.to_string()))?;
        // Extract asset ID from storage key
        // Key structure: Twox128("PoolAssets") + Twox128("Asset") + Blake2_128Concat(asset_id)
        // Skip 48 bytes (16+16+16) to get to the raw asset_id
        let key = entry.key_bytes();
        if key.len() >= 52 && let Ok(asset_id) = u32::decode(&mut &key[48..]) {
            asset_ids.push(asset_id);
        }
    }

    Ok(asset_ids)
}

/// Fetch pool asset details from PoolAssets::Asset storage.
///
/// Returns decoded pool asset info if the asset exists, None otherwise.
pub async fn get_pool_asset_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    asset_id: u32,
    ss58_prefix: u16,
) -> Result<Option<DecodedPoolAssetInfo>, PoolAssetsStorageError> {
    let storage_addr = subxt::dynamic::storage::<_, AssetDetails>("PoolAssets", "Asset");

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (asset_id,))
        .await
    {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let details: AssetDetails = value
        .decode()
        .map_err(|e| PoolAssetsStorageError::DecodeFailed(e.to_string()))?;

    Ok(Some(DecodedPoolAssetInfo {
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

/// Fetch pool asset metadata from PoolAssets::Metadata storage.
///
/// Returns decoded metadata if it exists, None otherwise.
pub async fn get_pool_asset_metadata(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    asset_id: u32,
) -> Result<Option<DecodedPoolAssetMetadata>, PoolAssetsStorageError> {
    let storage_addr = subxt::dynamic::storage::<_, AssetMetadataStorage>("PoolAssets", "Metadata");

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (asset_id,))
        .await
    {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let metadata: AssetMetadataStorage = value
        .decode()
        .map_err(|e| PoolAssetsStorageError::DecodeFailed(e.to_string()))?;

    Ok(Some(DecodedPoolAssetMetadata {
        deposit: metadata.deposit.to_string(),
        name: format!("0x{}", hex::encode(&metadata.name)),
        symbol: format!("0x{}", hex::encode(&metadata.symbol)),
        decimals: metadata.decimals.to_string(),
        is_frozen: metadata.is_frozen,
    }))
}

/// Fetch pool asset balance for an account from PoolAssets::Account storage.
///
/// Handles both modern and legacy runtime formats automatically.
pub async fn get_pool_asset_balance(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    asset_id: u32,
    account: &AccountId32,
) -> Result<Option<DecodedPoolAssetBalance>, PoolAssetsStorageError> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("PoolAssets", "Account");
    let account_bytes: [u8; 32] = *account.as_ref();

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (asset_id, account_bytes))
        .await
    {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let raw_bytes = value.into_bytes();
    decode_pool_asset_balance(&raw_bytes)
}

/// Fetch pool asset balances for multiple assets for an account.
///
/// Returns balances for all requested assets that have non-zero balances.
pub async fn get_pool_asset_balances(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    asset_ids: &[u32],
) -> Result<Vec<(u32, DecodedPoolAssetBalance)>, PoolAssetsStorageError> {
    let account_bytes: [u8; 32] = *account.as_ref();
    let mut balances = Vec::new();

    for &asset_id in asset_ids {
        let storage_addr = subxt::dynamic::storage::<_, ()>("PoolAssets", "Account");

        if let Ok(value) = client_at_block
            .storage()
            .fetch(storage_addr, (asset_id, account_bytes))
            .await
        {
            let raw_bytes = value.into_bytes();
            if let Ok(Some(decoded)) = decode_pool_asset_balance(&raw_bytes) {
                balances.push((asset_id, decoded));
            }
        }
    }

    Ok(balances)
}

/// Fetch pool asset approval from PoolAssets::Approvals storage.
///
/// Returns the approval amount and deposit if an approval exists.
pub async fn get_pool_asset_approval(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    asset_id: u32,
    owner: &AccountId32,
    delegate: &AccountId32,
) -> Result<Option<DecodedPoolAssetApproval>, PoolAssetsStorageError> {
    let storage_addr = subxt::dynamic::storage::<_, ()>("PoolAssets", "Approvals");
    let owner_bytes: [u8; 32] = *owner.as_ref();
    let delegate_bytes: [u8; 32] = *delegate.as_ref();

    let value = match client_at_block
        .storage()
        .fetch(storage_addr, (asset_id, owner_bytes, delegate_bytes))
        .await
    {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let raw_bytes = value.into_bytes();
    decode_pool_asset_approval(&raw_bytes)
}

// ================================================================================================
// Internal Decoding Functions
// ================================================================================================

/// Decode pool asset balance from raw SCALE bytes, handling multiple runtime versions.
fn decode_pool_asset_balance(
    raw_bytes: &[u8],
) -> Result<Option<DecodedPoolAssetBalance>, PoolAssetsStorageError> {
    // Try modern format first (balance, status, reason)
    if let Ok(account) = PoolAssetAccountModern::decode(&mut &raw_bytes[..]) {
        return Ok(Some(DecodedPoolAssetBalance {
            balance: account.balance.to_string(),
            is_frozen: account.status.is_frozen(),
            is_sufficient: account.reason.is_sufficient(),
        }));
    }

    // Fall back to legacy format (balance, is_frozen, sufficient)
    if let Ok(account) = PoolAssetAccountLegacy::decode(&mut &raw_bytes[..]) {
        return Ok(Some(DecodedPoolAssetBalance {
            balance: account.balance.to_string(),
            is_frozen: account.is_frozen,
            is_sufficient: account.sufficient,
        }));
    }

    // If neither format works, return an error
    Err(PoolAssetsStorageError::DecodeFailed(
        "Failed to decode pool asset account: unknown format".to_string(),
    ))
}

/// Decode pool asset approval from raw SCALE bytes.
fn decode_pool_asset_approval(
    raw_bytes: &[u8],
) -> Result<Option<DecodedPoolAssetApproval>, PoolAssetsStorageError> {
    if let Ok(approval) = PoolAssetApprovalStorage::decode(&mut &raw_bytes[..]) {
        return Ok(Some(DecodedPoolAssetApproval {
            amount: approval.amount.to_string(),
            deposit: approval.deposit.to_string(),
        }));
    }

    Err(PoolAssetsStorageError::DecodeFailed(
        "Failed to decode pool asset approval: unknown format".to_string(),
    ))
}

// ================================================================================================
// Helper Functions
// ================================================================================================

/// Format a 32-byte account ID as SS58 address.
fn format_account_id(bytes: &[u8; 32], ss58_prefix: u16) -> String {
    AccountId32::from(*bytes).to_ss58check_with_version(ss58_prefix.into())
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
}
