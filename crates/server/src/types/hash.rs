use serde::{Deserialize, Serialize};
use sp_core::H256;
use std::fmt;

// Note: subxt-rpcs uses primitive_types::H256, sp_core::H256 is re-exported
// We need to support both types
use primitive_types::H256 as PrimitiveH256;

/// Wrapper type for block hashes with controlled string representation.
///
/// This provides a single source of truth for how block hashes are formatted
/// in API responses. All block hashes should use this type instead of raw H256.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockHash(pub H256);

impl BlockHash {
    /// Create a BlockHash from H256
    pub fn new(hash: H256) -> Self {
        Self(hash)
    }

    /// Get the inner H256
    pub fn inner(&self) -> &H256 {
        &self.0
    }

    /// Convert to H256
    pub fn into_inner(self) -> H256 {
        self.0
    }

    /// Get the hash as bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_fixed_bytes()
    }
}

impl From<H256> for BlockHash {
    fn from(hash: H256) -> Self {
        Self(hash)
    }
}

impl From<PrimitiveH256> for BlockHash {
    fn from(hash: PrimitiveH256) -> Self {
        // Convert primitive_types::H256 to sp_core::H256
        Self(H256::from(hash.0))
    }
}

impl From<BlockHash> for H256 {
    fn from(hash: BlockHash) -> Self {
        hash.0
    }
}

impl From<[u8; 32]> for BlockHash {
    fn from(bytes: [u8; 32]) -> Self {
        Self(H256::from(bytes))
    }
}

/// Display implementation for API responses
/// Format: "0x" followed by 64 lowercase hex characters
impl fmt::Display for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use H256's Debug impl which formats as "0x..."
        write!(f, "{:?}", self.0)
    }
}

/// Serialize as hex string with "0x" prefix for JSON responses
impl Serialize for BlockHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Deserialize from hex string (with or without "0x" prefix)
impl<'de> Deserialize<'de> for BlockHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let hex_str = s.strip_prefix("0x").unwrap_or(&s);

        let bytes = hex::decode(hex_str).map_err(serde::de::Error::custom)?;

        if bytes.len() != 32 {
            return Err(serde::de::Error::custom(format!(
                "Expected 32 bytes, got {}",
                bytes.len()
            )));
        }

        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(&bytes);

        Ok(BlockHash(H256::from(hash_bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_hash_display() {
        let hash = BlockHash(H256::from([0x42; 32]));
        let display = hash.to_string();

        assert!(display.starts_with("0x"));
        assert_eq!(display.len(), 66); // "0x" + 64 hex chars
        assert_eq!(
            display,
            "0x4242424242424242424242424242424242424242424242424242424242424242"
        );
    }

    #[test]
    fn test_block_hash_serialize() {
        let hash = BlockHash(H256::from([0x42; 32]));
        let json = serde_json::to_string(&hash).unwrap();

        assert_eq!(
            json,
            "\"0x4242424242424242424242424242424242424242424242424242424242424242\""
        );
    }

    #[test]
    fn test_block_hash_deserialize() {
        let json = "\"0x4242424242424242424242424242424242424242424242424242424242424242\"";
        let hash: BlockHash = serde_json::from_str(json).unwrap();

        assert_eq!(hash.0, H256::from([0x42; 32]));
    }

    #[test]
    fn test_block_hash_deserialize_without_prefix() {
        let json = "\"4242424242424242424242424242424242424242424242424242424242424242\"";
        let hash: BlockHash = serde_json::from_str(json).unwrap();

        assert_eq!(hash.0, H256::from([0x42; 32]));
    }

    #[test]
    fn test_block_hash_from_h256() {
        let h256 = H256::from([0x42; 32]);
        let hash = BlockHash::from(h256);

        assert_eq!(hash.0, h256);
    }

    #[test]
    fn test_block_hash_into_h256() {
        let hash = BlockHash(H256::from([0x42; 32]));
        let h256: H256 = hash.into();

        assert_eq!(h256, H256::from([0x42; 32]));
    }
}
