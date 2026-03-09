// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common types for Assets-like pallets (Assets, PoolAssets, ForeignAssets).
//!
//! This module provides shared SCALE decode types used by all asset pallets.
//! Each pallet uses the same underlying storage structure for:
//! - Asset details (owner, issuer, supply, etc.)
//! - Asset metadata (name, symbol, decimals)
//! - Account balances
//! - Approvals

use parity_scale_codec::Decode;
use sp_core::crypto::{AccountId32, Ss58Codec};
use subxt::ext::scale_decode::DecodeAsType;

// ================================================================================================
// Helper Functions
// ================================================================================================

/// Formats a 32-byte account ID as an SS58 address string.
///
/// This is used to display account addresses (owner, issuer, admin, freezer)
/// in the human-readable SS58 format with the appropriate network prefix.
pub fn format_account_id(bytes: &[u8; 32], ss58_prefix: u16) -> String {
    AccountId32::from(*bytes).to_ss58check_with_version(ss58_prefix.into())
}

// ================================================================================================
// Asset Status
// ================================================================================================

/// Asset status enum used by all asset pallets.
#[derive(Debug, Clone, Decode, DecodeAsType)]
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

// ================================================================================================
// Asset Details
// ================================================================================================

/// Asset details structure from Asset/PoolAssets/ForeignAssets::Asset storage.
/// This is the common format used across all asset pallets.
#[derive(Debug, Clone, Decode, DecodeAsType)]
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
// Asset Metadata
// ================================================================================================

/// Asset metadata structure from Asset/PoolAssets/ForeignAssets::Metadata storage.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct AssetMetadata {
    pub deposit: u128,
    pub name: Vec<u8>,
    pub symbol: Vec<u8>,
    pub decimals: u8,
    pub is_frozen: bool,
}

// ================================================================================================
// Account Balance Types
// ================================================================================================

/// Account status enum used in modern asset account storage.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub enum AccountStatus {
    Liquid,
    Frozen,
    Blocked,
}

impl AccountStatus {
    /// Returns true if the account is frozen or blocked.
    pub fn is_frozen(&self) -> bool {
        matches!(self, AccountStatus::Frozen | AccountStatus::Blocked)
    }
}

/// Existence reason enum for modern asset accounts.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
#[allow(dead_code)]
pub enum ExistenceReason {
    Consumer,
    Sufficient,
    DepositHeld(u128),
    DepositRefunded,
    DepositFrom([u8; 32], u128),
}

impl ExistenceReason {
    /// Returns true if the existence reason is Sufficient.
    pub fn is_sufficient(&self) -> bool {
        matches!(self, ExistenceReason::Sufficient)
    }
}

/// Modern asset account structure (with status and reason fields).
/// Used by current runtimes.
#[derive(Debug, Clone, Decode, DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
pub struct AssetAccount {
    pub balance: u128,
    pub status: AccountStatus,
    pub reason: ExistenceReason,
    pub extra: (),
}

/// Legacy asset account structure (with is_frozen and sufficient fields).
/// Used as fallback for older runtimes.
#[derive(Debug, Clone, Decode)]
pub struct AssetAccountLegacy {
    pub balance: u128,
    pub is_frozen: bool,
    pub sufficient: bool,
}

// ================================================================================================
// Approvals
// ================================================================================================

/// Approval structure from Asset/PoolAssets/ForeignAssets::Approvals storage.
#[derive(Debug, Clone, Decode)]
pub struct AssetApproval {
    pub amount: u128,
    pub deposit: u128,
}
