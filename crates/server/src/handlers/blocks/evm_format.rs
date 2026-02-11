//! EVM address format conversion utilities.
//!
//! This module provides functionality to convert AccountId32 addresses to EVM format
//! (20-byte Ethereum-style addresses) for events in the 'revive' pallet.
//!
//! The conversion extracts the first 20 bytes of a 32-byte AccountId32 address
//! and formats it as a hex string, matching the format used by EVM chains.

use serde_json::Value;
use sp_core::crypto::{AccountId32, Ss58Codec};

use super::types::ExtrinsicInfo;

/// Checks if the 'revive' pallet is present in the chain metadata.
pub fn has_revive_pallet(metadata: &subxt::Metadata) -> bool {
    metadata
        .pallets()
        .any(|p| p.name().to_lowercase() == "revive")
}

/// Applies EVM address format conversion to extrinsics from the 'revive' pallet.
pub fn apply_evm_format(extrinsics: &mut [ExtrinsicInfo], metadata: &subxt::Metadata) {
    if !has_revive_pallet(metadata) {
        return;
    }

    for extrinsic in extrinsics.iter_mut() {
        if extrinsic.method.pallet.to_lowercase() == "revive" {
            for event in extrinsic.events.iter_mut() {
                event.data = event.data.iter().map(convert_data_to_evm_address).collect();
            }
        }
    }
}

/// Recursively converts AccountId32 addresses to EVM format in JSON data.
fn convert_data_to_evm_address(data: &Value) -> Value {
    match data {
        Value::Array(arr) => Value::Array(arr.iter().map(convert_data_to_evm_address).collect()),
        Value::String(s) => {
            if let Some(evm_address) = try_convert_to_evm_address(s) {
                Value::String(evm_address)
            } else {
                Value::String(s.clone())
            }
        }
        Value::Object(obj) => {
            let converted: serde_json::Map<String, Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), convert_data_to_evm_address(v)))
                .collect();
            Value::Object(converted)
        }
        _ => data.clone(),
    }
}

/// Attempts to convert a string to EVM address format if it's a valid SS58 AccountId32.
/// Only converts valid SS58 addresses
fn try_convert_to_evm_address(s: &str) -> Option<String> {
    if let Ok(account_id) = AccountId32::from_ss58check(s) {
        return Some(account_id_to_evm_hex(&account_id));
    }
    None
}

/// Converts an AccountId32 to EVM hex format (0x + first 20 bytes).
fn account_id_to_evm_hex(account_id: &AccountId32) -> String {
    let bytes: &[u8] = account_id.as_ref();
    let evm_bytes = &bytes[..20];
    format!("0x{}", hex::encode(evm_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_account_id_to_evm_hex() {
        let bytes = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ];
        let account_id = AccountId32::from(bytes);

        let evm_hex = account_id_to_evm_hex(&account_id);

        assert_eq!(evm_hex, "0x0102030405060708090a0b0c0d0e0f1011121314");
    }

    #[test]
    fn test_convert_ss58_address() {
        // Valid SS58 address (Alice on Polkadot)
        let data = json!([
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            "some_other_string",
            42
        ]);

        let result = convert_data_to_evm_address(&data);

        assert!(result.is_array());
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        // First element (SS58) should be converted to EVM address (42 chars = 0x + 40 hex)
        assert!(arr[0].as_str().unwrap().starts_with("0x"));
        assert_eq!(arr[0].as_str().unwrap().len(), 42);
        // Second element should be unchanged
        assert_eq!(arr[1].as_str().unwrap(), "some_other_string");
        // Third element should be unchanged
        assert_eq!(arr[2].as_i64().unwrap(), 42);
    }

    #[test]
    fn test_hex_strings_not_converted() {
        // 32-byte hex strings (H256) should NOT be converted - they could be topics/hashes
        let h256_topic = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        // 20-byte hex strings (H160) should NOT be converted - already EVM format
        let h160_address = "0x0102030405060708090a0b0c0d0e0f1011121314";

        let data = json!([h256_topic, h160_address]);

        let result = convert_data_to_evm_address(&data);

        let arr = result.as_array().unwrap();
        // H256 should be unchanged (not truncated!)
        assert_eq!(arr[0].as_str().unwrap(), h256_topic);
        // H160 should be unchanged (already correct format)
        assert_eq!(arr[1].as_str().unwrap(), h160_address);
    }

    #[test]
    fn test_convert_nested_object_with_ss58() {
        // Use SS58 addresses which should be converted
        let ss58_address = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
        let data = json!({
            "address": ss58_address,
            "nested": {
                "value": "unchanged",
                "another_address": ss58_address
            }
        });

        let result = convert_data_to_evm_address(&data);

        assert!(result.is_object());
        let obj = result.as_object().unwrap();
        // Top-level SS58 address should be converted
        assert!(obj["address"].as_str().unwrap().starts_with("0x"));
        assert_eq!(obj["address"].as_str().unwrap().len(), 42);
        // Nested value should be unchanged
        assert_eq!(obj["nested"]["value"].as_str().unwrap(), "unchanged");
        // Nested SS58 address should be converted
        assert!(
            obj["nested"]["another_address"]
                .as_str()
                .unwrap()
                .starts_with("0x")
        );
        assert_eq!(obj["nested"]["another_address"].as_str().unwrap().len(), 42);
    }

    #[test]
    fn test_non_address_strings_unchanged() {
        let data = json!([
            "hello",
            "0x123", // Too short
            "not_an_address",
            "",
            // H256 hash (like event topic) - must NOT be truncated
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
        ]);

        let result = convert_data_to_evm_address(&data);

        let arr = result.as_array().unwrap();
        assert_eq!(arr[0].as_str().unwrap(), "hello");
        assert_eq!(arr[1].as_str().unwrap(), "0x123");
        assert_eq!(arr[2].as_str().unwrap(), "not_an_address");
        assert_eq!(arr[3].as_str().unwrap(), "");
        // H256 topic hash must remain intact
        assert_eq!(
            arr[4].as_str().unwrap(),
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        );
    }
}
