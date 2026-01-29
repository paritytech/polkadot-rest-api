//! Common proxy info utilities shared across handler modules.

use crate::utils::ResolvedBlock;
use scale_value::{Composite, Value, ValueDef};
use sp_core::crypto::{AccountId32, Ss58Codec};
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
    let storage_query =
        subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Proxy", "Proxies");

    let proxy_exists = client_at_block
        .storage()
        .entry(storage_query.clone())
        .is_ok();

    if !proxy_exists {
        return Err(ProxyQueryError::ProxyPalletNotAvailable);
    }

    let storage_entry = client_at_block.storage().entry(storage_query)?;

    // Storage key for Proxies: (account)
    let account_bytes: [u8; 32] = *account.as_ref();
    let key = vec![Value::from_bytes(account_bytes)];
    let storage_value = storage_entry.try_fetch(key).await?;

    let (delegated_accounts, deposit_held) = if let Some(value) = storage_value {
        decode_proxy_info(&value).await?
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

/// Decode proxy info from storage value
/// The storage value is a tuple: (Vec<ProxyDefinition>, Balance)
async fn decode_proxy_info(
    value: &subxt::storage::StorageValue<'_, scale_value::Value>,
) -> Result<(Vec<DecodedProxyDefinition>, String), ProxyQueryError> {
    // Decode as scale_value::Value to inspect structure
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        ProxyQueryError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode storage value",
        ))
    })?;

    // The storage value is a tuple: (Vec<ProxyDefinition>, Balance)
    match &decoded.value {
        ValueDef::Composite(Composite::Unnamed(values)) => {
            // First element is the Vec of proxy definitions
            let proxy_definitions = if let Some(proxies_value) = values.first() {
                decode_proxy_definitions(proxies_value)?
            } else {
                Vec::new()
            };

            // Second element is the deposit
            let deposit = if let Some(deposit_value) = values.get(1) {
                match &deposit_value.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(val)) => val.to_string(),
                    _ => "0".to_string(),
                }
            } else {
                "0".to_string()
            };

            Ok((proxy_definitions, deposit))
        }
        _ => Ok((Vec::new(), "0".to_string())),
    }
}

/// Decode proxy definitions from a Value
fn decode_proxy_definitions(
    value: &Value<()>,
) -> Result<Vec<DecodedProxyDefinition>, ProxyQueryError> {
    let mut definitions = Vec::new();

    // The value should be a sequence/array of proxy definitions
    if let ValueDef::Composite(Composite::Unnamed(items)) = &value.value {
        for item in items {
            if let Some(def) = decode_single_proxy_definition(item)? {
                definitions.push(def);
            }
        }
    }

    Ok(definitions)
}

/// Decode a single proxy definition
fn decode_single_proxy_definition(
    value: &Value<()>,
) -> Result<Option<DecodedProxyDefinition>, ProxyQueryError> {
    match &value.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            let delegate = extract_account_id_field(fields, "delegate")
                .or_else(|| extract_account_id_field(fields, "Delegate"))
                .unwrap_or_else(|| "unknown".to_string());

            let proxy_type = extract_proxy_type_field(fields, "proxyType")
                .or_else(|| extract_proxy_type_field(fields, "proxy_type"))
                .unwrap_or_else(|| "Unknown".to_string());

            let delay = extract_u32_field(fields, "delay")
                .map(|d| d.to_string())
                .unwrap_or_else(|| "0".to_string());

            Ok(Some(DecodedProxyDefinition {
                delegate,
                proxy_type,
                delay,
            }))
        }
        ValueDef::Composite(Composite::Unnamed(values)) => {
            // Tuple-style: (delegate, proxy_type, delay)
            let delegate = values
                .first()
                .and_then(extract_account_id_from_value)
                .unwrap_or_else(|| "unknown".to_string());

            let proxy_type = values
                .get(1)
                .and_then(extract_proxy_type_from_value)
                .unwrap_or_else(|| "Unknown".to_string());

            let delay = values
                .get(2)
                .and_then(|v| match &v.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(val.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| "0".to_string());

            Ok(Some(DecodedProxyDefinition {
                delegate,
                proxy_type,
                delay,
            }))
        }
        _ => Ok(None),
    }
}

// ================================================================================================
// Field Extraction Helpers
// ================================================================================================

/// Extract an AccountId field from named fields and convert to SS58
fn extract_account_id_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<String> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| extract_account_id_from_value(value))
}

/// Extract an AccountId from a Value and convert to SS58
fn extract_account_id_from_value(value: &Value<()>) -> Option<String> {
    match &value.value {
        ValueDef::Composite(Composite::Unnamed(bytes)) => {
            // This might be a raw byte array
            let byte_vec: Vec<u8> = bytes
                .iter()
                .filter_map(|v| match &v.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(b)) => Some(*b as u8),
                    _ => None,
                })
                .collect();

            if byte_vec.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&byte_vec);
                let account_id = AccountId32::from(arr);
                // Use generic substrate prefix (42)
                Some(account_id.to_ss58check())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract proxy type from named fields
fn extract_proxy_type_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<String> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| extract_proxy_type_from_value(value))
}

/// Extract proxy type from a Value (usually a variant/enum)
fn extract_proxy_type_from_value(value: &Value<()>) -> Option<String> {
    match &value.value {
        ValueDef::Variant(variant) => Some(variant.name.clone()),
        _ => None,
    }
}

/// Extract u32 field from named fields (stored as u128 in scale_value)
fn extract_u32_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<u32> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| match &value.value {
            ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val as u32),
            _ => None,
        })
}
