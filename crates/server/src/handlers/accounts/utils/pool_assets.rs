// ================================================================================================
// Pool Assets Data Fetching
// ================================================================================================

use crate::handlers::accounts::{
    AccountsError, PoolAssetBalance,
    utils::{extract_bool_field, extract_is_sufficient_from_reason, extract_u128_field},
};
use parity_scale_codec::Decode;
use scale_value::{Composite, Value, ValueDef};
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig, storage::StorageValue};

/// Fetch all pool asset IDs from storage
pub async fn query_all_pool_assets_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let storage_query = ("PoolAssets", "Asset");
    let storage_entry = client_at_block.storage().entry(storage_query)?;
    let mut asset_ids = Vec::new();

    let mut values = storage_entry.iter(Vec::<scale_value::Value>::new()).await?;
    while let Some(result) = values.next().await {
        let entry = result?;
        // Extract asset ID from storage key
        // Storage key structure for PoolAssets::Asset(AssetId):
        // - Bytes 0-15: Twox128("PoolAssets")
        // - Bytes 16-31: Twox128("Asset")
        // - Bytes 32-47: Blake2_128Concat hash of asset_id
        // - Bytes 48+: Raw SCALE-encoded u32 asset_id
        let key = entry.key_bytes();

        // Skip pallet hash (16) + storage hash (16) + Blake2_128 hash (16) = 48 bytes
        // Then decode the raw u32 asset ID
        if key.len() >= 52 {
            // 48 bytes prefix + 4 bytes u32
            if let Ok(asset_id) = u32::decode(&mut &key[48..]) {
                asset_ids.push(asset_id);
            }
        }
    }
    Ok(asset_ids)
}

/// Query pool asset balances for an account
pub async fn query_pool_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    assets: &[u32],
) -> Result<Vec<PoolAssetBalance>, AccountsError> {
    let storage_query = ("PoolAssets", "Account");
    let storage_entry = client_at_block.storage().entry(storage_query)?;

    // Encode the storage key: (asset_id, account_id)
    // Convert AccountId32 to [u8; 32] for encoding
    let account_bytes: &[u8; 32] = account.as_ref();

    let mut balances = Vec::new();

    for asset_id in assets {
        let key = vec![
            Value::u128(*asset_id as u128),
            Value::from_bytes(account_bytes),
        ];
        let storage_value = storage_entry.try_fetch(key).await?;

        if let Some(value) = storage_value {
            // Decode the storage value
            let decoded = decode_pool_asset_balance(&value).await?;
            if let Some(decoded_balance) = decoded {
                balances.push(PoolAssetBalance {
                    asset_id: *asset_id,
                    balance: decoded_balance.balance,
                    is_frozen: decoded_balance.is_frozen,
                    is_sufficient: decoded_balance.is_sufficient,
                });
            }
        }
    }

    Ok(balances)
}

// ================================================================================================
// Pool Asset Balance Decoding
// ================================================================================================

/// Decoded pool asset balance data
#[derive(Debug, Clone)]
pub struct DecodedPoolAssetBalance {
    pub balance: String,
    pub is_frozen: bool,
    pub is_sufficient: bool,
}

/// Decode pool asset balance from storage value, handling multiple runtime versions
pub async fn decode_pool_asset_balance(
    value: &StorageValue<'_, scale_value::Value>,
) -> Result<Option<DecodedPoolAssetBalance>, AccountsError> {
    // Decode as scale_value::Value to inspect structure
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        AccountsError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode storage value",
        ))
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
        ValueDef::Composite(composite) => decode_pool_balance_composite(composite),
        _ => {
            // Fallback: return zero balance
            Ok(Some(DecodedPoolAssetBalance {
                balance: "0".to_string(),
                is_frozen: false,
                is_sufficient: false,
            }))
        }
    }
}

/// Decode pool balance from a composite structure
fn decode_pool_balance_composite(
    composite: &Composite<()>,
) -> Result<Option<DecodedPoolAssetBalance>, AccountsError> {
    match composite {
        Composite::Named(fields) => {
            // Extract fields by name
            let balance = extract_u128_field(fields, "balance").unwrap_or(0);
            let is_frozen = extract_bool_field(fields, "isFrozen")
                .or_else(|| extract_bool_field(fields, "is_frozen"))
                .unwrap_or(false);

            // Handle different runtime versions for isSufficient
            let is_sufficient =
                if let Some(reason_value) = fields.iter().find(|(name, _)| name == "reason") {
                    // Post-v9160: reason enum
                    extract_is_sufficient_from_reason(&reason_value.1)
                } else if let Some(sufficient) = extract_bool_field(fields, "sufficient") {
                    // v9160: sufficient boolean
                    sufficient
                } else {
                    // Pre-v9160: isSufficient boolean
                    extract_bool_field(fields, "isSufficient")
                        .or_else(|| extract_bool_field(fields, "is_sufficient"))
                        .unwrap_or_default()
                };

            Ok(Some(DecodedPoolAssetBalance {
                balance: balance.to_string(),
                is_frozen,
                is_sufficient,
            }))
        }
        Composite::Unnamed(_) => {
            // Fallback: return zero balance
            Ok(Some(DecodedPoolAssetBalance {
                balance: "0".to_string(),
                is_frozen: false,
                is_sufficient: false,
            }))
        }
    }
}
