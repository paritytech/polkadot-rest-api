// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! ForeignAssets pallet storage query functions.
//!
//! This module provides standalone functions for querying ForeignAssets pallet storage items.
//! ForeignAssets uses XCM Location as the asset identifier key.

use super::assets_common::{
    AccountStatus, AssetAccount, AssetDetails, AssetMetadata, ExistenceReason,
};
use crate::handlers::common::xcm_types::{BLAKE2_128_HASH_LEN, Location};
use parity_scale_codec::Decode;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// Note: All SCALE decode types (AssetStatus, AssetDetails, AssetMetadata,
// AccountStatus, ExistenceReason, AssetAccount) are imported from the assets_common module.

// ================================================================================================
// Public Data Types
// ================================================================================================

/// Decoded foreign asset info from ForeignAssets::Asset storage
#[derive(Debug, Clone)]
pub struct DecodedForeignAssetInfo {
    /// The XCM Location identifier
    pub location: Location,
    /// Owner account (raw bytes)
    pub owner: [u8; 32],
    /// Issuer account (raw bytes)
    pub issuer: [u8; 32],
    /// Admin account (raw bytes)
    pub admin: [u8; 32],
    /// Freezer account (raw bytes)
    pub freezer: [u8; 32],
    /// Total supply
    pub supply: u128,
    /// Deposit held
    pub deposit: u128,
    /// Minimum balance
    pub min_balance: u128,
    /// Whether asset is sufficient
    pub is_sufficient: bool,
    /// Number of accounts
    pub accounts: u32,
    /// Number of sufficient accounts
    pub sufficients: u32,
    /// Number of approvals
    pub approvals: u32,
    /// Asset status
    pub status: String,
}

/// Decoded foreign asset metadata from ForeignAssets::Metadata storage
#[derive(Debug, Clone)]
pub struct DecodedForeignAssetMetadata {
    /// The XCM Location identifier
    pub location: Location,
    /// Deposit held for metadata
    pub deposit: u128,
    /// Asset name (raw bytes)
    pub name: Vec<u8>,
    /// Asset symbol (raw bytes)
    pub symbol: Vec<u8>,
    /// Decimal places
    pub decimals: u8,
    /// Whether asset is frozen
    pub is_frozen: bool,
}

/// Decoded foreign asset account balance
#[derive(Debug, Clone)]
pub struct DecodedForeignAssetBalance {
    /// The XCM Location identifier
    pub location: Location,
    /// Account balance
    pub balance: u128,
    /// Whether account is frozen
    pub is_frozen: bool,
    /// Whether asset is sufficient for the account
    pub is_sufficient: bool,
}

// ================================================================================================
// Storage Query Functions
// ================================================================================================

/// Iterate all foreign asset locations from ForeignAssets::Asset storage.
/// Returns a list of all registered XCM Locations.
pub async fn iter_foreign_asset_locations(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<Vec<Location>> {
    let storage_addr = subxt::dynamic::storage::<(Location,), ()>("ForeignAssets", "Asset");

    let mut stream = client_at_block
        .storage()
        .iter(storage_addr, ())
        .await
        .ok()?;

    let mut locations = Vec::new();

    while let Some(result) = stream.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Extract Location from storage key
        if let Ok(key) = entry.key()
            && let Some(key_part) = key.part(0)
        {
            let bytes = key_part.bytes();
            if bytes.len() > BLAKE2_128_HASH_LEN {
                let location_bytes = &bytes[BLAKE2_128_HASH_LEN..];
                if let Ok(location) = Location::decode(&mut &location_bytes[..]) {
                    locations.push(location);
                }
            }
        }
    }

    Some(locations)
}

/// Iterate all foreign assets with their details from ForeignAssets::Asset storage.
pub async fn iter_foreign_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<Vec<DecodedForeignAssetInfo>> {
    let storage_addr =
        subxt::dynamic::storage::<(Location,), AssetDetails>("ForeignAssets", "Asset");

    let mut stream = client_at_block
        .storage()
        .iter(storage_addr, ())
        .await
        .ok()?;

    let mut assets = Vec::new();

    while let Some(result) = stream.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Extract Location from storage key
        let location = if let Ok(key) = entry.key()
            && let Some(key_part) = key.part(0)
        {
            let bytes = key_part.bytes();
            if bytes.len() > BLAKE2_128_HASH_LEN {
                let location_bytes = &bytes[BLAKE2_128_HASH_LEN..];
                Location::decode(&mut &location_bytes[..]).ok()
            } else {
                None
            }
        } else {
            None
        };

        let location = match location {
            Some(l) => l,
            None => continue,
        };

        // Decode asset details
        if let Ok(details) = entry.value().decode() {
            assets.push(DecodedForeignAssetInfo {
                location,
                owner: details.owner,
                issuer: details.issuer,
                admin: details.admin,
                freezer: details.freezer,
                supply: details.supply,
                deposit: details.deposit,
                min_balance: details.min_balance,
                is_sufficient: details.is_sufficient,
                accounts: details.accounts,
                sufficients: details.sufficients,
                approvals: details.approvals,
                status: details.status.as_str().to_string(),
            });
        }
    }

    Some(assets)
}

/// Iterate all foreign asset metadata from ForeignAssets::Metadata storage.
pub async fn iter_foreign_asset_metadata(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<Vec<DecodedForeignAssetMetadata>> {
    let storage_addr =
        subxt::dynamic::storage::<(Location,), AssetMetadata>("ForeignAssets", "Metadata");

    let mut stream = client_at_block
        .storage()
        .iter(storage_addr, ())
        .await
        .ok()?;

    let mut metadata_list = Vec::new();

    while let Some(result) = stream.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Extract Location from storage key
        let location = if let Ok(key) = entry.key()
            && let Some(key_part) = key.part(0)
        {
            let bytes = key_part.bytes();
            if bytes.len() > BLAKE2_128_HASH_LEN {
                let location_bytes = &bytes[BLAKE2_128_HASH_LEN..];
                Location::decode(&mut &location_bytes[..]).ok()
            } else {
                None
            }
        } else {
            None
        };

        let location = match location {
            Some(l) => l,
            None => continue,
        };

        // Decode metadata
        if let Ok(metadata) = entry.value().decode() {
            metadata_list.push(DecodedForeignAssetMetadata {
                location,
                deposit: metadata.deposit,
                name: metadata.name,
                symbol: metadata.symbol,
                decimals: metadata.decimals,
                is_frozen: metadata.is_frozen,
            });
        }
    }

    Some(metadata_list)
}

/// Get foreign asset balance for a specific account and location.
pub async fn get_foreign_asset_balance(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    location: &Location,
) -> Option<DecodedForeignAssetBalance> {
    let storage_addr =
        subxt::dynamic::storage::<(Location, [u8; 32]), AssetAccount>("ForeignAssets", "Account");

    let account_bytes: [u8; 32] = *account.as_ref();

    let value = client_at_block
        .storage()
        .fetch(storage_addr, (location.clone(), account_bytes))
        .await
        .ok()?;

    let asset_account: AssetAccount = value.decode().ok()?;

    // Skip zero-balance entries
    if asset_account.balance == 0 {
        return None;
    }

    let is_frozen = matches!(
        asset_account.status,
        AccountStatus::Frozen | AccountStatus::Blocked
    );
    let is_sufficient = matches!(asset_account.reason, ExistenceReason::Sufficient);

    Some(DecodedForeignAssetBalance {
        location: location.clone(),
        balance: asset_account.balance,
        is_frozen,
        is_sufficient,
    })
}

/// Get foreign asset balances for all locations for an account.
pub async fn get_all_foreign_asset_balances(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
) -> Vec<DecodedForeignAssetBalance> {
    // First get all locations
    let locations = match iter_foreign_asset_locations(client_at_block).await {
        Some(l) => l,
        None => return vec![],
    };

    let mut balances = Vec::new();
    let account_bytes: [u8; 32] = *account.as_ref();

    for location in locations {
        let storage_addr = subxt::dynamic::storage::<(Location, [u8; 32]), AssetAccount>(
            "ForeignAssets",
            "Account",
        );

        let result = client_at_block
            .storage()
            .fetch(storage_addr, (location.clone(), account_bytes))
            .await;

        if let Ok(value) = result
            && let Ok(asset_account) = value.decode()
        {
            // Skip zero-balance entries
            if asset_account.balance == 0 {
                continue;
            }

            let is_frozen = matches!(
                asset_account.status,
                AccountStatus::Frozen | AccountStatus::Blocked
            );
            let is_sufficient = matches!(asset_account.reason, ExistenceReason::Sufficient);

            balances.push(DecodedForeignAssetBalance {
                location,
                balance: asset_account.balance,
                is_frozen,
                is_sufficient,
            });
        }
    }

    balances
}

// ================================================================================================
// Additional Query Functions
// ================================================================================================

/// Get foreign asset account balance for a specific (location, account) pair.
/// Returns None if the account doesn't exist, has zero balance, or decoding fails.
pub async fn get_foreign_asset_account(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    location: &Location,
    account_bytes: &[u8; 32],
) -> Option<DecodedForeignAssetBalance> {
    let storage_addr =
        subxt::dynamic::storage::<(Location, [u8; 32]), AssetAccount>("ForeignAssets", "Account");

    let result = client_at_block
        .storage()
        .fetch(storage_addr, (location.clone(), *account_bytes))
        .await;

    match result {
        Ok(value) => {
            match value.decode() {
                Ok(asset_account) => {
                    // Skip zero-balance entries
                    if asset_account.balance == 0 {
                        return None;
                    }

                    let is_frozen = asset_account.status.is_frozen();
                    let is_sufficient = asset_account.reason.is_sufficient();

                    Some(DecodedForeignAssetBalance {
                        location: location.clone(),
                        balance: asset_account.balance,
                        is_frozen,
                        is_sufficient,
                    })
                }
                Err(_) => None,
            }
        }
        Err(_) => None, // No entry for this (location, account) pair
    }
}

/// Fallback fetch for older runtimes using scale_value dynamic decoding.
/// Returns the raw scale_value Value for the caller to extract fields from.
pub async fn get_foreign_asset_account_raw(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    location: &Location,
    account_bytes: &[u8; 32],
) -> Result<Option<scale_value::Value<()>>, &'static str> {
    let storage_addr =
        subxt::dynamic::storage::<(Location, [u8; 32]), ()>("ForeignAssets", "Account");

    let result = client_at_block
        .storage()
        .fetch(storage_addr, (location.clone(), *account_bytes))
        .await;

    match result {
        Ok(value) => match value.decode_as::<scale_value::Value<()>>() {
            Ok(decoded) => Ok(Some(decoded)),
            Err(_) => Err("Failed to decode ForeignAssets::Account as scale_value"),
        },
        Err(_) => Ok(None), // No entry for this (location, account) pair
    }
}
