// ================================================================================================
// Pool Assets Data Fetching
// ================================================================================================

use crate::handlers::accounts::{AccountsError, PoolAssetBalance};
use parity_scale_codec::Decode;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ================================================================================================
// SCALE Decode Types for PoolAssets::Account storage
// ================================================================================================

/// Account status for a pool asset account (modern runtimes)
#[derive(Debug, Clone, Decode)]
enum PoolAccountStatus {
    Liquid,
    Frozen,
    Blocked,
}

impl PoolAccountStatus {
    fn is_frozen(&self) -> bool {
        matches!(self, PoolAccountStatus::Frozen | PoolAccountStatus::Blocked)
    }
}

/// Existence reason for a pool asset account (modern runtimes)
#[derive(Debug, Clone, Decode)]
#[allow(dead_code)] // Fields needed for SCALE decoding
enum PoolExistenceReason {
    Consumer,
    Sufficient,
    DepositHeld(u128),
    DepositRefunded,
    DepositFrom([u8; 32], u128),
}

impl PoolExistenceReason {
    fn is_sufficient(&self) -> bool {
        matches!(self, PoolExistenceReason::Sufficient)
    }
}

/// Modern PoolAssetAccount structure (current runtimes with status/reason fields)
#[derive(Debug, Clone, Decode)]
struct PoolAssetAccountModern {
    balance: u128,
    status: PoolAccountStatus,
    reason: PoolExistenceReason,
    // extra field is typically () - ignored
}

/// Legacy PoolAssetBalance structure (older runtimes with is_frozen/sufficient booleans)
#[derive(Debug, Clone, Decode)]
struct PoolAssetAccountLegacy {
    balance: u128,
    is_frozen: bool,
    sufficient: bool,
    // extra field is typically () - ignored
}

/// Fetch all pool asset IDs from storage
pub async fn query_all_pool_assets_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    // Use Vec<u8> as return type since we only need raw bytes to extract asset IDs from keys
    let storage_query = subxt::storage::dynamic::<Vec<u32>, Vec<u8>>("PoolAssets", "Asset");
    let storage_entry = client_at_block.storage().entry(storage_query)?;
    let mut asset_ids = Vec::new();

    let mut values = storage_entry.iter(Vec::<u32>::new()).await?;
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
    // Use dynamic storage to fetch raw bytes for each (asset_id, account_id) key
    let account_bytes: [u8; 32] = *account.as_ref();

    let mut balances = Vec::new();

    for asset_id in assets {
        // Build the storage address for PoolAssets::Account(asset_id, account_id)
        let storage_addr = subxt::dynamic::storage::<_, ()>("PoolAssets", "Account");

        let storage_value = client_at_block
            .storage()
            .fetch(storage_addr, (*asset_id, account_bytes))
            .await;

        if let Ok(value) = storage_value {
            // Get raw bytes from the storage value
            let raw_bytes = value.into_bytes();
            // Decode the storage value from raw bytes
            if let Some(decoded_balance) = decode_pool_asset_balance(&raw_bytes)? {
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

/// Decode pool asset balance from raw SCALE bytes, handling multiple runtime versions
fn decode_pool_asset_balance(
    raw_bytes: &[u8],
) -> Result<Option<DecodedPoolAssetBalance>, AccountsError> {
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
    Err(AccountsError::DecodeFailed(
        parity_scale_codec::Error::from("Failed to decode pool asset account: unknown format"),
    ))
}
