use crate::types::BlockHash;
use config::Hasher;
use parity_scale_codec::{Decode, Encode};
use sp_core::H256;
use sp_runtime::generic::{Digest, DigestItem};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HashError {
    #[error("Missing header field: {0}")]
    MissingField(String),

    #[error("Invalid hex format: {0}")]
    InvalidHex(String),

    #[error("Invalid number format: {0}")]
    InvalidNumber(String),

    #[error("SCALE decoding error: {0}")]
    ScaleDecodeError(String),
}

/// Extension trait for Hasher to provide hashing functionality
pub trait HasherExt {
    /// Hash the given data and return a 32-byte hash
    fn hash(&self, data: &[u8]) -> [u8; 32];
}

impl HasherExt for Hasher {
    fn hash(&self, data: &[u8]) -> [u8; 32] {
        match self {
            Hasher::Blake2_256 => sp_core::blake2_256(data),
            Hasher::Keccak256 => sp_core::keccak_256(data),
        }
    }
}

/// Compute the block hash from header JSON fields
///
/// This reconstructs the SCALE-encoded header and hashes it with Blake2b-256,
/// matching Substrate's block hash calculation.
///
/// Returns `BlockHash` wrapper type with controlled string formatting.
/// Convert to string with `hash.to_string()` or use directly in JSON responses.
pub fn compute_block_hash_from_header_json(
    header_json: &serde_json::Value,
) -> Result<BlockHash, HashError> {
    compute_block_hash_from_header_json_with_hasher(header_json, &Hasher::Blake2_256)
}

/// Compute the block hash from header JSON fields with configurable hasher
///
/// This reconstructs the SCALE-encoded header and hashes it with the specified hasher,
/// matching the chain's block hash calculation.
///
/// Returns `BlockHash` wrapper type with controlled string formatting.
pub fn compute_block_hash_from_header_json_with_hasher(
    header_json: &serde_json::Value,
    hasher: &Hasher,
) -> Result<BlockHash, HashError> {
    let parent_hash = extract_hash(header_json, "parentHash")?;
    let number = extract_block_number(header_json, "number")?;
    let state_root = extract_hash(header_json, "stateRoot")?;
    let extrinsics_root = extract_hash(header_json, "extrinsicsRoot")?;
    let digest = extract_digest(header_json)?;

    // Construct Header and encode it
    let header = sp_runtime::generic::Header::<u32, sp_runtime::traits::BlakeTwo256> {
        parent_hash,
        number,
        state_root,
        extrinsics_root,
        digest,
    };

    // Compute hash using the configured hasher on SCALE-encoded header
    let encoded = header.encode();
    let hash = hasher.hash(&encoded);

    Ok(BlockHash::from(H256::from(hash)))
}

/// Extract a hash field (H256) from JSON
fn extract_hash(json: &serde_json::Value, field: &str) -> Result<H256, HashError> {
    let hex_str = json
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| HashError::MissingField(field.to_string()))?;

    parse_hash(hex_str)
}

/// Parse a hex string into H256
fn parse_hash(hex_str: &str) -> Result<H256, HashError> {
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    let bytes =
        hex::decode(hex_str).map_err(|e| HashError::InvalidHex(format!("{}: {}", hex_str, e)))?;

    if bytes.len() != 32 {
        return Err(HashError::InvalidHex(format!(
            "Expected 32 bytes, got {}",
            bytes.len()
        )));
    }

    Ok(H256::from_slice(&bytes))
}

/// Extract block number from JSON header field.
///
/// Handles both formats returned by different RPC implementations:
/// - Hex string: `"number": "0x1a2b3c"` (standard Substrate nodes)
/// - Numeric: `"number": 12345` (Smoldot light client)
fn extract_block_number(json: &serde_json::Value, field: &str) -> Result<u32, HashError> {
    let value = json
        .get(field)
        .ok_or_else(|| HashError::MissingField(field.to_string()))?;

    parse_block_number_from_json(value)
        .map(|n| n as u32)
        .map_err(HashError::InvalidNumber)
}

/// Parse a block number from a JSON value.
///
/// Handles both formats returned by different RPC implementations:
/// - Hex string: `"0x1a2b3c"` (standard Substrate nodes)
/// - Numeric: `12345` (Smoldot light client)
///
/// This mirrors subxt's `NumberOrHex` deserialization approach.
pub fn parse_block_number_from_json(value: &serde_json::Value) -> Result<u64, String> {
    // Try as string first (hex format from standard Substrate nodes)
    if let Some(hex_str) = value.as_str() {
        let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        return u64::from_str_radix(hex_str, 16)
            .map_err(|e| format!("invalid hex '{}': {}", hex_str, e));
    }

    // Try as number (numeric format from Smoldot)
    if let Some(num) = value.as_u64() {
        return Ok(num);
    }

    Err(format!("expected hex string or number, got: {}", value))
}

/// Extract and decode digest from JSON
fn extract_digest(json: &serde_json::Value) -> Result<Digest, HashError> {
    let logs_array = json
        .get("digest")
        .and_then(|d| d.get("logs"))
        .and_then(|l| l.as_array())
        .ok_or_else(|| HashError::MissingField("digest.logs".to_string()))?;

    let mut digest_items = Vec::new();

    for log_hex in logs_array {
        let log_str = log_hex
            .as_str()
            .ok_or_else(|| HashError::InvalidHex("digest log is not a string".to_string()))?;

        let log_bytes = hex::decode(log_str.strip_prefix("0x").unwrap_or(log_str))
            .map_err(|e| HashError::InvalidHex(format!("digest log: {}", e)))?;

        // Decode DigestItem from SCALE-encoded bytes
        let digest_item = DigestItem::decode(&mut &log_bytes[..])
            .map_err(|e| HashError::ScaleDecodeError(format!("digest item: {}", e)))?;

        digest_items.push(digest_item);
    }

    Ok(Digest { logs: digest_items })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_hash() {
        let hash_str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let result = parse_hash(hash_str);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_block_number_from_json_hex_string() {
        // Hex string format (standard Substrate nodes)
        assert_eq!(parse_block_number_from_json(&json!("0x64")).unwrap(), 100);
        assert_eq!(parse_block_number_from_json(&json!("64")).unwrap(), 100);
        assert_eq!(parse_block_number_from_json(&json!("0x0")).unwrap(), 0);
        assert_eq!(
            parse_block_number_from_json(&json!("0xf4240")).unwrap(),
            1_000_000
        );
    }

    #[test]
    fn test_parse_block_number_from_json_numeric() {
        // Numeric format (Smoldot light client)
        assert_eq!(parse_block_number_from_json(&json!(100)).unwrap(), 100);
        assert_eq!(parse_block_number_from_json(&json!(0)).unwrap(), 0);
        assert_eq!(
            parse_block_number_from_json(&json!(1_000_000)).unwrap(),
            1_000_000
        );
    }

    #[test]
    fn test_parse_block_number_from_json_invalid() {
        // Invalid formats
        assert!(parse_block_number_from_json(&json!(null)).is_err());
        assert!(parse_block_number_from_json(&json!([])).is_err());
        assert!(parse_block_number_from_json(&json!({})).is_err());
        assert!(parse_block_number_from_json(&json!("not_hex")).is_err());
    }

    #[test]
    fn test_compute_hash_missing_field() {
        let json = json!({
            "parentHash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        });

        let result = compute_block_hash_from_header_json(&json);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), HashError::MissingField(_)));
    }

    #[test]
    fn test_compute_hash_real_polkadot_block() {
        // Real header from Polkadot block 1,000,000
        // Expected hash: 0x490cd542b4a40ad743183c7d1088a4fe7b1edf21e50c850b86f29e389f31c5c1
        let header_json = json!({
            "parentHash": "0xe89d37367d4c8310520d6f0a43cfaa4a722aa54a9a3fbdf59a108cfaec8fc3b3",
            "number": "0xf4240",
            "stateRoot": "0xf85945dd5a4cc62fda9527b07e58632b0c12a78ce5f0e4d92e44b65c5919ec49",
            "extrinsicsRoot": "0xfd4fdcff91ba217c04b1eb64925c354e31a3f7cc27bda72f7f090a5a27b532bc",
            "digest": {
                "logs": [
                    "0x0642414245b5010397000000c52edc0f000000002477179454d7803d78c89a458f3aa1223f887c67c71383ee10003f0919caff0390e039aef579da4d501499311e62f3567305255a5613e75e4f816b166f511908dd0a875741ac1cf53c81b59a36971e0303c443e475c2fc59bfb2385946b5a702",
                    "0x05424142450101ecf08f9ed7930c0ce2b9e8eeca3f2432e828cddf53bc067c9cfafe31522b26194acaac7672a63e238f4b93b18f24c6d4a211bfccb1c9caf0270a29c4eab2418e"
                ]
            }
        });

        let computed_hash = compute_block_hash_from_header_json(&header_json)
            .expect("Failed to compute hash from real Polkadot header");

        // Expected hash as BlockHash
        let expected_hash = BlockHash::from(H256::from_slice(
            &hex::decode("490cd542b4a40ad743183c7d1088a4fe7b1edf21e50c850b86f29e389f31c5c1")
                .unwrap(),
        ));

        assert_eq!(
            computed_hash, expected_hash,
            "Computed hash doesn't match actual Polkadot block 1,000,000 hash"
        );

        // Verify the string representation uses to_string() (idiomatic)
        assert_eq!(
            computed_hash.to_string(),
            "0x490cd542b4a40ad743183c7d1088a4fe7b1edf21e50c850b86f29e389f31c5c1"
        );
    }

    #[test]
    fn test_compute_hash_simple_header() {
        // Simplified header with empty digest for basic testing
        let header_json = json!({
            "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "number": "0x1",
            "stateRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "extrinsicsRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "digest": {
                "logs": []
            }
        });

        let result = compute_block_hash_from_header_json(&header_json);
        assert!(
            result.is_ok(),
            "Should successfully compute hash for simple header"
        );

        // Hash should be valid BlockHash wrapping 32 bytes
        let hash = result.unwrap();
        assert_eq!(hash.as_bytes().len(), 32);

        // String representation should be a valid 0x-prefixed hex string
        let hash_str = hash.to_string();
        assert!(hash_str.starts_with("0x"));
        assert_eq!(hash_str.len(), 66); // "0x" + 64 hex chars
    }

    #[test]
    fn test_hasher_blake2_256() {
        use config::Hasher;

        let data = b"test data for hashing";
        let hash = Hasher::Blake2_256.hash(data);
        assert_eq!(hash.len(), 32);

        // Hash should be deterministic
        let hash2 = Hasher::Blake2_256.hash(data);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_hasher_keccak256() {
        use config::Hasher;

        let data = b"test data for hashing";
        let hash = Hasher::Keccak256.hash(data);
        assert_eq!(hash.len(), 32);

        // Different hasher should produce different hash
        let blake_hash = Hasher::Blake2_256.hash(data);
        assert_ne!(hash, blake_hash);
    }

    #[test]
    fn test_hasher_produces_consistent_results() {
        use config::Hasher;

        let data = b"consistent data";
        let hash1 = Hasher::Blake2_256.hash(data);
        let hash2 = Hasher::Blake2_256.hash(data);
        let hash3 = Hasher::Blake2_256.hash(data);

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }

    #[test]
    fn test_compute_hash_with_different_hashers() {
        let header_json = json!({
            "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "number": "0x1",
            "stateRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "extrinsicsRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "digest": {"logs": []}
        });

        let hash_blake2 = compute_block_hash_from_header_json_with_hasher(
            &header_json,
            &config::Hasher::Blake2_256,
        )
        .unwrap();

        let hash_keccak = compute_block_hash_from_header_json_with_hasher(
            &header_json,
            &config::Hasher::Keccak256,
        )
        .unwrap();

        // Different hashers should produce different hashes
        assert_ne!(hash_blake2.as_bytes(), hash_keccak.as_bytes());
    }
}
