//! Shared XCM type definitions for typed SCALE encoding/decoding.
//!
//! These types represent XCM v4 Location and its components. They implement
//! `DecodeAsType`/`EncodeAsType` for use with subxt 0.50.0 dynamic storage queries,
//! and custom `Serialize` implementations that produce Sidecar-compatible JSON output.

use parity_scale_codec::{Decode, Encode};
use serde::{Serialize, Serializer, ser::SerializeMap};

// ============================================================================
// XCM Location Types (DecodeAsType + EncodeAsType for typed encoding/decoding)
// ============================================================================

/// XCM v4 Location type for foreign asset keys.
/// Uses DecodeAsType/EncodeAsType for efficient typed encoding/decoding from Subxt.
/// Custom Serialize produces Sidecar-compatible JSON format.
#[derive(
    Debug,
    Clone,
    Decode,
    Encode,
    subxt::ext::scale_decode::DecodeAsType,
    subxt::ext::scale_encode::EncodeAsType,
)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
#[encode_as_type(crate_path = "subxt::ext::scale_encode")]
pub struct Location {
    pub parents: u8,
    pub interior: Junctions,
}

impl Serialize for Location {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("parents", &self.parents.to_string())?;
        map.serialize_entry("interior", &self.interior)?;
        map.end()
    }
}

/// XCM v4 Junctions enum - represents the interior path of a Location.
#[derive(
    Debug,
    Clone,
    Decode,
    Encode,
    subxt::ext::scale_decode::DecodeAsType,
    subxt::ext::scale_encode::EncodeAsType,
)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
#[encode_as_type(crate_path = "subxt::ext::scale_encode")]
pub enum Junctions {
    Here,
    X1([Junction; 1]),
    X2([Junction; 2]),
    X3([Junction; 3]),
    X4([Junction; 4]),
    X5([Junction; 5]),
    X6([Junction; 6]),
    X7([Junction; 7]),
    X8([Junction; 8]),
}

impl Serialize for Junctions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Junctions::Here => map.serialize_entry("Here", &serde_json::Value::Null)?,
            Junctions::X1(arr) => map.serialize_entry("X1", &arr.as_slice())?,
            Junctions::X2(arr) => map.serialize_entry("X2", &arr.as_slice())?,
            Junctions::X3(arr) => map.serialize_entry("X3", &arr.as_slice())?,
            Junctions::X4(arr) => map.serialize_entry("X4", &arr.as_slice())?,
            Junctions::X5(arr) => map.serialize_entry("X5", &arr.as_slice())?,
            Junctions::X6(arr) => map.serialize_entry("X6", &arr.as_slice())?,
            Junctions::X7(arr) => map.serialize_entry("X7", &arr.as_slice())?,
            Junctions::X8(arr) => map.serialize_entry("X8", &arr.as_slice())?,
        }
        map.end()
    }
}

/// XCM v4 Junction enum - individual path components.
#[derive(
    Debug,
    Clone,
    Decode,
    Encode,
    subxt::ext::scale_decode::DecodeAsType,
    subxt::ext::scale_encode::EncodeAsType,
)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
#[encode_as_type(crate_path = "subxt::ext::scale_encode")]
pub enum Junction {
    Parachain(#[codec(compact)] u32),
    AccountId32 {
        network: Option<NetworkId>,
        id: [u8; 32],
    },
    AccountIndex64 {
        network: Option<NetworkId>,
        #[codec(compact)]
        index: u64,
    },
    AccountKey20 {
        network: Option<NetworkId>,
        key: [u8; 20],
    },
    PalletInstance(u8),
    GeneralIndex(#[codec(compact)] u128),
    GeneralKey {
        length: u8,
        data: [u8; 32],
    },
    OnlyChild,
    Plurality {
        id: BodyId,
        part: BodyPart,
    },
    GlobalConsensus(NetworkId),
}

impl Serialize for Junction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Junction::Parachain(id) => {
                map.serialize_entry("Parachain", &format_number_with_commas(*id as u128))?
            }
            Junction::AccountId32 { network, id } => {
                let inner = AccountId32Inner { network, id };
                map.serialize_entry("AccountId32", &inner)?
            }
            Junction::AccountIndex64 { network, index } => {
                let inner = AccountIndex64Inner {
                    network,
                    index: *index,
                };
                map.serialize_entry("AccountIndex64", &inner)?
            }
            Junction::AccountKey20 { network, key } => {
                let inner = AccountKey20Inner { network, key };
                map.serialize_entry("AccountKey20", &inner)?
            }
            Junction::PalletInstance(idx) => {
                map.serialize_entry("PalletInstance", &idx.to_string())?
            }
            Junction::GeneralIndex(idx) => {
                map.serialize_entry("GeneralIndex", &format_number_with_commas(*idx))?
            }
            Junction::GeneralKey { length, data } => {
                let inner = GeneralKeyInner {
                    length: *length,
                    data,
                };
                map.serialize_entry("GeneralKey", &inner)?
            }
            Junction::OnlyChild => {
                map.serialize_entry("OnlyChild", &serde_json::Value::Null)?
            }
            Junction::Plurality { id, part } => {
                let inner = PluralityInner { id, part };
                map.serialize_entry("Plurality", &inner)?
            }
            Junction::GlobalConsensus(network) => {
                map.serialize_entry("GlobalConsensus", network)?
            }
        }
        map.end()
    }
}

// Helper structs for Junction serialization
struct AccountId32Inner<'a> {
    network: &'a Option<NetworkId>,
    id: &'a [u8; 32],
}

impl Serialize for AccountId32Inner<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("network", self.network)?;
        map.serialize_entry("id", &format!("0x{}", hex::encode(self.id)))?;
        map.end()
    }
}

struct AccountIndex64Inner<'a> {
    network: &'a Option<NetworkId>,
    index: u64,
}

impl Serialize for AccountIndex64Inner<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("network", self.network)?;
        map.serialize_entry("index", &format_number_with_commas(self.index as u128))?;
        map.end()
    }
}

struct AccountKey20Inner<'a> {
    network: &'a Option<NetworkId>,
    key: &'a [u8; 20],
}

impl Serialize for AccountKey20Inner<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("network", self.network)?;
        map.serialize_entry("key", &format!("0x{}", hex::encode(self.key)))?;
        map.end()
    }
}

struct GeneralKeyInner<'a> {
    length: u8,
    data: &'a [u8; 32],
}

impl Serialize for GeneralKeyInner<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("length", &self.length.to_string())?;
        // Output full 32 bytes to match Sidecar format
        map.serialize_entry("data", &format!("0x{}", hex::encode(self.data)))?;
        map.end()
    }
}

#[derive(Serialize)]
struct PluralityInner<'a> {
    id: &'a BodyId,
    part: &'a BodyPart,
}

/// XCM v4 NetworkId enum.
#[derive(
    Debug,
    Clone,
    Decode,
    Encode,
    subxt::ext::scale_decode::DecodeAsType,
    subxt::ext::scale_encode::EncodeAsType,
)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
#[encode_as_type(crate_path = "subxt::ext::scale_encode")]
pub enum NetworkId {
    ByGenesis([u8; 32]),
    ByFork {
        block_number: u64,
        block_hash: [u8; 32],
    },
    Polkadot,
    Kusama,
    Westend,
    Rococo,
    Wococo,
    Ethereum {
        #[codec(compact)]
        chain_id: u64,
    },
    BitcoinCore,
    BitcoinCash,
    PolkadotBulletin,
}

impl Serialize for NetworkId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            NetworkId::ByGenesis(hash) => {
                map.serialize_entry("ByGenesis", &format!("0x{}", hex::encode(hash)))?;
            }
            NetworkId::ByFork {
                block_number,
                block_hash,
            } => {
                let inner = ByForkInner {
                    block_number: *block_number,
                    block_hash,
                };
                map.serialize_entry("ByFork", &inner)?;
            }
            NetworkId::Polkadot => {
                map.serialize_entry("Polkadot", &serde_json::Value::Null)?;
            }
            NetworkId::Kusama => {
                map.serialize_entry("Kusama", &serde_json::Value::Null)?;
            }
            NetworkId::Westend => {
                map.serialize_entry("Westend", &serde_json::Value::Null)?;
            }
            NetworkId::Rococo => {
                map.serialize_entry("Rococo", &serde_json::Value::Null)?;
            }
            NetworkId::Wococo => {
                map.serialize_entry("Wococo", &serde_json::Value::Null)?;
            }
            NetworkId::Ethereum { chain_id } => {
                let inner = EthereumInner {
                    chain_id: *chain_id,
                };
                map.serialize_entry("Ethereum", &inner)?;
            }
            NetworkId::BitcoinCore => {
                map.serialize_entry("BitcoinCore", &serde_json::Value::Null)?;
            }
            NetworkId::BitcoinCash => {
                map.serialize_entry("BitcoinCash", &serde_json::Value::Null)?;
            }
            NetworkId::PolkadotBulletin => {
                map.serialize_entry("PolkadotBulletin", &serde_json::Value::Null)?;
            }
        }
        map.end()
    }
}

struct ByForkInner<'a> {
    block_number: u64,
    block_hash: &'a [u8; 32],
}

impl Serialize for ByForkInner<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry(
            "blockNumber",
            &format_number_with_commas(self.block_number as u128),
        )?;
        map.serialize_entry("blockHash", &format!("0x{}", hex::encode(self.block_hash)))?;
        map.end()
    }
}

struct EthereumInner {
    chain_id: u64,
}

impl Serialize for EthereumInner {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("chainId", &self.chain_id.to_string())?;
        map.end()
    }
}

/// XCM v3 BodyId enum (used by Plurality junction).
#[derive(
    Debug,
    Clone,
    Decode,
    Encode,
    subxt::ext::scale_decode::DecodeAsType,
    subxt::ext::scale_encode::EncodeAsType,
)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
#[encode_as_type(crate_path = "subxt::ext::scale_encode")]
pub enum BodyId {
    Unit,
    Moniker([u8; 4]),
    Index(#[codec(compact)] u32),
    Executive,
    Technical,
    Legislative,
    Judicial,
    Defense,
    Administration,
    Treasury,
}

impl Serialize for BodyId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            BodyId::Unit => map.serialize_entry("Unit", &serde_json::Value::Null)?,
            BodyId::Moniker(data) => {
                map.serialize_entry("Moniker", &format!("0x{}", hex::encode(data)))?
            }
            BodyId::Index(idx) => {
                map.serialize_entry("Index", &format_number_with_commas(*idx as u128))?
            }
            BodyId::Executive => map.serialize_entry("Executive", &serde_json::Value::Null)?,
            BodyId::Technical => map.serialize_entry("Technical", &serde_json::Value::Null)?,
            BodyId::Legislative => map.serialize_entry("Legislative", &serde_json::Value::Null)?,
            BodyId::Judicial => map.serialize_entry("Judicial", &serde_json::Value::Null)?,
            BodyId::Defense => map.serialize_entry("Defense", &serde_json::Value::Null)?,
            BodyId::Administration => {
                map.serialize_entry("Administration", &serde_json::Value::Null)?
            }
            BodyId::Treasury => map.serialize_entry("Treasury", &serde_json::Value::Null)?,
        }
        map.end()
    }
}

/// XCM v3 BodyPart enum (used by Plurality junction).
#[derive(
    Debug,
    Clone,
    Decode,
    Encode,
    subxt::ext::scale_decode::DecodeAsType,
    subxt::ext::scale_encode::EncodeAsType,
)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
#[encode_as_type(crate_path = "subxt::ext::scale_encode")]
pub enum BodyPart {
    Voice,
    Members {
        #[codec(compact)]
        count: u32,
    },
    Fraction {
        #[codec(compact)]
        nom: u32,
        #[codec(compact)]
        denom: u32,
    },
    AtLeastProportion {
        #[codec(compact)]
        nom: u32,
        #[codec(compact)]
        denom: u32,
    },
    MoreThanProportion {
        #[codec(compact)]
        nom: u32,
        #[codec(compact)]
        denom: u32,
    },
}

impl Serialize for BodyPart {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            BodyPart::Voice => map.serialize_entry("Voice", &serde_json::Value::Null)?,
            BodyPart::Members { count } => {
                #[derive(Serialize)]
                struct Inner {
                    count: String,
                }
                map.serialize_entry(
                    "Members",
                    &Inner {
                        count: format_number_with_commas(*count as u128),
                    },
                )?
            }
            BodyPart::Fraction { nom, denom } => {
                #[derive(Serialize)]
                struct Inner {
                    nom: String,
                    denom: String,
                }
                map.serialize_entry(
                    "Fraction",
                    &Inner {
                        nom: format_number_with_commas(*nom as u128),
                        denom: format_number_with_commas(*denom as u128),
                    },
                )?
            }
            BodyPart::AtLeastProportion { nom, denom } => {
                #[derive(Serialize)]
                struct Inner {
                    nom: String,
                    denom: String,
                }
                map.serialize_entry(
                    "AtLeastProportion",
                    &Inner {
                        nom: format_number_with_commas(*nom as u128),
                        denom: format_number_with_commas(*denom as u128),
                    },
                )?
            }
            BodyPart::MoreThanProportion { nom, denom } => {
                #[derive(Serialize)]
                struct Inner {
                    nom: String,
                    denom: String,
                }
                map.serialize_entry(
                    "MoreThanProportion",
                    &Inner {
                        nom: format_number_with_commas(*nom as u128),
                        denom: format_number_with_commas(*denom as u128),
                    },
                )?
            }
        }
        map.end()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Format a number with comma separators (e.g., 2001 -> "2,001").
/// Matches Sidecar's formatting for parachain IDs and other numbers.
pub fn format_number_with_commas(n: u128) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Blake2_128Concat hasher prefix length (16 bytes hash + original key bytes).
pub const BLAKE2_128_HASH_LEN: usize = 16;

/// Decode MultiLocation from SCALE-encoded bytes.
///
/// The bytes from Subxt's key().part(0).bytes() include the Blake2_128Concat hash prefix
/// (16 bytes) followed by the actual SCALE-encoded Location. We skip the hash prefix
/// to get the raw Location bytes for decoding.
///
/// Uses typed Location struct with DecodeAsType for efficient decoding.
/// The custom Serialize implementations produce Sidecar-compatible JSON format
/// (PascalCase variants, comma-formatted numbers).
pub fn decode_multi_location_from_bytes(key_part_bytes: &[u8]) -> serde_json::Value {
    // Skip the Blake2_128Concat hash prefix (16 bytes) to get the actual Location bytes
    if key_part_bytes.len() <= BLAKE2_128_HASH_LEN {
        return serde_json::json!({"raw": format!("0x{}", hex::encode(key_part_bytes))});
    }

    let multi_location_bytes = &key_part_bytes[BLAKE2_128_HASH_LEN..];

    if multi_location_bytes.is_empty() {
        return serde_json::json!({"raw": "0x"});
    }

    // Decode using SCALE codec directly into our typed Location struct
    match Location::decode(&mut &multi_location_bytes[..]) {
        Ok(location) => {
            // Serialize using custom Serialize impl that produces Sidecar-compatible JSON
            serde_json::to_value(&location).unwrap_or_else(|e| {
                tracing::warn!("Failed to serialize Location: {:?}", e);
                serde_json::json!({"error": format!("Failed to serialize Location: {}", e)})
            })
        }
        Err(e) => {
            tracing::warn!(
                "Failed to decode MultiLocation: {:?}, bytes: 0x{}",
                e,
                hex::encode(multi_location_bytes)
            );
            serde_json::json!({"error": format!("Failed to decode MultiLocation: {}", e)})
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_multi_location_simple() {
        // Location { parents: 0, interior: Here } encodes to: [0, 0]
        let mut key_part_bytes = vec![0u8; BLAKE2_128_HASH_LEN]; // fake hash prefix
        key_part_bytes.extend_from_slice(&[
            0, // parents = 0
            0, // interior = Here variant
        ]);

        let result = decode_multi_location_from_bytes(&key_part_bytes);

        assert!(
            result.get("raw").is_none() && result.get("error").is_none(),
            "Should not fall back to raw hex or error: {}",
            result
        );
        assert_eq!(result["parents"], "0");
        assert!(result["interior"].get("Here").is_some());
    }

    #[test]
    fn test_decode_multi_location_with_parachain() {
        // Location { parents: 1, interior: X1([Parachain(1000)]) }
        let mut key_part_bytes = vec![0u8; BLAKE2_128_HASH_LEN]; // fake hash prefix
        key_part_bytes.extend_from_slice(&[
            1, // parents = 1
            1, // X1 variant
            0, // Parachain variant
            0xa1, 0x0f, // compact encoded 1000
        ]);

        let result = decode_multi_location_from_bytes(&key_part_bytes);

        assert!(
            result.get("raw").is_none() && result.get("error").is_none(),
            "Should not fall back to raw hex or error: {}",
            result
        );
        assert_eq!(result["parents"], "1");
    }

    #[test]
    fn test_decode_multi_location_invalid() {
        let short_bytes = vec![0u8; BLAKE2_128_HASH_LEN]; // only hash, no location
        let result = decode_multi_location_from_bytes(&short_bytes);
        assert!(result.get("raw").is_some() || result.get("error").is_some());
    }

    #[test]
    fn test_decode_multi_location_empty() {
        let empty_bytes: Vec<u8> = vec![];
        let result = decode_multi_location_from_bytes(&empty_bytes);
        assert!(result.get("raw").is_some());
    }

    #[test]
    fn test_decode_multi_location_only_hash() {
        let only_hash = vec![0u8; BLAKE2_128_HASH_LEN];
        let result = decode_multi_location_from_bytes(&only_hash);
        assert!(result.get("raw").is_some());
    }

    #[test]
    fn test_format_number_with_commas() {
        assert_eq!(format_number_with_commas(0), "0");
        assert_eq!(format_number_with_commas(999), "999");
        assert_eq!(format_number_with_commas(1000), "1,000");
        assert_eq!(format_number_with_commas(1000000), "1,000,000");
        assert_eq!(format_number_with_commas(2001), "2,001");
    }
}
