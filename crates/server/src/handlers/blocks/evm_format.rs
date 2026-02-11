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

/// Attempts to convert a string to EVM address format if it's a valid SS58 AccountId32
fn try_convert_to_evm_address(s: &str) -> Option<String> {
    if let Ok(account_id) = AccountId32::from_ss58check(s) {
        let bytes: &[u8] = account_id.as_ref();
        if bytes[20..].iter().all(|&b| b == 0) {
            return Some(format!("0x{}", hex::encode(&bytes[..20])));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_mapped_evm_address_converted() {
        // A mapped EVM address: 20 bytes of EVM address + 12 zero bytes
        let mut bytes = [0u8; 32];
        bytes[..20].copy_from_slice(&[
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
        ]);
        // bytes[20..32] are already zeros
        let account_id = AccountId32::from(bytes);
        let ss58 = account_id.to_ss58check();

        let result = try_convert_to_evm_address(&ss58);

        assert_eq!(
            result,
            Some("0x0102030405060708090a0b0c0d0e0f1011121314".to_string())
        );
    }

    #[test]
    fn test_native_substrate_address_unchanged() {
        // Native Substrate address (Alice) - has non-zero trailing bytes, should NOT be converted
        let alice_ss58 = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";

        let result = try_convert_to_evm_address(alice_ss58);

        assert_eq!(result, None);
    }

    #[test]
    fn test_convert_mixed_addresses() {
        // Create a mapped EVM address for testing
        let mut bytes = [0u8; 32];
        bytes[..20].copy_from_slice(&[
            0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45,
            0x67, 0x89, 0xab, 0xcd, 0xef, 0x01,
        ]);
        let mapped_account = AccountId32::from(bytes);
        let mapped_ss58 = mapped_account.to_ss58check();

        let data = json!([
            mapped_ss58,
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY", // Alice - native, unchanged
            "some_other_string",
            42
        ]);

        let result = convert_data_to_evm_address(&data);

        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        // Mapped EVM address should be converted
        assert_eq!(
            arr[0].as_str().unwrap(),
            "0xabcdef0123456789abcdef0123456789abcdef01"
        );
        // Native Substrate address should be unchanged
        assert_eq!(
            arr[1].as_str().unwrap(),
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
        );
        // Other string unchanged
        assert_eq!(arr[2].as_str().unwrap(), "some_other_string");
        // Number unchanged
        assert_eq!(arr[3].as_i64().unwrap(), 42);
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
    fn test_convert_nested_object_with_mapped_evm() {
        // Create a mapped EVM address (20 bytes + 12 zero bytes)
        let mut bytes = [0u8; 32];
        bytes[..20].copy_from_slice(&[
            0xde, 0xad, 0xbe, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23,
            0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
        ]);
        let mapped_account = AccountId32::from(bytes);
        let mapped_ss58 = mapped_account.to_ss58check();

        let data = json!({
            "address": mapped_ss58,
            "nested": {
                "value": "unchanged",
                "another_address": mapped_ss58
            }
        });

        let result = convert_data_to_evm_address(&data);

        assert!(result.is_object());
        let obj = result.as_object().unwrap();
        assert_eq!(
            obj["address"].as_str().unwrap(),
            "0xdeadbeef0123456789abcdef0123456789abcdef"
        );
        // Nested value should be unchanged
        assert_eq!(obj["nested"]["value"].as_str().unwrap(), "unchanged");
        assert_eq!(
            obj["nested"]["another_address"].as_str().unwrap(),
            "0xdeadbeef0123456789abcdef0123456789abcdef"
        );
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
