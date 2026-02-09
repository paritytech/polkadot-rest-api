//! Formatting utilities for hex encoding and address conversion.

use sp_core::crypto::{AccountId32, Ss58Codec};

/// Format bytes as hex string with "0x" prefix
pub fn hex_with_prefix(data: &[u8]) -> String {
    format!("0x{}", hex::encode(data))
}

/// Convert to lowerCamelCase by only lowercasing the first character
/// This preserves snake_case names (e.g., "inbound_messages_data" stays unchanged)
/// while converting PascalCase to lowerCamelCase (e.g., "PreRuntime" → "preRuntime")
/// Used for SCALE enum variant names which should preserve their original casing
pub fn lowercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().chain(chars).collect(),
    }
}

/// Decode account address bytes to SS58 format
/// Tries to decode:
/// 1. MultiAddress::Id variant (0x00 + 32 bytes)
/// 2. Raw 32-byte AccountId32 (0x + 32 bytes)
pub fn decode_address_to_ss58(hex_str: &str, ss58_prefix: u16) -> Option<String> {
    if !hex_str.starts_with("0x") {
        return None;
    }

    let account_bytes = if hex_str.starts_with("0x00") && hex_str.len() == 68 {
        // MultiAddress::Id: skip "0x00" variant prefix
        match hex::decode(&hex_str[4..]) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!(
                    hex_str = %hex_str,
                    error = %e,
                    "Failed to hex decode MultiAddress::Id field"
                );
                return None;
            }
        }
    } else if hex_str.len() == 66 {
        // Raw AccountId32: skip "0x" prefix
        match hex::decode(&hex_str[2..]) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!(
                    hex_str = %hex_str,
                    error = %e,
                    "Failed to hex decode raw AccountId32 field"
                );
                return None;
            }
        }
    } else {
        return None;
    };

    // Must be exactly 32 bytes
    if account_bytes.len() != 32 {
        tracing::debug!(
            hex_str = %hex_str,
            byte_len = account_bytes.len(),
            "Decoded bytes are not 32 bytes, skipping SS58 conversion"
        );
        return None;
    }

    // Convert to AccountId32
    let account_id = match AccountId32::try_from(account_bytes.as_slice()) {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(
                hex_str = %hex_str,
                error = ?e,
                "Failed to convert bytes to AccountId32"
            );
            return None;
        }
    };

    // Encode to SS58 with chain-specific prefix
    Some(
        account_id
            .to_ss58check_with_version(sp_core::crypto::Ss58AddressFormat::custom(ss58_prefix)),
    )
}

pub fn to_camel_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
    }
}
