//! Handler for GET /accounts/{accountId}/asset-balances endpoint.

use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use config::ChainType;
use futures::StreamExt;
use parity_scale_codec::Decode;
use scale_value::{Composite, Value, ValueDef};
use serde_json::json;
use sp_core::crypto::AccountId32;
use sp_runtime::print;
use tracing::info;
use std::str::FromStr;
use subxt_historic::{SubstrateConfig, client::{ClientAtBlock, OnlineClientAtBlock}, storage::StorageValue};

use super::types::{
    AssetBalance, AssetBalancesError, AssetBalancesQueryParams, AssetBalancesResponse, BlockInfo,
};

// ================================================================================================
// Type Aliases
// ================================================================================================

/// Type alias for the ClientAtBlock type used in this module
type BlockClient<'a> = ClientAtBlock<OnlineClientAtBlock<'a, SubstrateConfig>, SubstrateConfig>;

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/asset-balances
///
/// Returns asset balances for a given account.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `assets` (optional): List of asset IDs to query (queries all if omitted)
pub async fn get_asset_balances(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<AssetBalancesQueryParams>,
) -> Result<Response, AssetBalancesError> {
    // 1. Validate and parse address
    let account = validate_and_parse_address(&account_id)?;

    // 2. Handle useRcBlock case
    if params.use_rc_block {
        return handle_use_rc_block(state, account, params).await;
    }

    // 3. Normal case: resolve block
    let block_id = params
        .at
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    // 4. Query asset balances
    let response = query_asset_balances(&state, &account, &resolved_block, &params.assets).await?;

    Ok(Json(response).into_response())
}

// ================================================================================================
// Relay Chain Block Handling
// ================================================================================================

async fn handle_use_rc_block(
    state: AppState,
    account: AccountId32,
    params: AssetBalancesQueryParams,
) -> Result<Response, AssetBalancesError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(AssetBalancesError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(AssetBalancesError::RelayChainNotConfigured);
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

        let mut response =
            query_asset_balances(&state, &account, &ah_resolved, &params.assets).await?;

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

// ================================================================================================
// Asset Balance Querying
// ================================================================================================

async fn query_asset_balances(
    state: &AppState,
    account: &AccountId32,
    block: &utils::ResolvedBlock,
    asset_ids: &[u32],
) -> Result<AssetBalancesResponse, AssetBalancesError> {
    let client_at_block = state.client.at(block.number).await?;
    let pallets = client_at_block.storage().entries().into_iter().map(|e| e.pallet_name().to_string()).collect::<Vec<_>>();
    info!("Querying asset balances at block: {:?} ", pallets);
// TODO: check this logic
    // Check Assets pallet exists
    let assets_exists = client_at_block
        .storage()
        .entry("Assets", "Account")
        .is_ok();

    if !assets_exists {
        return Err(AssetBalancesError::AssetsPalletNotAvailable);
    }

    // Determine which assets to query
    let assets_to_query = if asset_ids.is_empty() {
        // Query all asset IDs
        fetch_all_asset_ids(&client_at_block).await?
    } else {
        asset_ids.to_vec()
    };

    // Query each asset balance in parallel
    let mut query_futures = Vec::new();
    for asset_id in &assets_to_query {
        query_futures.push(query_single_asset(&client_at_block, *asset_id, account));
    }

    let results = futures::future::join_all(query_futures).await;

    // Collect successful results
    let mut assets = Vec::new();
    for result in results {
        if let Ok(Some(balance)) = result {
            assets.push(balance);
        }
    }

    Ok(AssetBalancesResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        assets,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

/// Fetch all asset IDs from storage
async fn fetch_all_asset_ids(
    client: &BlockClient<'_>,
) -> Result<Vec<u32>, AssetBalancesError> {
    let storage_entry = client.storage().entry("Assets", "Asset")?;
    let mut asset_ids = Vec::new();

    let mut iter = storage_entry.iter(()).await?;
    while let Some(result) = iter.next().await {
        let entry = result?;

        // Extract asset ID from storage key
        // The key is Assets::Asset(AssetId), we need to decode the AssetId
        let key_bytes = entry.key_bytes();

        // Skip the pallet hash and entry hash (32 bytes total)
        // Then decode the u32 asset ID
        if key_bytes.len() >= 36 {
            // 32 bytes for hashes + at least 4 for u32
            let asset_id_bytes = &key_bytes[32..];
            if let Ok(asset_id) = u32::decode(&mut &asset_id_bytes[..]) {
                asset_ids.push(asset_id);
            }
        }
    }

    Ok(asset_ids)
}

/// Query a single asset balance for an account
async fn query_single_asset(
    client: &BlockClient<'_>,
    asset_id: u32,
    account: &AccountId32,
) -> Result<Option<AssetBalance>, AssetBalancesError> {
    let storage_entry = client.storage().entry("Assets", "Account")?;

    // Encode the storage key: (asset_id, account_id)
    // Convert AccountId32 to [u8; 32] for encoding
    let account_bytes: &[u8; 32] = account.as_ref();
    let key = (asset_id, *account_bytes);
    let storage_value = storage_entry.fetch(&key).await?;

    // Handle None case (account has no balance for this asset)
    let Some(value) = storage_value else {
        return Ok(None);
    };

    // Decode the storage value
    decode_asset_balance(&value, asset_id).await
}

/// Decode asset balance from storage value, handling multiple runtime versions
async fn decode_asset_balance(
    value: &StorageValue<'_>,
    asset_id: u32,
) -> Result<Option<AssetBalance>, AssetBalancesError> {
    // Decode as scale_value::Value to inspect structure
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        AssetBalancesError::DecodeFailed(parity_scale_codec::Error::from("Failed to decode storage value"))
    })?;

    // Handle Option wrapper (post-v9160)
    let balance_value = match &decoded.value {
        ValueDef::Variant(variant) => {
            // This is an Option enum
            if variant.name == "Some" {
                // Extract the inner value from the composite
                match &variant.values {
                    Composite::Unnamed(values) => {
                        if let Some(inner) = values.first() {
                            inner
                        } else {
                            // Empty Some variant, return None
                            return Ok(None);
                        }
                    }
                    Composite::Named(fields) => {
                        if let Some((_, inner)) = fields.first() {
                            inner
                        } else {
                            return Ok(None);
                        }
                    }
                }
            } else {
                // None variant
                return Ok(None);
            }
        }
        _ => &decoded,
    };

    // Now decode the actual balance structure
    match &balance_value.value {
        ValueDef::Composite(composite) => decode_balance_composite(composite, asset_id),
        _ => {
            // Fallback: return zero balance
            Ok(Some(AssetBalance {
                asset_id,
                balance: "0".to_string(),
                is_frozen: false,
                is_sufficient: false,
            }))
        }
    }
}

/// Decode balance from a composite structure
fn decode_balance_composite(
    composite: &Composite<()>,
    asset_id: u32,
) -> Result<Option<AssetBalance>, AssetBalancesError> {
    match composite {
        Composite::Named(fields) => {
            // Extract fields by name
            let balance = extract_u128_field(fields, "balance").unwrap_or(0);
            let is_frozen = extract_bool_field(fields, "isFrozen")
                .or_else(|| extract_bool_field(fields, "is_frozen"))
                .unwrap_or(false);

            // Handle different runtime versions for isSufficient
            let is_sufficient = if let Some(reason_value) = fields.iter().find(|(name, _)| name == "reason") {
                // Post-v9160: reason enum
                extract_is_sufficient_from_reason(&reason_value.1)
            } else if let Some(sufficient) = extract_bool_field(fields, "sufficient") {
                // v9160: sufficient boolean
                sufficient
            } else if let Some(is_sufficient) = extract_bool_field(fields, "isSufficient")
                .or_else(|| extract_bool_field(fields, "is_sufficient"))
            {
                // Pre-v9160: isSufficient boolean
                is_sufficient
            } else {
                false
            };

            Ok(Some(AssetBalance {
                asset_id,
                balance: balance.to_string(),
                is_frozen,
                is_sufficient,
            }))
        }
        Composite::Unnamed(_) => {
            // Fallback: return zero balance
            Ok(Some(AssetBalance {
                asset_id,
                balance: "0".to_string(),
                is_frozen: false,
                is_sufficient: false,
            }))
        }
    }
}

/// Extract u128 field from named fields
fn extract_u128_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<u128> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| match &value.value {
            ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val),
            _ => None,
        })
}

/// Extract boolean field from named fields
fn extract_bool_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<bool> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| match &value.value {
            ValueDef::Primitive(scale_value::Primitive::Bool(val)) => Some(*val),
            _ => None,
        })
}

/// Extract isSufficient from reason enum
fn extract_is_sufficient_from_reason(reason_value: &Value<()>) -> bool {
    match &reason_value.value {
        ValueDef::Variant(variant) => {
            // Check if variant name is "Sufficient" or "isSufficient"
            variant.name == "Sufficient" || variant.name == "isSufficient"
        }
        _ => false,
    }
}

// ================================================================================================
// Helper Functions
// ================================================================================================

/// Validate and parse account address (supports SS58 and hex formats)
fn validate_and_parse_address(addr: &str) -> Result<AccountId32, AssetBalancesError> {
    // Try SS58 format first (any network prefix)
    if let Ok(account) = AccountId32::from_str(addr) {
        return Ok(account);
    }

    // Try hex format (0x-prefixed, 32 bytes)
    if addr.starts_with("0x") && addr.len() == 66 {
        // 0x + 64 hex chars = 32 bytes
        if let Ok(bytes) = hex::decode(&addr[2..]) {
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                return Ok(AccountId32::from(arr));
            }
        }
    }

    Err(AssetBalancesError::InvalidAddress(addr.to_string()))
}

/// Fetch timestamp for a given block
async fn fetch_timestamp(state: &AppState, block_number: u64) -> Result<String, AssetBalancesError> {
    let client_at_block = state.client.at(block_number).await?;

    if let Ok(timestamp_entry) = client_at_block.storage().entry("Timestamp", "Now") {
        if let Ok(Some(timestamp)) = timestamp_entry.fetch(()).await {
            // Timestamp is a u64 (milliseconds) - decode from storage value
            let timestamp_bytes = timestamp.into_bytes();
            let mut cursor = &timestamp_bytes[..];
            if let Ok(timestamp_value) = u64::decode(&mut cursor) {
                return Ok(timestamp_value.to_string());
            }
        }
    }

    Err(AssetBalancesError::AssetsPalletNotAvailable)
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_validation_hex() {
        // Alice's address in hex
        let addr = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
        assert!(validate_and_parse_address(addr).is_ok());
    }

    #[test]
    fn test_address_validation_ss58() {
        // Alice's address in SS58 (Polkadot prefix)
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
        assert!(validate_and_parse_address(addr).is_ok());
    }

    #[test]
    fn test_address_validation_invalid() {
        let addr = "invalid-address";
        assert!(validate_and_parse_address(addr).is_err());
    }

    #[test]
    fn test_address_validation_short_hex() {
        let addr = "0x1234"; // Too short
        assert!(validate_and_parse_address(addr).is_err());
    }
}
