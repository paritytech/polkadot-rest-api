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
    foreign_assets_queries::iter_foreign_asset_locations(client_at_block)
        .await
        .ok_or_else(|| AccountsError::PalletNotAvailable("ForeignAssets".to_string()))
}

/// Query foreign asset balances for a specific account.
///
/// For each provided Location, fetches the ForeignAssets::Account storage
/// entry for the (Location, AccountId) double-map key.
///
/// Uses typed DecodeAsType decoding. If typed decode fails (e.g., on older
/// runtimes with different struct layout), falls back to legacy struct decoding.
///
/// When `show_empty` is false (default), filters out zero-balance entries.
/// When `show_empty` is true, returns all requested assets including those with zero balance.
///
/// Note: Queries are executed in parallel for performance.
pub async fn query_foreign_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    account: &AccountId32,
    locations: &[Location],
    show_empty: bool,
) -> Result<Vec<ForeignAssetBalance>, AccountsError> {
    use futures::future::join_all;

    let account_bytes: [u8; 32] = *account.as_ref();

    // Create futures for all location queries in parallel
    let futures: Vec<_> = locations
        .iter()
        .map(|location| {
            let location = location.clone();
            async move {
                let result = foreign_assets_queries::get_foreign_asset_account(
                    client_at_block,
                    &location,
                    &account_bytes,
                )
                .await;
                (location, result)
            }
        })
        .collect();

    // Execute all queries in parallel
    let results = join_all(futures).await;

    // Process results
    let mut balances = Vec::new();
    for (location, result) in results {
        match result {
            Some(decoded) => {
                // Skip zero-balance entries unless show_empty is true
                if decoded.balance == 0 && !show_empty {
                    continue;
                }

                let multi_location_json =
                    serde_json::to_value(&location).unwrap_or(serde_json::json!({}));

                balances.push(ForeignAssetBalance {
                    multi_location: multi_location_json,
                    balance: decoded.balance.to_string(),
                    is_frozen: decoded.is_frozen,
                    is_sufficient: decoded.is_sufficient,
                });
            }
            None => {
                // Try fallback decode using the centralized raw fetch
                if let Ok(Some(fb)) =
                    try_fallback_foreign_asset_account(client_at_block, &location, &account_bytes)
                        .await
                {
                    if fb.balance != "0" || show_empty {
                        balances.push(fb);
                    }
                } else if show_empty {
                    // No balance found but show_empty is true - add zero balance entry
                    let multi_location_json =
                        serde_json::to_value(&location).unwrap_or(serde_json::json!({}));
                    balances.push(ForeignAssetBalance {
                        multi_location: multi_location_json,
                        balance: "0".to_string(),
                        is_frozen: false,
                        is_sufficient: false,
                    });
                }
            }
        }
    }

    Ok(balances)
}

/// Fallback for older runtimes - tries to decode using legacy struct layout
async fn try_fallback_foreign_asset_account(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    location: &Location,
    account_bytes: &[u8; 32],
) -> Result<Option<ForeignAssetBalance>, AccountsError> {
    // Use centralized raw fetch with legacy struct
    let decoded = foreign_assets_queries::get_foreign_asset_account_raw(
        client_at_block,
        location,
        account_bytes,
    )
    .await
    .map_err(|_| AccountsError::PalletNotAvailable("ForeignAssets".to_string()))?;

    let asset_account = match decoded {
        Some(a) => a,
        None => return Ok(None),
    };

    let multi_location_json = serde_json::to_value(location).unwrap_or(serde_json::json!({}));

    Ok(Some(ForeignAssetBalance {
        multi_location: multi_location_json,
        balance: asset_account.balance.to_string(),
        is_frozen: asset_account.is_frozen,
        is_sufficient: asset_account.sufficient,
    }))
}

/// Parse foreign asset location JSON strings into Location objects.
///
/// Uses `staging_xcm::v4::Location` for JSON deserialization (which has full
/// serde support), then SCALE-encodes and decodes into our typed Location struct.
///
/// Accepts both the form this API emits and the older Sidecar-style query form:
/// string-encoded numbers (`"parents": "2"`), grouped numbers (`"Parachain": "1,000"`),
/// `0x`-prefixed hex byte arrays, and camelCase keys (`chainId`, `blockNumber`,
/// `blockHash`). Plain numbers and snake_case keys keep working.
pub fn parse_foreign_asset_locations(
    json_strings: &[String],
) -> Result<Vec<Location>, AccountsError> {
    use parity_scale_codec::Decode;

    let mut locations = Vec::new();
    for json_str in json_strings {
        // Parse JSON string
        let mut json_value: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| AccountsError::InvalidForeignAsset(format!("Invalid JSON: {}", e)))?;

        // Normalize API/Sidecar JSON so staging_xcm can deserialize it.
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

/// Recursively normalize location JSON so `staging_xcm` can deserialize it.
///
/// Output `Serialize` matches Sidecar: numbers as strings (sometimes grouped with
/// commas), byte arrays as `0x` hex, and a few camelCase keys. `staging_xcm`
/// wants JSON numbers, `[u8; N]` arrays, and snake_case field names.
fn coerce_string_numbers(value: &mut serde_json::Value) {
    const KEY_ALIASES: &[(&str, &str)] = &[
        ("chainId", "chain_id"),
        ("blockNumber", "block_number"),
        ("blockHash", "block_hash"),
    ];

    match value {
        serde_json::Value::String(s) => {
            if let Some(bytes) = decode_0x_hex(s) {
                *value = serde_json::Value::Array(
                    bytes.into_iter().map(|b| serde_json::json!(b)).collect(),
                );
                return;
            }
            let stripped: String = s.chars().filter(|c| *c != ',').collect();
            if let Ok(n) = stripped.parse::<u128>() {
                *value = serde_json::json!(n);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                coerce_string_numbers(item);
            }
        }
        serde_json::Value::Object(map) => {
            for &(from, to) in KEY_ALIASES {
                // If both camelCase and snake_case forms are supplied, prefer
                // the existing snake_case field and discard the camelCase alias.
                if let Some(v) = map.remove(from)
                    && !map.contains_key(to)
                {
                    map.insert(to.to_string(), v);
                }
            }
            for val in map.values_mut() {
                coerce_string_numbers(val);
            }
        }
        _ => {}
    }
}

/// Decode a `0x`-prefixed hex string into bytes. Odd length or invalid hex
/// is left for serde to reject.
fn decode_0x_hex(s: &str) -> Option<Vec<u8>> {
    let hex_str = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    if hex_str.is_empty() || hex_str.len() % 2 != 0 {
        return None;
    }
    hex::decode(hex_str).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::common::xcm_types::{BodyId, BodyPart, Junction, Junctions, NetworkId};

    fn parse_one(json: &str) -> Location {
        let mut locations =
            parse_foreign_asset_locations(&[json.to_string()]).expect("location should parse");
        assert_eq!(locations.len(), 1);
        locations.pop().unwrap()
    }

    fn account_key20(hex_body: &str) -> [u8; 20] {
        let bytes = hex::decode(hex_body).unwrap();
        let mut key = [0u8; 20];
        key.copy_from_slice(&bytes);
        key
    }

    #[test]
    fn parse_round_trips_serialized_locations() {
        let cases = vec![
            (
                "Parachain",
                Location {
                    parents: 1,
                    interior: Junctions::X1([Junction::Parachain(1000)]),
                },
            ),
            (
                "AccountId32",
                Location {
                    parents: 0,
                    interior: Junctions::X1([Junction::AccountId32 {
                        network: None,
                        id: [0x42; 32],
                    }]),
                },
            ),
            (
                "AccountIndex64",
                Location {
                    parents: 0,
                    interior: Junctions::X1([Junction::AccountIndex64 {
                        network: None,
                        index: 1234,
                    }]),
                },
            ),
            (
                "AccountKey20",
                Location {
                    parents: 0,
                    interior: Junctions::X1([Junction::AccountKey20 {
                        network: None,
                        key: [0x07; 20],
                    }]),
                },
            ),
            (
                "PalletInstance",
                Location {
                    parents: 0,
                    interior: Junctions::X1([Junction::PalletInstance(50)]),
                },
            ),
            (
                "GeneralIndex",
                Location {
                    parents: 0,
                    interior: Junctions::X1([Junction::GeneralIndex(12345)]),
                },
            ),
            (
                "GeneralKey",
                Location {
                    parents: 0,
                    interior: Junctions::X1([Junction::GeneralKey {
                        length: 4,
                        data: {
                            let mut d = [0u8; 32];
                            d[0..4].copy_from_slice(&[1, 2, 3, 4]);
                            d
                        },
                    }]),
                },
            ),
            (
                "OnlyChild",
                Location {
                    parents: 0,
                    interior: Junctions::X1([Junction::OnlyChild]),
                },
            ),
            (
                "Plurality with Moniker",
                Location {
                    parents: 0,
                    interior: Junctions::X1([Junction::Plurality {
                        id: BodyId::Moniker(*b"TEST"),
                        part: BodyPart::Voice,
                    }]),
                },
            ),
            (
                "GlobalConsensus ByGenesis",
                Location {
                    parents: 2,
                    interior: Junctions::X1([Junction::GlobalConsensus(NetworkId::ByGenesis(
                        [0x09; 32],
                    ))]),
                },
            ),
            (
                "GlobalConsensus ByFork",
                Location {
                    parents: 2,
                    interior: Junctions::X1([Junction::GlobalConsensus(NetworkId::ByFork {
                        block_number: 1_234_567,
                        block_hash: [0xab; 32],
                    })]),
                },
            ),
            (
                "GlobalConsensus Ethereum",
                Location {
                    parents: 2,
                    interior: Junctions::X1([Junction::GlobalConsensus(NetworkId::Ethereum {
                        chain_id: 1,
                    })]),
                },
            ),
            (
                "GlobalConsensus Polkadot",
                Location {
                    parents: 2,
                    interior: Junctions::X1([Junction::GlobalConsensus(NetworkId::Polkadot)]),
                },
            ),
            (
                "Multi-junction Ethereum and AccountKey20",
                Location {
                    parents: 2,
                    interior: Junctions::X2([
                        Junction::GlobalConsensus(NetworkId::Ethereum { chain_id: 1 }),
                        Junction::AccountKey20 {
                            network: None,
                            key: account_key20("9d39a5de30e57443bff2a8307a4256c8797a3497"),
                        },
                    ]),
                },
            ),
        ];

        for (name, original) in cases {
            let json = serde_json::to_string(&original)
                .unwrap_or_else(|e| panic!("failed to serialize {name}: {e}"));
            let parsed = parse_one(&json);
            assert_eq!(parsed, original, "round-trip mismatch for case: {name}");
        }
    }

    #[test]
    fn parse_prefers_snake_case_over_camel_case_on_collision() {
        // Ethereum: chain_id takes precedence over chainId when both are present with different values.
        let eth_json = r#"{"parents":"2","interior":{"X1":[{"GlobalConsensus":{"Ethereum":{"chainId":"999","chain_id":"1"}}}]}}"#;
        let eth_loc = parse_one(eth_json);
        assert_eq!(
            eth_loc,
            Location {
                parents: 2,
                interior: Junctions::X1([Junction::GlobalConsensus(NetworkId::Ethereum {
                    chain_id: 1
                })]),
            }
        );

        // ByFork: block_number and block_hash take precedence over blockNumber and blockHash when both are present.
        let fork_json = r#"{"parents":"2","interior":{"X1":[{"GlobalConsensus":{"ByFork":{"blockNumber":"999","block_number":"1,000","blockHash":"0x9999999999999999999999999999999999999999999999999999999999999999","block_hash":"0x1111111111111111111111111111111111111111111111111111111111111111"}}}]}}"#;
        let fork_loc = parse_one(fork_json);
        assert_eq!(
            fork_loc,
            Location {
                parents: 2,
                interior: Junctions::X1([Junction::GlobalConsensus(NetworkId::ByFork {
                    block_number: 1000,
                    block_hash: [0x11; 32],
                })]),
            }
        );
    }

    #[test]
    fn parse_still_accepts_snake_case_and_plain_numbers() {
        let loc = parse_one(
            r#"{"parents":"2","interior":{"X1":[{"GlobalConsensus":{"Ethereum":{"chain_id":"1"}}}]}}"#,
        );
        assert_eq!(
            loc,
            Location {
                parents: 2,
                interior: Junctions::X1([Junction::GlobalConsensus(NetworkId::Ethereum {
                    chain_id: 1
                })]),
            }
        );

        let loc = parse_one(r#"{"parents":1,"interior":{"X1":[{"Parachain":1000}]}}"#);
        assert_eq!(
            loc,
            Location {
                parents: 1,
                interior: Junctions::X1([Junction::Parachain(1000)]),
            }
        );

        let loc = parse_one(
            r#"{"parents":2,"interior":{"X1":[{"GlobalConsensus":{"ByFork":{"block_number":1000,"block_hash":[17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17,17]}}}]}}"#,
        );
        assert_eq!(
            loc,
            Location {
                parents: 2,
                interior: Junctions::X1([Junction::GlobalConsensus(NetworkId::ByFork {
                    block_number: 1000,
                    block_hash: [17; 32],
                })]),
            }
        );
    }

    #[test]
    fn parse_rejects_invalid_json() {
        let err = parse_foreign_asset_locations(&["not-valid-json".to_string()]).unwrap_err();
        match err {
            AccountsError::InvalidForeignAsset(msg) => {
                assert!(msg.contains("Invalid JSON"), "unexpected error: {msg}");
            }
            other => panic!("expected InvalidForeignAsset, got {other:?}"),
        }
    }
}
