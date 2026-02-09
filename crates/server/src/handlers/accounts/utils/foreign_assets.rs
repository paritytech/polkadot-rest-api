// ================================================================================================
// Foreign Assets Data Fetching
// ================================================================================================
//
// Uses typed DecodeAsType decoding (subxt 0.50.0 pattern) for efficient
// storage queries against the ForeignAssets pallet. Falls back to scale_value
// decoding for older runtimes when the typed decode fails.

use crate::handlers::accounts::{AccountsError, ForeignAssetBalance};
use crate::handlers::common::xcm_types::{Location, BLAKE2_128_HASH_LEN};
use futures::StreamExt;
use parity_scale_codec::{Decode, Encode};
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

// ============================================================================
// SCALE Decode Types for ForeignAssets::Account storage
// ============================================================================

/// ExistenceReason enum - handles the runtime version differences for the
/// isSufficient/reason field in asset account storage.
///
/// Modern runtimes (post-v9160) use this enum. The `Sufficient` variant
/// indicates the asset is self-sufficient (does not require an ED deposit).
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
#[allow(dead_code)] // Fields needed for SCALE decoding layout
enum ExistenceReason {
    Consumer,
    Sufficient,
    DepositHeld(u128),
    DepositRefunded,
    DepositFrom([u8; 32], u128),
}

/// AssetAccount struct from ForeignAssets::Account storage.
/// This matches the modern runtime layout (post-v9160 with `reason` field).
///
/// Storage type: DoubleMap(Location, AccountId32) -> Option<AssetAccount>
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
#[allow(dead_code)] // `extra` needed for SCALE decoding layout
struct AssetAccount {
    balance: u128,
    status: AssetAccountStatus,
    reason: ExistenceReason,
    extra: (),
}

/// Status of an asset account (modern runtimes).
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
enum AssetAccountStatus {
    Liquid,
    Frozen,
    Blocked,
}

// ============================================================================
// Public Query Functions
// ============================================================================

/// Query all foreign asset multilocation keys from ForeignAssets::Asset storage.
///
/// Iterates the Asset storage map to discover all registered multilocations.
/// Returns the Location objects needed for subsequent Account lookups.
pub async fn query_all_foreign_asset_locations(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<Location>, AccountsError> {
    // Use typed dynamic storage iteration with Location as key type
    let storage_addr = subxt::dynamic::storage::<(Location,), ()>("ForeignAssets", "Asset");

    let mut locations = Vec::new();
    let mut stream = client_at_block
        .storage()
        .iter(storage_addr, ())
        .await
        .map_err(|_| AccountsError::PalletNotAvailable("ForeignAssets".to_string()))?;

    while let Some(result) = stream.next().await {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("Error reading foreign asset entry: {:?}", e);
                continue;
            }
        };

        // Extract Location from storage key using subxt's key().part(0)
        // Then decode the raw bytes (skipping Blake2_128Concat hash prefix)
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

    Ok(locations)
}

/// Query foreign asset balances for a specific account.
///
/// For each provided Location, fetches the ForeignAssets::Account storage
/// entry for the (Location, AccountId) double-map key.
///
/// Uses typed DecodeAsType decoding. If typed decode fails (e.g., on older
/// runtimes with different struct layout), falls back to scale_value decoding.
/// Filters out zero-balance entries.
pub async fn query_foreign_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    locations: &[Location],
) -> Result<Vec<ForeignAssetBalance>, AccountsError> {
    let account_bytes: [u8; 32] = *account.as_ref();
    let mut balances = Vec::new();

    for location in locations {
        // Create the storage address for ForeignAssets::Account (DoubleMap)
        let storage_addr = subxt::dynamic::storage::<(Location, [u8; 32]), AssetAccount>(
            "ForeignAssets",
            "Account",
        );

        // Fetch the account balance for this (location, account) pair
        let result = client_at_block
            .storage()
            .fetch(storage_addr, (location.clone(), account_bytes))
            .await;

        match result {
            Ok(storage_val) => {
                // Try typed decode first (modern runtime)
                match storage_val.decode() {
                    Ok(asset_account) => {
                        // Skip zero-balance entries
                        if asset_account.balance == 0 {
                            continue;
                        }

                        let is_frozen = matches!(
                            asset_account.status,
                            AssetAccountStatus::Frozen | AssetAccountStatus::Blocked
                        );
                        let is_sufficient = matches!(
                            asset_account.reason,
                            ExistenceReason::Sufficient
                        );

                        let multi_location_json = serde_json::to_value(location)
                            .unwrap_or(serde_json::json!({}));

                        balances.push(ForeignAssetBalance {
                            multi_location: multi_location_json,
                            balance: asset_account.balance.to_string(),
                            is_frozen,
                            is_sufficient,
                        });
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Failed typed decode for ForeignAssets::Account: {:?}, \
                             attempting scale_value fallback",
                            e
                        );
                        // Fallback: use scale_value decode for older runtimes
                        if let Some(fb) =
                            fallback_decode_foreign_asset_account(&storage_val, location)
                        {
                            if fb.balance != "0" {
                                balances.push(fb);
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // No entry for this (location, account) pair -- skip
                continue;
            }
        }
    }

    Ok(balances)
}

/// Parse foreign asset location JSON strings into Location objects.
///
/// Uses `staging_xcm::v4::Location` for JSON deserialization (which has full
/// serde support), then SCALE-encodes and decodes into our typed Location struct.
pub fn parse_foreign_asset_locations(
    json_strings: &[String],
) -> Result<Vec<Location>, AccountsError> {
    let mut locations = Vec::new();
    for json_str in json_strings {
        // Parse JSON string
        let json_value: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
            AccountsError::InvalidForeignAsset(format!("Invalid JSON: {}", e))
        })?;

        // Deserialize into staging_xcm Location (which has Deserialize)
        let xcm_location: staging_xcm::v4::Location =
            serde_json::from_value(json_value).map_err(|e| {
                AccountsError::InvalidForeignAsset(format!("Invalid XCM location: {}", e))
            })?;

        // SCALE roundtrip: encode staging_xcm Location, decode as our Location
        let encoded = xcm_location.encode();
        let our_location = Location::decode(&mut &encoded[..]).map_err(|e| {
            AccountsError::InvalidForeignAsset(format!("Failed to decode location: {}", e))
        })?;

        locations.push(our_location);
    }
    Ok(locations)
}

// ============================================================================
// Fallback Decoder
// ============================================================================

/// Fallback decoder using scale_value for older runtime versions.
///
/// Handles three runtime version layouts:
/// - Post-v9160: has `reason` field (ExistenceReason enum)
/// - v9160: has `sufficient` boolean field
/// - Pre-v9160: has `isSufficient` or `is_sufficient` boolean field
fn fallback_decode_foreign_asset_account(
    storage_val: &subxt::storage::StorageValue<'_, AssetAccount>,
    location: &Location,
) -> Option<ForeignAssetBalance> {
    use crate::handlers::accounts::utils::{
        extract_bool_field, extract_is_sufficient_from_reason, extract_u128_field,
    };
    use scale_value::{Composite, Value, ValueDef};

    let decoded: Value<()> = storage_val.decode_as().ok()?;

    // Unwrap Option<AssetAccount> if present
    let inner = match &decoded.value {
        ValueDef::Variant(variant) if variant.name == "Some" => match &variant.values {
            Composite::Unnamed(vals) => vals.first()?,
            Composite::Named(fields) => &fields.first()?.1,
        },
        ValueDef::Variant(_) => return None, // None variant
        _ => &decoded,
    };

    match &inner.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            let balance = extract_u128_field(fields, "balance").unwrap_or(0);
            let is_frozen = extract_bool_field(fields, "isFrozen")
                .or_else(|| extract_bool_field(fields, "is_frozen"))
                .unwrap_or(false);

            let is_sufficient =
                if let Some(reason_value) = fields.iter().find(|(name, _)| name == "reason") {
                    extract_is_sufficient_from_reason(&reason_value.1)
                } else if let Some(sufficient) = extract_bool_field(fields, "sufficient") {
                    sufficient
                } else {
                    extract_bool_field(fields, "isSufficient")
                        .or_else(|| extract_bool_field(fields, "is_sufficient"))
                        .unwrap_or_default()
                };

            let multi_location_json =
                serde_json::to_value(location).unwrap_or(serde_json::json!({}));

            Some(ForeignAssetBalance {
                multi_location: multi_location_json,
                balance: balance.to_string(),
                is_frozen,
                is_sufficient,
            })
        }
        _ => None,
    }
}
