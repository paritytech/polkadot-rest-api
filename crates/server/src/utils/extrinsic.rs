// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Extrinsic parsing utilities
//!
//! This module handles era/mortality extraction from Substrate extrinsics.
//!
//! ## Why Manual Parsing?
//!
//! Era information is encoded in the SignedExtra/TransactionExtensions section
//! of signed extrinsics. While modern Substrate uses transaction extensions,
//! the era data isn't always reliably exposed as a named extension through
//! subxt's API (e.g., CheckMortality may not appear in the extensions list).
//!
//! To reliably extract era information, we manually parse the SCALE-encoded
//! extrinsic bytes by navigating through the structure:
//! 1. Version byte (signed bit | version)
//! 2. MultiAddress (sender address - 5 variants)
//! 3. MultiSignature (signature - 3 types: Ed25519, Sr25519, Ecdsa)
//! 4. SignedExtra/Extensions (era is the first field here)
//!
//! This approach works across different Substrate versions and chain configurations.
//!
//! ## Functions
//!
//! - [`extract_era_from_extrinsic_bytes`]: Main function to extract era from raw extrinsic bytes
//! - [`decode_era_from_bytes`]: Low-level SCALE decoder for era bytes
//! - [`parse_era_info`]: JSON parser for era from transaction extension data

use serde::Serialize;
use serde_json::Value;

/// Era information for extrinsics
///
/// Represents transaction mortality - whether a transaction is valid indefinitely
/// or only for a specific number of blocks.
///
/// ## Mortality
///
/// Substrate transactions can be either immortal or mortal:
///
/// - **Immortal**: The transaction never expires and can be included in any future block.
///   This is represented as `immortalEra: "0x00"`. Typically used for unsigned extrinsics
///   (like inherents) or transactions that don't need time bounds.
///
/// - **Mortal**: The transaction expires after a certain number of blocks. This prevents
///   replay attacks and helps manage the transaction pool. Represented as
///   `mortalEra: [period, phase]` where:
///   - `period`: Number of blocks the transaction is valid for (power of 2, e.g., 64, 128)
///   - `phase`: Offset within the period when the transaction can be included
///
/// ## Example
///
/// ```json
/// {
///   "era": {
///     "mortalEra": ["64", "19"]
///   }
/// }
/// ```
///
/// This indicates the transaction is valid for 64 blocks, with phase 19.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EraInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub immortal_era: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mortal_era: Option<Vec<String>>,
}

/// Decode Era from SCALE bytes using sp_runtime::generic::Era
///
/// Decodes era information from raw SCALE-encoded bytes at the given offset.
///
/// # Arguments
///
/// * `bytes` - SCALE-encoded bytes containing era data
/// * `offset` - Current position in bytes, will be updated after decoding
///
/// # Returns
///
/// * `Some(EraInfo)` - Successfully decoded era information
/// * `None` - Failed to decode (insufficient bytes or invalid encoding)
pub fn decode_era_from_bytes(bytes: &[u8], offset: &mut usize) -> Option<EraInfo> {
    use parity_scale_codec::Decode;
    use sp_runtime::generic::Era;

    if *offset >= bytes.len() {
        tracing::debug!(
            "Cannot decode era: offset {} exceeds byte length {}",
            offset,
            bytes.len()
        );
        return None;
    }

    // Try to decode Era using the built-in SCALE decoder
    let mut cursor = &bytes[*offset..];
    match Era::decode(&mut cursor) {
        Ok(era) => {
            // Update offset based on how many bytes were consumed
            let consumed = bytes[*offset..].len() - cursor.len();
            *offset += consumed;

            match era {
                Era::Immortal => Some(EraInfo {
                    immortal_era: Some("0x00".to_string()),
                    mortal_era: None,
                }),
                Era::Mortal(period, phase) => {
                    // Note: the period in Era::Mortal is the actual period (power of 2)
                    // and phase is the quantized phase
                    // To get the values matching sidecar, we need period and phase as-is
                    Some(EraInfo {
                        immortal_era: None,
                        mortal_era: Some(vec![period.to_string(), phase.to_string()]),
                    })
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                "Failed to decode Era from bytes at offset {}: {:?}",
                offset,
                e
            );
            None
        }
    }
}

/// Extract era from raw extrinsic bytes by manually parsing SCALE encoding
///
/// This is the main function for extracting era information from Substrate extrinsics.
/// It manually navigates through the SCALE-encoded structure to find the era field.
///
/// # Arguments
///
/// * `bytes` - Raw extrinsic bytes from `extrinsic.bytes()` (without length prefix)
///
/// # Returns
///
/// * `Some(EraInfo)` - Successfully extracted era (immortal for unsigned, mortal/immortal for signed)
/// * `None` - Parsing failed due to malformed or insufficient bytes
///
/// # Structure
///
/// For signed extrinsics, the byte structure is:
/// ```text
/// [version:1 byte] [address:SCALE] [signature:SCALE] [extra:SCALE] [call:SCALE]
/// ```
///
/// Era is the first field in the extra/SignedExtra section.
pub fn extract_era_from_extrinsic_bytes(bytes: &[u8]) -> Option<EraInfo> {
    use parity_scale_codec::Decode;

    if bytes.is_empty() {
        tracing::debug!("Cannot extract era: empty extrinsic bytes");
        return None;
    }

    // First byte is version (signed bit | version)
    let version = bytes[0];
    if version & 0b1000_0000 == 0 {
        // Unsigned extrinsic - immortal
        tracing::trace!("Unsigned extrinsic detected, using immortal era");
        return Some(EraInfo {
            immortal_era: Some("0x00".to_string()),
            mortal_era: None,
        });
    }

    // For signed extrinsics, manually parse SCALE encoding to find era
    let mut cursor = &bytes[1..]; // Skip version byte

    // Decode and skip MultiAddress enum
    let address_variant = match u8::decode(&mut cursor) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to decode address variant from extrinsic: {:?}", e);
            return None;
        }
    };

    // Parse address based on variant
    match address_variant {
        0x00 => {
            // Id variant - AccountId32 (32 bytes)
            if cursor.len() < 32 {
                tracing::warn!(
                    "Insufficient bytes for Id address variant: need 32, have {}",
                    cursor.len()
                );
                return None;
            }
            cursor = &cursor[32..];
        }
        0x01 => {
            // Index variant - Compact<u32>
            if let Err(e) = parity_scale_codec::Compact::<u32>::decode(&mut cursor) {
                tracing::warn!("Failed to decode Index address variant: {:?}", e);
                return None;
            }
        }
        0x02 => {
            // Raw variant - Vec<u8> (compact length + bytes)
            let len = match parity_scale_codec::Compact::<u32>::decode(&mut cursor) {
                Ok(l) => l.0 as usize,
                Err(e) => {
                    tracing::warn!("Failed to decode Raw address variant length: {:?}", e);
                    return None;
                }
            };
            if cursor.len() < len {
                tracing::warn!(
                    "Insufficient bytes for Raw address variant: need {}, have {}",
                    len,
                    cursor.len()
                );
                return None;
            }
            cursor = &cursor[len..];
        }
        0x03 => {
            // Address32 variant - [u8; 32]
            if cursor.len() < 32 {
                tracing::warn!(
                    "Insufficient bytes for Address32 variant: need 32, have {}",
                    cursor.len()
                );
                return None;
            }
            cursor = &cursor[32..];
        }
        0x04 => {
            // Address20 variant - [u8; 20]
            if cursor.len() < 20 {
                tracing::warn!(
                    "Insufficient bytes for Address20 variant: need 20, have {}",
                    cursor.len()
                );
                return None;
            }
            cursor = &cursor[20..];
        }
        _ => {
            tracing::warn!("Unknown address variant: 0x{:02x}", address_variant);
            return None;
        }
    }

    // Decode and skip MultiSignature enum
    let sig_variant = match u8::decode(&mut cursor) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to decode signature variant from extrinsic: {:?}", e);
            return None;
        }
    };

    // Parse signature based on variant
    match sig_variant {
        0x00 | 0x01 => {
            // Ed25519 (0x00) or Sr25519 (0x01) - both 64 bytes
            if cursor.len() < 64 {
                tracing::warn!(
                    "Insufficient bytes for Ed25519/Sr25519 signature: need 64, have {}",
                    cursor.len()
                );
                return None;
            }
            cursor = &cursor[64..];
        }
        0x02 => {
            // Ecdsa - 65 bytes
            if cursor.len() < 65 {
                tracing::warn!(
                    "Insufficient bytes for Ecdsa signature: need 65, have {}",
                    cursor.len()
                );
                return None;
            }
            cursor = &cursor[65..];
        }
        _ => {
            tracing::warn!("Unknown signature variant: 0x{:02x}", sig_variant);
            return None;
        }
    }

    // Now we're at the SignedExtra/TransactionExtensions section
    // Era is the first field encoded here
    tracing::trace!(
        "Remaining bytes after address+signature: {} bytes, first few: {:?}",
        cursor.len(),
        &cursor[..cursor.len().min(10)]
    );

    let mut offset = 0;
    let result = decode_era_from_bytes(cursor, &mut offset);

    tracing::trace!("Era decode result: {:?}", result);

    result
}

/// Parse era information from transaction extension JSON
///
/// This is a fallback parser used when era information is successfully extracted
/// through subxt's transaction extensions API (when CheckMortality extension is present).
///
/// # Arguments
///
/// * `era_json` - JSON value containing era data from transaction extension
///
/// # Returns
///
/// `EraInfo` - Parsed era information, defaults to immortal if parsing fails
///
/// # Note
///
/// This function is less commonly used than [`extract_era_from_extrinsic_bytes`]
/// because CheckMortality doesn't always appear in the extension list. It's kept
/// as a fallback for cases where the extension is available.
pub fn parse_era_info(era_json: &Value) -> EraInfo {
    // Era can be immortal (0x00) or mortal (period, phase)
    // Check if it's an object with "Mortal" or contains array with period/phase
    if let Value::Object(map) = era_json
        && let Some(Value::String(name)) = map.get("name")
        && name == "Mortal"
        && let Some(Value::Array(values)) = map.get("values")
    {
        let mortal_values: Vec<String> = values
            .iter()
            .filter_map(|v| {
                if let Value::Array(arr) = v {
                    arr.first()
                        .and_then(|val| val.as_u64())
                        .map(|n| n.to_string())
                } else {
                    v.as_u64().map(|n| n.to_string())
                }
            })
            .collect();

        if !mortal_values.is_empty() {
            return EraInfo {
                immortal_era: None,
                mortal_era: Some(mortal_values),
            };
        }
    }

    // Default to immortal
    EraInfo {
        immortal_era: Some("0x00".to_string()),
        mortal_era: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_immortal_era() {
        let bytes = [0x00];
        let mut offset = 0;
        let result = decode_era_from_bytes(&bytes, &mut offset);

        assert!(result.is_some());
        let era = result.unwrap();
        assert_eq!(era.immortal_era, Some("0x00".to_string()));
        assert_eq!(era.mortal_era, None);
        assert_eq!(offset, 1);
    }

    #[test]
    fn test_decode_mortal_era() {
        // Example mortal era bytes: 0xe602 should decode to period=128, phase=46
        let bytes = [0xe6, 0x02];
        let mut offset = 0;
        let result = decode_era_from_bytes(&bytes, &mut offset);

        assert!(result.is_some());
        let era = result.unwrap();
        assert_eq!(era.immortal_era, None);
        assert_eq!(
            era.mortal_era,
            Some(vec!["128".to_string(), "46".to_string()])
        );
        assert_eq!(offset, 2);
    }

    #[test]
    fn test_parse_era_info_mortal() {
        let json = serde_json::json!({
            "name": "Mortal",
            "values": [[128], [46]]
        });

        let result = parse_era_info(&json);
        assert_eq!(result.immortal_era, None);
        assert_eq!(
            result.mortal_era,
            Some(vec!["128".to_string(), "46".to_string()])
        );
    }

    #[test]
    fn test_parse_era_info_default_immortal() {
        let json = serde_json::json!({
            "name": "Unknown",
            "values": []
        });

        let result = parse_era_info(&json);
        assert_eq!(result.immortal_era, Some("0x00".to_string()));
        assert_eq!(result.mortal_era, None);
    }

    #[test]
    fn test_extract_era_unsigned_extrinsic() {
        // Unsigned extrinsic starts with version byte without signed bit (e.g., 0x04)
        let extrinsic_bytes = vec![0x04, 0x00, 0x01]; // Minimal unsigned extrinsic
        let result = extract_era_from_extrinsic_bytes(&extrinsic_bytes);

        assert!(result.is_some());
        let era = result.unwrap();
        assert_eq!(era.immortal_era, Some("0x00".to_string()));
        assert_eq!(era.mortal_era, None);
    }

    #[test]
    fn test_extract_era_signed_extrinsic_with_immortal_era() {
        // Construct a minimal signed extrinsic:
        // - Version byte: 0x84 (signed bit | version 4)
        // - MultiAddress Id variant (0x00) + 32 bytes AccountId32
        // - MultiSignature Sr25519 variant (0x01) + 64 bytes signature
        // - Era: 0x00 (immortal)
        let mut extrinsic_bytes = vec![0x84]; // Signed version byte

        // Address: Id variant (0x00) + 32-byte account
        extrinsic_bytes.push(0x00);
        extrinsic_bytes.extend_from_slice(&[0x42; 32]);

        // Signature: Sr25519 variant (0x01) + 64-byte signature
        extrinsic_bytes.push(0x01);
        extrinsic_bytes.extend_from_slice(&[0xAA; 64]);

        // Era: Immortal (0x00)
        extrinsic_bytes.push(0x00);

        let result = extract_era_from_extrinsic_bytes(&extrinsic_bytes);

        assert!(result.is_some());
        let era = result.unwrap();
        assert_eq!(era.immortal_era, Some("0x00".to_string()));
        assert_eq!(era.mortal_era, None);
    }

    #[test]
    fn test_extract_era_signed_extrinsic_with_mortal_era() {
        // Construct a signed extrinsic with mortal era (period=128, phase=46)
        let mut extrinsic_bytes = vec![0x84]; // Signed version byte

        // Address: Id variant (0x00) + 32-byte account
        extrinsic_bytes.push(0x00);
        extrinsic_bytes.extend_from_slice(&[0x42; 32]);

        // Signature: Sr25519 variant (0x01) + 64-byte signature
        extrinsic_bytes.push(0x01);
        extrinsic_bytes.extend_from_slice(&[0xAA; 64]);

        // Era: Mortal (0xe602 = period 128, phase 46)
        extrinsic_bytes.push(0xe6);
        extrinsic_bytes.push(0x02);

        let result = extract_era_from_extrinsic_bytes(&extrinsic_bytes);

        assert!(result.is_some());
        let era = result.unwrap();
        assert_eq!(era.immortal_era, None);
        assert_eq!(
            era.mortal_era,
            Some(vec!["128".to_string(), "46".to_string()])
        );
    }

    #[test]
    fn test_extract_era_signed_extrinsic_with_ed25519_signature() {
        // Test with Ed25519 signature variant (0x00)
        let mut extrinsic_bytes = vec![0x84];

        // Address: Id variant + 32-byte account
        extrinsic_bytes.push(0x00);
        extrinsic_bytes.extend_from_slice(&[0x42; 32]);

        // Signature: Ed25519 variant (0x00) + 64-byte signature
        extrinsic_bytes.push(0x00);
        extrinsic_bytes.extend_from_slice(&[0xBB; 64]);

        // Era: Immortal
        extrinsic_bytes.push(0x00);

        let result = extract_era_from_extrinsic_bytes(&extrinsic_bytes);

        assert!(result.is_some());
        let era = result.unwrap();
        assert_eq!(era.immortal_era, Some("0x00".to_string()));
        assert_eq!(era.mortal_era, None);
    }

    #[test]
    fn test_extract_era_signed_extrinsic_with_ecdsa_signature() {
        // Test with Ecdsa signature variant (0x02) which is 65 bytes
        let mut extrinsic_bytes = vec![0x84];

        // Address: Id variant + 32-byte account
        extrinsic_bytes.push(0x00);
        extrinsic_bytes.extend_from_slice(&[0x42; 32]);

        // Signature: Ecdsa variant (0x02) + 65-byte signature
        extrinsic_bytes.push(0x02);
        extrinsic_bytes.extend_from_slice(&[0xCC; 65]);

        // Era: Mortal
        extrinsic_bytes.push(0xe6);
        extrinsic_bytes.push(0x02);

        let result = extract_era_from_extrinsic_bytes(&extrinsic_bytes);

        assert!(result.is_some());
        let era = result.unwrap();
        assert_eq!(era.immortal_era, None);
        assert_eq!(
            era.mortal_era,
            Some(vec!["128".to_string(), "46".to_string()])
        );
    }

    #[test]
    fn test_extract_era_empty_bytes() {
        let result = extract_era_from_extrinsic_bytes(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_era_insufficient_bytes() {
        // Not enough bytes after address to contain signature
        let mut extrinsic_bytes = vec![0x84];
        extrinsic_bytes.push(0x00);
        extrinsic_bytes.extend_from_slice(&[0x42; 32]);
        extrinsic_bytes.push(0x01); // Signature variant but no signature bytes

        let result = extract_era_from_extrinsic_bytes(&extrinsic_bytes);
        assert!(result.is_none());
    }

    // Real-world test fixtures from Polkadot chain
    // These tests use actual extrinsic bytes captured from the chain to ensure
    // our parsing logic works correctly with production data.

    #[test]
    fn test_extract_era_real_polkadot_staking_nominate() {
        // Source: Polkadot block 24500000, extrinsic index 2
        // Method: Staking::nominate
        // Expected era: mortalEra ["64", "19"]
        //
        // Note: The raw bytes from the chain include a compact length prefix (ed09)
        // which indicates the extrinsic is 605 bytes long. The actual extrinsic data
        // starts at the version byte (84).
        //
        // Breakdown of the extrinsic bytes (after length prefix):
        // - 84: Version byte (signed, version 4)
        // - 00: Address variant (Id)
        // - af3e1d...224b74: 32-byte AccountId32
        // - 00: Signature variant (Ed25519)
        // - 48ceb5...48c06: 64-byte signature
        // - 35: First era byte
        // - 01: Second era byte (together 0x3501 encodes period=64, phase=19)
        // - 74: Nonce (compact encoded)
        // - 00: Tip (compact encoded)
        // - ...rest of extrinsic (call data)
        let extrinsic_hex = "8400af3e1db41e95040f7630e64d1b3104235c08545e452b15fd70601881aa224b740048ceb5c1995db4427ba1322f48702cebe4b4564e03d660d6a713f25e48143be454875d56716def88a61283643fcb9a0aed7caccbfe285dfba8399b07bc448c063501740001070540000000966d74f8027e07b43717b6876d97544fe0d71facef06acc8382749ae944e00005fa73637062b";
        let extrinsic_bytes = hex::decode(extrinsic_hex).unwrap();

        let result = extract_era_from_extrinsic_bytes(&extrinsic_bytes);

        assert!(
            result.is_some(),
            "Should successfully parse real Polkadot extrinsic"
        );
        let era = result.unwrap();
        assert_eq!(era.immortal_era, None);
        assert_eq!(
            era.mortal_era,
            Some(vec!["64".to_string(), "19".to_string()]),
            "Should extract correct mortal era from real extrinsic"
        );
    }

    #[test]
    fn test_decode_era_bytes_from_polkadot_extrinsic() {
        // The era bytes extracted from the above extrinsic: 0x3501
        // This encodes period=64, phase=19
        let era_bytes = hex::decode("3501").unwrap();
        let mut offset = 0;

        let result = decode_era_from_bytes(&era_bytes, &mut offset);

        assert!(result.is_some());
        let era = result.unwrap();
        assert_eq!(era.immortal_era, None);
        assert_eq!(
            era.mortal_era,
            Some(vec!["64".to_string(), "19".to_string()])
        );
        assert_eq!(offset, 2, "Should consume both era bytes");
    }
}
