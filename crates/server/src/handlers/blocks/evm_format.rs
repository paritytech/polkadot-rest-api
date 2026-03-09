// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! EVM address format conversion utilities.
//!
//! This module provides functionality to convert AccountId32 addresses to EVM format
//! (20-byte Ethereum-style addresses) for events in the 'revive' pallet.
//!

use serde_json::Value;
use sp_core::{
    crypto::{AccountId32, Ss58Codec},
    keccak_256,
};

use super::types::ExtrinsicInfo;

/// Checks if the 'revive' pallet is present in the chain metadata.
pub fn has_revive_pallet(metadata: &subxt::Metadata) -> bool {
    metadata
        .pallets()
        .any(|p| p.name().to_lowercase() == "revive")
}

/// Applies EVM address format conversion to revive pallet events.
pub fn apply_evm_format(extrinsics: &mut [ExtrinsicInfo], metadata: &subxt::Metadata) {
    if !has_revive_pallet(metadata) {
        return;
    }

    for extrinsic in extrinsics.iter_mut() {
        if extrinsic.method.pallet.to_lowercase() == "revive" {
            for event in extrinsic.events.iter_mut() {
                if event.method.pallet.to_lowercase() == "revive" {
                    event.data = event.data.iter().map(convert_data_to_evm_address).collect();
                }
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
fn try_convert_to_evm_address(s: &str) -> Option<String> {
    let account_id = AccountId32::from_ss58check(s).ok()?;
    let bytes: &[u8; 32] = account_id.as_ref();

    if bytes[20..] == [0xEE; 12] {
        Some(format!("0x{}", hex::encode(&bytes[..20])))
    } else {
        let hash = keccak_256(bytes);
        Some(format!("0x{}", hex::encode(&hash[12..])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn eth_derived_account(evm_bytes: &[u8; 20]) -> AccountId32 {
        let mut bytes = [0xEE_u8; 32];
        bytes[..20].copy_from_slice(evm_bytes);
        AccountId32::from(bytes)
    }

    fn expected_keccak_evm(account: &AccountId32) -> String {
        let bytes: &[u8; 32] = account.as_ref();
        let hash = keccak_256(bytes);
        format!("0x{}", hex::encode(&hash[12..]))
    }

    #[test]
    fn test_eth_derived_address_converted() {
        // Eth-derived: 20 bytes of EVM address + 12 × 0xEE padding
        let evm_bytes: [u8; 20] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
        ];
        let account = eth_derived_account(&evm_bytes);
        let ss58 = account.to_ss58check();

        let result = try_convert_to_evm_address(&ss58);

        assert_eq!(
            result,
            Some("0x0102030405060708090a0b0c0d0e0f1011121314".to_string())
        );
    }

    #[test]
    fn test_mapped_account_keccak_hashed() {
        let mut bytes = [0u8; 32];
        bytes[..20].copy_from_slice(&[
            0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45,
            0x67, 0x89, 0xab, 0xcd, 0xef, 0x01,
        ]);
        let account = AccountId32::from(bytes);
        let ss58 = account.to_ss58check();

        let result = try_convert_to_evm_address(&ss58);

        assert_eq!(result, Some(expected_keccak_evm(&account)));
    }

    #[test]
    fn test_native_substrate_address_keccak_hashed() {
        let alice_ss58 = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
        let alice_account = AccountId32::from_ss58check(alice_ss58).unwrap();

        let result = try_convert_to_evm_address(alice_ss58);

        assert_eq!(result, Some(expected_keccak_evm(&alice_account)));
    }

    #[test]
    fn test_convert_mixed_data() {
        let evm_bytes: [u8; 20] = [
            0xde, 0xad, 0xbe, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23,
            0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
        ];
        let eth_account = eth_derived_account(&evm_bytes);
        let eth_ss58 = eth_account.to_ss58check();

        let data = json!([eth_ss58, "some_other_string", 42]);

        let result = convert_data_to_evm_address(&data);

        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(
            arr[0].as_str().unwrap(),
            "0xdeadbeef0123456789abcdef0123456789abcdef"
        );
        // Non-SS58 string unchanged
        assert_eq!(arr[1].as_str().unwrap(), "some_other_string");
        // Number unchanged
        assert_eq!(arr[2].as_i64().unwrap(), 42);
    }

    #[test]
    fn test_hex_strings_not_converted() {
        // Hex strings are not valid SS58, so they pass through unchanged
        let h256_topic = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        let h160_address = "0x0102030405060708090a0b0c0d0e0f1011121314";

        let data = json!([h256_topic, h160_address]);

        let result = convert_data_to_evm_address(&data);

        let arr = result.as_array().unwrap();
        assert_eq!(arr[0].as_str().unwrap(), h256_topic);
        assert_eq!(arr[1].as_str().unwrap(), h160_address);
    }

    #[test]
    fn test_convert_nested_object() {
        let evm_bytes: [u8; 20] = [
            0xde, 0xad, 0xbe, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23,
            0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
        ];
        let eth_account = eth_derived_account(&evm_bytes);
        let eth_ss58 = eth_account.to_ss58check();

        let data = json!({
            "address": eth_ss58,
            "nested": {
                "value": "unchanged",
                "another_address": eth_ss58
            }
        });

        let result = convert_data_to_evm_address(&data);

        let obj = result.as_object().unwrap();
        assert_eq!(
            obj["address"].as_str().unwrap(),
            "0xdeadbeef0123456789abcdef0123456789abcdef"
        );
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
            "0x123",
            "not_an_address",
            "",
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
        ]);

        let result = convert_data_to_evm_address(&data);

        let arr = result.as_array().unwrap();
        assert_eq!(arr[0].as_str().unwrap(), "hello");
        assert_eq!(arr[1].as_str().unwrap(), "0x123");
        assert_eq!(arr[2].as_str().unwrap(), "not_an_address");
        assert_eq!(arr[3].as_str().unwrap(), "");
        assert_eq!(
            arr[4].as_str().unwrap(),
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        );
    }
}
