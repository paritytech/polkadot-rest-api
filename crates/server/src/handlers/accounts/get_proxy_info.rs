use super::types::{BlockInfo, ProxyDefinition, ProxyInfoError, ProxyInfoQueryParams, ProxyInfoResponse};
use super::utils::validate_and_parse_address;
use crate::handlers::accounts::utils::fetch_timestamp;
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use config::ChainType;
use scale_value::{Composite, Value, ValueDef};
use serde_json::json;
use sp_core::crypto::{AccountId32, Ss58Codec};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/proxy-info
///
/// Returns proxy information for a given account.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
pub async fn get_proxy_info(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<ProxyInfoQueryParams>,
) -> Result<Response, ProxyInfoError> {
    let account = validate_and_parse_address(&account_id)
        .map_err(|_| ProxyInfoError::InvalidAddress(account_id.clone()))?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    let block_id = params.at.map(|s| s.parse::<utils::BlockId>()).transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    println!(
        "Fetching proxy info for account {:?} at block {}",
        account, resolved_block.number
    );

    let response = query_proxy_info(&state, &account, &resolved_block).await?;

    Ok(Json(response).into_response())
}

async fn query_proxy_info(
    state: &AppState,
    account: &AccountId32,
    block: &utils::ResolvedBlock,
) -> Result<ProxyInfoResponse, ProxyInfoError> {
    let client_at_block = state.client.at(block.number).await?;

    let proxy_exists = client_at_block
        .storage()
        .entry("Proxy", "Proxies")
        .is_ok();

    if !proxy_exists {
        return Err(ProxyInfoError::ProxyPalletNotAvailable);
    }

    let storage_entry = client_at_block.storage().entry("Proxy", "Proxies")?;

    // Storage key for Proxies: (account)
    let account_bytes: [u8; 32] = *account.as_ref();
    let storage_value = storage_entry.fetch(&(&account_bytes,)).await?;

    let (delegated_accounts, deposit_held) = if let Some(value) = storage_value {
        decode_proxy_info(&value).await?
    } else {
        (Vec::new(), "0".to_string())
    };

    Ok(ProxyInfoResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        delegated_accounts,
        deposit_held,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ================================================================================================
// Proxy Info Decoding
// ================================================================================================

/// Decode proxy info from storage value
/// The storage value is a tuple: (Vec<ProxyDefinition>, Balance)
async fn decode_proxy_info(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<(Vec<ProxyDefinition>, String), ProxyInfoError> {
    // Decode as scale_value::Value to inspect structure
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        ProxyInfoError::DecodeFailed(parity_scale_codec::Error::from(
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
fn decode_proxy_definitions(value: &Value<()>) -> Result<Vec<ProxyDefinition>, ProxyInfoError> {
    let mut definitions = Vec::new();

    // The value should be a sequence/array of proxy definitions
    match &value.value {
        ValueDef::Composite(Composite::Unnamed(items)) => {
            for item in items {
                if let Some(def) = decode_single_proxy_definition(item)? {
                    definitions.push(def);
                }
            }
        }
        _ => {}
    }

    Ok(definitions)
}

/// Decode a single proxy definition
fn decode_single_proxy_definition(
    value: &Value<()>,
) -> Result<Option<ProxyDefinition>, ProxyInfoError> {
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

            Ok(Some(ProxyDefinition {
                delegate,
                proxy_type,
                delay,
            }))
        }
        ValueDef::Composite(Composite::Unnamed(values)) => {
            // Tuple-style: (delegate, proxy_type, delay)
            let delegate = values
                .first()
                .and_then(|v| extract_account_id_from_value(v))
                .unwrap_or_else(|| "unknown".to_string());

            let proxy_type = values
                .get(1)
                .and_then(|v| extract_proxy_type_from_value(v))
                .unwrap_or_else(|| "Unknown".to_string());

            let delay = values
                .get(2)
                .and_then(|v| match &v.value {
                    ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(val.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| "0".to_string());

            Ok(Some(ProxyDefinition {
                delegate,
                proxy_type,
                delay,
            }))
        }
        _ => Ok(None),
    }
}

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

// ================================================================================================
// Relay Chain Block Handling
// ================================================================================================

async fn handle_use_rc_block(
    state: AppState,
    account: AccountId32,
    params: ProxyInfoQueryParams,
) -> Result<Response, ProxyInfoError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(ProxyInfoError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(ProxyInfoError::RelayChainNotConfigured);
    }

    // Resolve RC block
    let rc_block_id = params
        .at
        .unwrap_or_else(|| "head".to_string())
        .parse::<utils::BlockId>()?;
    let rc_resolved = utils::resolve_block_with_rpc(
        state.get_relay_chain_rpc_client().unwrap(),
        state.get_relay_chain_rpc().unwrap(),
        Some(rc_block_id),
    )
    .await?;

    // Find AH blocks
    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved).await?;

    if ah_blocks.is_empty() {
        return Ok(Json(json!([])).into_response());
    }

    let rc_block_hash = rc_resolved.hash.clone();
    let rc_block_number = rc_resolved.number.to_string();

    // Process each AH block
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };

        let mut response = query_proxy_info(&state, &account, &ah_resolved).await?;

        // Add RC block info
        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        // Fetch AH timestamp
        if let Ok(timestamp) = fetch_timestamp(&state, ah_block.number).await {
            response.ah_timestamp = Some(timestamp);
        }

        results.push(response);
    }

    Ok(Json(results).into_response())
}
