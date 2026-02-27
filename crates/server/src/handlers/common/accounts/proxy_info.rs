// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common proxy info utilities shared across handler modules.

use crate::handlers::runtime_queries::balances as balances_queries;
use crate::utils::ResolvedBlock;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

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
// Core Query Function
// ================================================================================================

/// Query proxy info from storage
///
/// This is the shared function used by both `/accounts/:accountId/proxy-info`
/// and `/rc/accounts/:accountId/proxy-info` endpoints.
///
/// # Arguments
/// * `client_at_block` - The subxt client at a specific block
/// * `account` - The account to query proxy info for
/// * `block` - The resolved block information
/// * `ss58_prefix` - The SS58 prefix to use for encoding delegate addresses
pub async fn query_proxy_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    block: &ResolvedBlock,
    ss58_prefix: u16,
) -> Result<RawProxyInfo, ProxyQueryError> {
    // Check if Proxy pallet exists
    if client_at_block
        .storage()
        .entry(("Proxy", "Proxies"))
        .is_err()
    {
        return Err(ProxyQueryError::ProxyPalletNotAvailable);
    }

    // Use centralized query function
    let (delegated_accounts, deposit_held) = if let Some((proxies, deposit)) =
        balances_queries::get_proxy_definitions(client_at_block, account, ss58_prefix).await
    {
        let definitions = proxies
            .into_iter()
            .map(|p| DecodedProxyDefinition {
                delegate: p.delegate,
                proxy_type: proxy_type_name(p.proxy_type),
                delay: p.delay.to_string(),
            })
            .collect();
        (definitions, deposit.to_string())
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
