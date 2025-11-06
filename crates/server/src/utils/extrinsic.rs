//! Extrinsic parsing utilities
//!
//! This module contains utilities for parsing and extracting information from
//! Substrate extrinsics, particularly era/mortality information.

use serde::Serialize;
use serde_json::Value;

/// Era information for extrinsics
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EraInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub immortal_era: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mortal_era: Option<Vec<String>>,
}

/// Decode Era from SCALE bytes using sp_runtime::generic::Era
pub fn decode_era_from_bytes(bytes: &[u8], offset: &mut usize) -> Option<EraInfo> {
    use parity_scale_codec::Decode;
    use sp_runtime::generic::Era;

    if *offset >= bytes.len() {
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
            tracing::trace!("Failed to decode Era: {:?}", e);
            None
        }
    }
}

/// Extract era from raw extrinsic bytes by manually parsing SCALE encoding
///
/// In a signed extrinsic, the structure is:
/// [version:1 byte] [address:SCALE] [signature:SCALE] [extra:SCALE] [call:SCALE]
///
/// Era is the first field in the extra/SignedExtra section
pub fn extract_era_from_extrinsic_bytes(bytes: &[u8]) -> Option<EraInfo> {
    use parity_scale_codec::Decode;

    if bytes.is_empty() {
        return None;
    }

    // First byte is version (signed bit | version)
    let version = bytes[0];
    if version & 0b1000_0000 == 0 {
        // Unsigned extrinsic - immortal
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
        Err(_) => {
            tracing::trace!("Failed to decode address variant");
            return None;
        }
    };

    // Parse address based on variant
    match address_variant {
        0x00 => {
            // Id variant - AccountId32 (32 bytes)
            if cursor.len() < 32 {
                tracing::trace!("Not enough bytes for Id variant");
                return None;
            }
            cursor = &cursor[32..];
        }
        0x01 => {
            // Index variant - Compact<u32>
            if parity_scale_codec::Compact::<u32>::decode(&mut cursor).is_err() {
                tracing::trace!("Failed to decode Index variant");
                return None;
            }
        }
        0x02 => {
            // Raw variant - Vec<u8> (compact length + bytes)
            let len = match parity_scale_codec::Compact::<u32>::decode(&mut cursor) {
                Ok(l) => l.0 as usize,
                Err(_) => {
                    tracing::trace!("Failed to decode Raw variant length");
                    return None;
                }
            };
            if cursor.len() < len {
                tracing::trace!("Not enough bytes for Raw variant");
                return None;
            }
            cursor = &cursor[len..];
        }
        0x03 => {
            // Address32 variant - [u8; 32]
            if cursor.len() < 32 {
                tracing::trace!("Not enough bytes for Address32 variant");
                return None;
            }
            cursor = &cursor[32..];
        }
        0x04 => {
            // Address20 variant - [u8; 20]
            if cursor.len() < 20 {
                tracing::trace!("Not enough bytes for Address20 variant");
                return None;
            }
            cursor = &cursor[20..];
        }
        _ => {
            tracing::trace!("Unknown address variant: {}", address_variant);
            return None;
        }
    }

    // Decode and skip MultiSignature enum
    let sig_variant = match u8::decode(&mut cursor) {
        Ok(v) => v,
        Err(_) => {
            tracing::trace!("Failed to decode signature variant");
            return None;
        }
    };

    // Parse signature based on variant
    match sig_variant {
        0x00 | 0x01 => {
            // Ed25519 (0x00) or Sr25519 (0x01) - both 64 bytes
            if cursor.len() < 64 {
                tracing::trace!("Not enough bytes for Ed25519/Sr25519 signature");
                return None;
            }
            cursor = &cursor[64..];
        }
        0x02 => {
            // Ecdsa - 65 bytes
            if cursor.len() < 65 {
                tracing::trace!("Not enough bytes for Ecdsa signature");
                return None;
            }
            cursor = &cursor[65..];
        }
        _ => {
            tracing::trace!("Unknown signature variant: {}", sig_variant);
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
/// This is used when era is extracted through subxt's transaction extensions API
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
}
