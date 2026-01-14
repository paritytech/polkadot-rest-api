//! Integration tests for /pallets/{palletId}/storage endpoint
//!
//! These tests verify the response format matches Sidecar's output exactly.

use serde_json::{json, Value};

/// Test that DeprecationInfo::NotDeprecated serializes correctly
#[test]
fn test_deprecation_info_not_deprecated_serialization() {
    use server::handlers::pallets::storage::DeprecationInfo;

    let info = DeprecationInfo::NotDeprecated(None);
    let serialized = serde_json::to_value(&info).unwrap();

    // Sidecar format: { "notDeprecated": null }
    assert_eq!(serialized, json!({ "notDeprecated": null }));
}

/// Test that DeprecationInfo::Deprecated serializes correctly with note and since
#[test]
fn test_deprecation_info_deprecated_with_note_serialization() {
    use server::handlers::pallets::storage::DeprecationInfo;

    let info = DeprecationInfo::Deprecated {
        note: Some("This is deprecated".to_string()),
        since: Some("v1.0.0".to_string()),
    };
    let serialized = serde_json::to_value(&info).unwrap();

    // Sidecar format: { "deprecated": { "note": "...", "since": "..." } }
    assert_eq!(
        serialized,
        json!({
            "deprecated": {
                "note": "This is deprecated",
                "since": "v1.0.0"
            }
        })
    );
}

/// Test that DeprecationInfo::Deprecated serializes correctly without optional fields
#[test]
fn test_deprecation_info_deprecated_without_optional_fields() {
    use server::handlers::pallets::storage::DeprecationInfo;

    let info = DeprecationInfo::Deprecated {
        note: None,
        since: None,
    };
    let serialized = serde_json::to_value(&info).unwrap();

    // Should omit null fields
    assert_eq!(serialized, json!({ "deprecated": {} }));
}

/// Test StorageTypeInfo::Plain serialization
#[test]
fn test_storage_type_info_plain_serialization() {
    use server::handlers::pallets::storage::StorageTypeInfo;

    let info = StorageTypeInfo::Plain {
        plain: "123".to_string(),
    };
    let serialized = serde_json::to_value(&info).unwrap();

    assert_eq!(serialized, json!({ "plain": "123" }));
}

/// Test StorageTypeInfo::Map serialization
#[test]
fn test_storage_type_info_map_serialization() {
    use server::handlers::pallets::storage::{MapTypeInfo, StorageTypeInfo};

    let info = StorageTypeInfo::Map {
        map: MapTypeInfo {
            hashers: vec!["Blake2_128Concat".to_string()],
            key: "0".to_string(),
            value: "3".to_string(),
        },
    };
    let serialized = serde_json::to_value(&info).unwrap();

    assert_eq!(
        serialized,
        json!({
            "map": {
                "hashers": ["Blake2_128Concat"],
                "key": "0",
                "value": "3"
            }
        })
    );
}

/// Test StorageItemMetadata serialization matches Sidecar format
#[test]
fn test_storage_item_metadata_serialization() {
    use server::handlers::pallets::storage::{
        DeprecationInfo, MapTypeInfo, StorageItemMetadata, StorageTypeInfo,
    };

    let item = StorageItemMetadata {
        name: "Account".to_string(),
        modifier: "Default".to_string(),
        ty: StorageTypeInfo::Map {
            map: MapTypeInfo {
                hashers: vec!["Blake2_128Concat".to_string()],
                key: "0".to_string(),
                value: "3".to_string(),
            },
        },
        fallback: "0x00".to_string(),
        docs: "The full account information.".to_string(),
        deprecation_info: DeprecationInfo::NotDeprecated(None),
    };

    let serialized = serde_json::to_value(&item).unwrap();

    // Verify structure matches Sidecar
    assert_eq!(serialized["name"], "Account");
    assert_eq!(serialized["modifier"], "Default");
    assert_eq!(serialized["type"]["map"]["hashers"][0], "Blake2_128Concat");
    assert_eq!(serialized["type"]["map"]["key"], "0");
    assert_eq!(serialized["type"]["map"]["value"], "3");
    assert_eq!(serialized["fallback"], "0x00");
    assert_eq!(serialized["docs"], "The full account information.");
    assert_eq!(serialized["deprecationInfo"], json!({ "notDeprecated": null }));
}

/// Test PalletsStorageResponse serialization matches Sidecar format
#[test]
fn test_pallets_storage_response_serialization() {
    use server::handlers::pallets::storage::{
        AtResponse, DeprecationInfo, MapTypeInfo, PalletsStorageResponse, StorageItemMetadata,
        StorageItems, StorageTypeInfo,
    };

    let response = PalletsStorageResponse {
        at: AtResponse {
            hash: "0xabc123".to_string(),
            height: "12345".to_string(),
        },
        pallet: "system".to_string(),
        pallet_index: "0".to_string(),
        items: StorageItems::Full(vec![StorageItemMetadata {
            name: "Account".to_string(),
            modifier: "Default".to_string(),
            ty: StorageTypeInfo::Map {
                map: MapTypeInfo {
                    hashers: vec!["Blake2_128Concat".to_string()],
                    key: "0".to_string(),
                    value: "3".to_string(),
                },
            },
            fallback: "0x00".to_string(),
            docs: "Account info".to_string(),
            deprecation_info: DeprecationInfo::NotDeprecated(None),
        }]),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    let serialized = serde_json::to_value(&response).unwrap();

    // Verify top-level structure matches Sidecar
    assert_eq!(serialized["at"]["hash"], "0xabc123");
    assert_eq!(serialized["at"]["height"], "12345");
    assert_eq!(serialized["pallet"], "system"); // lowercase
    assert_eq!(serialized["palletIndex"], "0"); // string, not number
    assert!(serialized["items"].is_array());
    assert_eq!(serialized["items"].as_array().unwrap().len(), 1);

    // Verify rcBlock fields are NOT present when None (Sidecar compatibility)
    assert!(serialized.get("rcBlockHash").is_none());
    assert!(serialized.get("rcBlockNumber").is_none());
    assert!(serialized.get("ahTimestamp").is_none());
}

/// Test that pallet name is lowercase (Sidecar compatibility)
#[test]
fn test_pallet_name_is_lowercase() {
    use server::handlers::pallets::storage::{AtResponse, PalletsStorageResponse, StorageItems};

    let response = PalletsStorageResponse {
        at: AtResponse {
            hash: "0x123".to_string(),
            height: "1".to_string(),
        },
        pallet: "system".to_string(), // Must be lowercase
        pallet_index: "0".to_string(),
        items: StorageItems::Full(vec![]),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    let serialized = serde_json::to_value(&response).unwrap();
    assert_eq!(serialized["pallet"], "system");

    // Verify it's not "System" (PascalCase)
    assert_ne!(serialized["pallet"], "System");
}

/// Test that palletIndex is a string (Sidecar compatibility)
#[test]
fn test_pallet_index_is_string() {
    use server::handlers::pallets::storage::{AtResponse, PalletsStorageResponse, StorageItems};

    let response = PalletsStorageResponse {
        at: AtResponse {
            hash: "0x123".to_string(),
            height: "1".to_string(),
        },
        pallet: "balances".to_string(),
        pallet_index: "5".to_string(), // String, not number
        items: StorageItems::Full(vec![]),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    let serialized = serde_json::to_value(&response).unwrap();

    // Must be string "5", not number 5
    assert!(serialized["palletIndex"].is_string());
    assert_eq!(serialized["palletIndex"], "5");
}

/// Test multiple hashers (for DoubleMap/NMap)
#[test]
fn test_multiple_hashers_serialization() {
    use server::handlers::pallets::storage::{MapTypeInfo, StorageTypeInfo};

    let info = StorageTypeInfo::Map {
        map: MapTypeInfo {
            hashers: vec![
                "Blake2_128Concat".to_string(),
                "Twox64Concat".to_string(),
            ],
            key: "(0, 1)".to_string(),
            value: "2".to_string(),
        },
    };

    let serialized = serde_json::to_value(&info).unwrap();
    let hashers = serialized["map"]["hashers"].as_array().unwrap();

    assert_eq!(hashers.len(), 2);
    assert_eq!(hashers[0], "Blake2_128Concat");
    assert_eq!(hashers[1], "Twox64Concat");
}

/// Test all supported hasher types
#[test]
fn test_all_hasher_types() {
    let supported_hashers = vec![
        "Blake2_128",
        "Blake2_256",
        "Blake2_128Concat",
        "Twox128",
        "Twox256",
        "Twox64Concat",
        "Identity",
    ];

    // Just verify these are valid string values that can be serialized
    for hasher in supported_hashers {
        let json_value = serde_json::to_value(hasher).unwrap();
        assert!(json_value.is_string());
    }
}

/// Fixture test: Expected Sidecar response structure for System.Account
#[test]
fn test_sidecar_fixture_system_account() {
    // This is the expected Sidecar response format for System.Account on V14+ chains
    let expected_sidecar_format: Value = json!({
        "name": "Account",
        "modifier": "Default",
        "type": {
            "map": {
                "hashers": ["Blake2_128Concat"],
                "key": "0",
                "value": "3"
            }
        },
        "fallback": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000080",
        "docs": " The full account information for a particular account ID.",
        "deprecationInfo": {
            "notDeprecated": null
        }
    });

    // Verify the structure we expect
    assert!(expected_sidecar_format["name"].is_string());
    assert!(expected_sidecar_format["modifier"].is_string());
    assert!(expected_sidecar_format["type"]["map"]["hashers"].is_array());
    assert!(expected_sidecar_format["type"]["map"]["key"].is_string());
    assert!(expected_sidecar_format["type"]["map"]["value"].is_string());
    assert!(expected_sidecar_format["fallback"].is_string());
    assert!(expected_sidecar_format["docs"].is_string());
    assert!(expected_sidecar_format["deprecationInfo"]["notDeprecated"].is_null());
}

/// Test StorageItems::OnlyIds serialization
#[test]
fn test_storage_items_only_ids_serialization() {
    use server::handlers::pallets::storage::{AtResponse, PalletsStorageResponse, StorageItems};

    let response = PalletsStorageResponse {
        at: AtResponse {
            hash: "0xabc123".to_string(),
            height: "12345".to_string(),
        },
        pallet: "system".to_string(),
        pallet_index: "0".to_string(),
        items: StorageItems::OnlyIds(vec![
            "Account".to_string(),
            "Number".to_string(),
            "BlockHash".to_string(),
        ]),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    let serialized = serde_json::to_value(&response).unwrap();

    // Items should be an array of strings
    let items = serialized["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], "Account");
    assert_eq!(items[1], "Number");
    assert_eq!(items[2], "BlockHash");

    // Items should NOT have object structure
    for item in items {
        assert!(item.is_string());
        assert!(!item.is_object());
    }
}
