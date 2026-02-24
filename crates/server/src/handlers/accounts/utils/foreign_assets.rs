// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::handlers::accounts::{AccountsError, ForeignAssetBalance};
use crate::handlers::common::xcm_types::Location;
use crate::handlers::runtime_queries::foreign_assets as foreign_assets_queries;
use parity_scale_codec::Encode;
use sp_core::crypto::AccountId32;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

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
    foreign_assets_queries::iter_all_foreign_asset_locations(client_at_block)
        .await
        .map_err(|_| AccountsError::PalletNotAvailable("ForeignAssets".to_string()))
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
        match foreign_assets_queries::get_foreign_asset_account(
            client_at_block,
            location,
            &account_bytes,
        )
        .await
        {
            Ok(Some(decoded)) => {
                // Skip zero-balance entries
                if decoded.balance == 0 {
                    continue;
                }

                let multi_location_json =
                    serde_json::to_value(location).unwrap_or(serde_json::json!({}));

                balances.push(ForeignAssetBalance {
                    multi_location: multi_location_json,
                    balance: decoded.balance.to_string(),
                    is_frozen: decoded.is_frozen,
                    is_sufficient: decoded.is_sufficient,
                });
            }
            Ok(None) => {
                // No entry for this (location, account) pair -- skip
                continue;
            }
            Err(e) => {
                tracing::debug!(
                    "Failed to fetch foreign asset account for location, attempting fallback: {:?}",
                    e
                );
                // Try fallback decode using the centralized raw fetch
                if let Ok(Some(fb)) =
                    try_fallback_foreign_asset_account(client_at_block, location, &account_bytes)
                        .await
                    && fb.balance != "0"
                {
                    balances.push(fb);
                }
            }
        }
    }

    Ok(balances)
}

/// Fallback for older runtimes - tries to decode using scale_value
async fn try_fallback_foreign_asset_account(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    location: &Location,
    account_bytes: &[u8; 32],
) -> Result<Option<ForeignAssetBalance>, AccountsError> {
    use crate::handlers::accounts::utils::{
        extract_bool_field, extract_is_sufficient_from_reason, extract_u128_field,
    };
    use scale_value::{Composite, Value, ValueDef};

    // Use centralized raw fetch
    let decoded: Value<()> = foreign_assets_queries::get_foreign_asset_account_raw(
        client_at_block,
        location,
        account_bytes,
    )
    .await
    .map_err(|_| AccountsError::PalletNotAvailable("ForeignAssets".to_string()))?
    .ok_or(AccountsError::PalletNotAvailable(
        "ForeignAssets".to_string(),
    ))?;

    // Unwrap Option<AssetAccount> if present
    let inner = match &decoded.value {
        ValueDef::Variant(variant) if variant.name == "Some" => match &variant.values {
            Composite::Unnamed(vals) => match vals.first() {
                Some(v) => v,
                None => return Ok(None),
            },
            Composite::Named(fields) => match fields.first() {
                Some((_, v)) => v,
                None => return Ok(None),
            },
        },
        ValueDef::Variant(_) => return Ok(None), // None variant
        _ => &decoded,
    };

    match &inner.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            let balance = extract_u128_field(&fields[..], "balance").unwrap_or(0);
            let is_frozen = extract_bool_field(&fields[..], "isFrozen")
                .or_else(|| extract_bool_field(&fields[..], "is_frozen"))
                .unwrap_or(false);

            let is_sufficient =
                if let Some(reason_value) = fields.iter().find(|(name, _)| name == "reason") {
                    extract_is_sufficient_from_reason(&reason_value.1)
                } else if let Some(sufficient) = extract_bool_field(&fields[..], "sufficient") {
                    sufficient
                } else {
                    extract_bool_field(&fields[..], "isSufficient")
                        .or_else(|| extract_bool_field(&fields[..], "is_sufficient"))
                        .unwrap_or_default()
                };

            let multi_location_json =
                serde_json::to_value(location).unwrap_or(serde_json::json!({}));

            Ok(Some(ForeignAssetBalance {
                multi_location: multi_location_json,
                balance: balance.to_string(),
                is_frozen,
                is_sufficient,
            }))
        }
        _ => Ok(None),
    }
}

/// Parse foreign asset location JSON strings into Location objects.
///
/// Uses `staging_xcm::v4::Location` for JSON deserialization (which has full
/// serde support), then SCALE-encodes and decodes into our typed Location struct.
///
/// Accepts both numeric (`"parents": 2`) and string-encoded (`"parents": "2"`)
/// number values to be compatible with Sidecar's query format where numbers
/// are represented as strings.
pub fn parse_foreign_asset_locations(
    json_strings: &[String],
) -> Result<Vec<Location>, AccountsError> {
    use parity_scale_codec::Decode;

    let mut locations = Vec::new();
    for json_str in json_strings {
        // Parse JSON string
        let mut json_value: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| AccountsError::InvalidForeignAsset(format!("Invalid JSON: {}", e)))?;

        // Coerce string-encoded numbers to actual numbers so staging_xcm can
        // deserialize them (it expects u8/u32/u64/u128, not strings).
        coerce_string_numbers(&mut json_value);

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

/// Recursively convert string-encoded numbers to JSON numbers.
///
/// Sidecar formats all numbers as strings (e.g., `"parents": "2"`,
/// `"Parachain": "1000"`), but `staging_xcm`'s serde `Deserialize` expects
/// actual JSON numbers. This coerces any string that parses as a `u128`
/// into a `Number`. Hex strings ("0x...") and other non-numeric strings
/// are left unchanged.
fn coerce_string_numbers(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => {
            if let Ok(n) = s.parse::<u128>() {
                *value = serde_json::json!(n);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                coerce_string_numbers(item);
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values_mut() {
                coerce_string_numbers(val);
            }
        }
        _ => {}
    }
}
