// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common proxy info utilities shared across handler modules.

use crate::utils::ResolvedBlock;
use parity_scale_codec::Decode;
use sp_core::crypto::{AccountId32, Ss58Codec};
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

// ================================================================================================
// SCALE Decode Types for Proxy::Proxies storage
// ================================================================================================

/// Proxy definition structure
/// Note: proxy_type is stored as u8 (variant index) since the actual enum varies by runtime
#[derive(Debug, Clone, Decode)]
struct ProxyDefinition {
    delegate: [u8; 32],
    proxy_type: u8, // Variant index - mapped to name later
    delay: u32,
}

/// Proxies storage value: (BoundedVec<ProxyDefinition>, Balance)
/// We decode as (Vec<ProxyDefinition>, u128)
#[derive(Debug, Clone, Decode)]
struct ProxiesStorageValue {
    definitions: Vec<ProxyDefinition>,
    deposit: u128,
}

/// Map proxy type variant index to common names
/// These are the common proxy types across Polkadot/Kusama runtimes
fn proxy_type_name(index: u8) -> String {
    match index {
        0 => "Any".to_string(),
        1 => "NonTransfer".to_string(),
        2 => "Governance".to_string(),
        3 => "Staking".to_string(),
        4 => "IdentityJudgement".to_string(),
        5 => "CancelProxy".to_string(),
        6 => "Auction".to_string(),
        7 => "NominationPools".to_string(),
        _ => format!("Unknown({})", index),
    }
}

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum ProxyQueryError {
    #[error("The runtime does not include the proxy pallet at this block")]
    ProxyPalletNotAvailable,

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(Box<subxt::error::OnlineClientAtBlockError>),

    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(Box<subxt::error::StorageError>),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(#[from] parity_scale_codec::Error),
}

impl From<subxt::error::OnlineClientAtBlockError> for ProxyQueryError {
    fn from(err: subxt::error::OnlineClientAtBlockError) -> Self {
        ProxyQueryError::ClientAtBlockFailed(Box::new(err))
    }
}

impl From<subxt::error::StorageError> for ProxyQueryError {
    fn from(err: subxt::error::StorageError) -> Self {
        ProxyQueryError::StorageQueryFailed(Box::new(err))
    }
}

// ================================================================================================
// Data Types
// ================================================================================================

/// Raw proxy info data returned from storage query
#[derive(Debug)]
pub struct RawProxyInfo {
    /// Block information
    pub block: FormattedBlockInfo,
    /// List of proxy definitions
    pub delegated_accounts: Vec<DecodedProxyDefinition>,
    /// Deposit held for proxies
    pub deposit_held: String,
}

/// Block information for response
#[derive(Debug, Clone)]
pub struct FormattedBlockInfo {
    pub hash: String,
    pub number: u64,
}

/// Decoded proxy definition from storage
#[derive(Debug, Clone)]
pub struct DecodedProxyDefinition {
    /// The delegate address (SS58 encoded)
    pub delegate: String,
    /// The type of proxy
    pub proxy_type: String,
    /// The announcement delay in blocks
    pub delay: String,
}

// ================================================================================================
// Core Query Function
// ================================================================================================

/// Query proxy info from storage
///
/// This is the shared function used by both `/accounts/:accountId/proxy-info`
/// and `/rc/accounts/:accountId/proxy-info` endpoints.
pub async fn query_proxy_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &ResolvedBlock,
) -> Result<RawProxyInfo, ProxyQueryError> {
    // Check if Proxy pallet exists
    if client_at_block
        .storage()
        .entry(("Proxy", "Proxies"))
        .is_err()
    {
        return Err(ProxyQueryError::ProxyPalletNotAvailable);
    }

    // Build the storage address for Proxy::Proxies(account)
    let storage_addr = subxt::dynamic::storage::<_, ()>("Proxy", "Proxies");
    let account_bytes: [u8; 32] = *account.as_ref();

    let storage_value = client_at_block
        .storage()
        .fetch(storage_addr, (account_bytes,))
        .await;

    let (delegated_accounts, deposit_held) = if let Ok(value) = storage_value {
        let raw_bytes = value.into_bytes();
        decode_proxy_info(&raw_bytes)?
    } else {
        (Vec::new(), "0".to_string())
    };

    Ok(RawProxyInfo {
        block: FormattedBlockInfo {
            hash: block.hash.clone(),
            number: block.number,
        },
        delegated_accounts,
        deposit_held,
    })
}

// ================================================================================================
// Proxy Info Decoding
// ================================================================================================

/// Decode proxy info from raw SCALE bytes
/// The storage value is a tuple: (Vec<ProxyDefinition>, Balance)
fn decode_proxy_info(
    raw_bytes: &[u8],
) -> Result<(Vec<DecodedProxyDefinition>, String), ProxyQueryError> {
    // Decode as ProxiesStorageValue
    if let Ok(proxies) = ProxiesStorageValue::decode(&mut &raw_bytes[..]) {
        let definitions = proxies
            .definitions
            .into_iter()
            .map(|def| {
                let account_id = AccountId32::from(def.delegate);
                DecodedProxyDefinition {
                    delegate: account_id.to_ss58check(),
                    proxy_type: proxy_type_name(def.proxy_type),
                    delay: def.delay.to_string(),
                }
            })
            .collect();

        return Ok((definitions, proxies.deposit.to_string()));
    }

    // If decoding fails, return empty
    Ok((Vec::new(), "0".to_string()))
}
