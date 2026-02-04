//! Handler for /pallets/foreign-assets endpoint.
//!
//! Returns information about all foreign assets on Asset Hub chains.
//! Foreign assets are cross-chain assets identified by XCM MultiLocation.

use crate::handlers::pallets::common::{AtResponse, PalletError, format_account_id};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use futures::StreamExt;
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignAssetsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub use_rc_block: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignAssetItem {
    /// The XCM MultiLocation identifier for this foreign asset (as JSON or hex string)
    pub multi_location: serde_json::Value,
    /// Asset details (owner, supply, etc.) - always present, empty object if not found
    pub foreign_asset_info: serde_json::Value,
    /// Asset metadata (name, symbol, decimals) - always present, empty object if not found
    pub foreign_asset_metadata: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsForeignAssetsResponse {
    pub at: AtResponse,
    pub items: Vec<ForeignAssetItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// ============================================================================
// Internal SCALE Decode Types
// ============================================================================

#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
enum AssetStatus {
    Live,
    Frozen,
    Destroying,
}

impl AssetStatus {
    fn as_str(&self) -> &'static str {
        match self {
            AssetStatus::Live => "Live",
            AssetStatus::Frozen => "Frozen",
            AssetStatus::Destroying => "Destroying",
        }
    }
}

#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
struct AssetDetails {
    owner: [u8; 32],
    issuer: [u8; 32],
    admin: [u8; 32],
    freezer: [u8; 32],
    supply: u128,
    deposit: u128,
    min_balance: u128,
    is_sufficient: bool,
    accounts: u32,
    sufficients: u32,
    approvals: u32,
    status: AssetStatus,
}

#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
struct AssetMetadataStorage {
    deposit: u128,
    name: Vec<u8>,
    symbol: Vec<u8>,
    decimals: u8,
    is_frozen: bool,
}

// ============================================================================
// XCM Location Types (DecodeAsType for typed decoding)
// ============================================================================

/// XCM v4 Location type for foreign asset keys.
/// Uses DecodeAsType for efficient typed decoding from Subxt.
/// Custom Serialize produces Sidecar-compatible JSON format.
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
struct Location {
    parents: u8,
    interior: Junctions,
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
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
enum Junctions {
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
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
enum Junction {
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
            Junction::OnlyChild => map.serialize_entry("OnlyChild", &serde_json::Value::Null)?,
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
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
enum NetworkId {
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
        match self {
            NetworkId::ByGenesis(hash) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("ByGenesis", &format!("0x{}", hex::encode(hash)))?;
                map.end()
            }
            NetworkId::ByFork {
                block_number,
                block_hash,
            } => {
                let mut map = serializer.serialize_map(Some(1))?;
                let inner = ByForkInner {
                    block_number: *block_number,
                    block_hash,
                };
                map.serialize_entry("ByFork", &inner)?;
                map.end()
            }
            NetworkId::Polkadot => serializer.serialize_str("Polkadot"),
            NetworkId::Kusama => serializer.serialize_str("Kusama"),
            NetworkId::Westend => serializer.serialize_str("Westend"),
            NetworkId::Rococo => serializer.serialize_str("Rococo"),
            NetworkId::Wococo => serializer.serialize_str("Wococo"),
            NetworkId::Ethereum { chain_id } => {
                let mut map = serializer.serialize_map(Some(1))?;
                let inner = EthereumInner {
                    chain_id: *chain_id,
                };
                map.serialize_entry("Ethereum", &inner)?;
                map.end()
            }
            NetworkId::BitcoinCore => serializer.serialize_str("BitcoinCore"),
            NetworkId::BitcoinCash => serializer.serialize_str("BitcoinCash"),
            NetworkId::PolkadotBulletin => serializer.serialize_str("PolkadotBulletin"),
        }
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
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType, Serialize)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
enum BodyId {
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

/// XCM v3 BodyPart enum (used by Plurality junction).
#[derive(Debug, Clone, Decode, subxt::ext::scale_decode::DecodeAsType, Serialize)]
#[decode_as_type(crate_path = "subxt::ext::scale_decode")]
enum BodyPart {
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

// ============================================================================
// Main Handler
// ============================================================================

/// Handler for GET /pallets/foreign-assets
///
/// Returns all foreign assets with their details and metadata.
/// Foreign assets use XCM MultiLocation as their identifier instead of simple u32 IDs.
pub async fn pallets_foreign_assets(
    State(state): State<AppState>,
    Query(params): Query<ForeignAssetsQueryParams>,
) -> Result<Response, PalletError> {
    // Foreign assets only exist on Asset Hub chains
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::PalletNotAvailable("ForeignAssets"));
    }

    if params.use_rc_block {
        return handle_use_rc_block(state, params).await;
    }

    // Create client at the specified block
    let client_at_block = match params.at {
        None => state.client.at_current_block().await?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<utils::BlockId>()?;
            match block_id {
                utils::BlockId::Hash(hash) => state.client.at_block(hash).await?,
                utils::BlockId::Number(number) => state.client.at_block(number).await?,
            }
        }
    };

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    let ss58_prefix = state.chain_info.ss58_prefix;
    let items = fetch_all_foreign_assets(&client_at_block, ss58_prefix).await?;

    Ok((
        StatusCode::OK,
        Json(PalletsForeignAssetsResponse {
            at,
            items,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

// ============================================================================
// RC Block Handler
// ============================================================================

async fn handle_use_rc_block(
    state: AppState,
    params: ForeignAssetsQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(PalletError::RelayChainNotConfigured);
    }

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_resolved_block = utils::resolve_block_with_rpc(
        state
            .get_relay_chain_rpc_client()
            .expect("relay chain client checked above"),
        state
            .get_relay_chain_rpc()
            .expect("relay chain rpc checked above"),
        Some(rc_block_id),
    )
    .await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    // Return empty array when no AH blocks found (matching Sidecar behavior)
    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(serde_json::json!([]))).into_response());
    }

    let rc_block_number = rc_resolved_block.number.to_string();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let ss58_prefix = state.chain_info.ss58_prefix;

    // Process ALL AH blocks, not just the first one
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = fetch_timestamp(&client_at_block).await;
        let items = fetch_all_foreign_assets(&client_at_block, ss58_prefix).await?;

        results.push(PalletsForeignAssetsResponse {
            at,
            items,
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        });
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Fetches all foreign assets by iterating over ForeignAssets::Asset storage.
/// Returns an error if the pallet doesn't exist or storage iteration fails.
async fn fetch_all_foreign_assets(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<Vec<ForeignAssetItem>, PalletError> {
    let mut items = Vec::new();

    // First, fetch all metadata entries and store them by their key bytes
    // We use the key part bytes directly from Subxt's API
    let mut metadata_map: std::collections::HashMap<Vec<u8>, serde_json::Value> =
        std::collections::HashMap::new();

    let metadata_addr = subxt::dynamic::storage::<(scale_value::Value,), AssetMetadataStorage>(
        "ForeignAssets",
        "Metadata",
    );

    // Try to iterate metadata - if this fails, the pallet might not exist
    match client_at_block.storage().iter(metadata_addr, ()).await {
        Ok(mut metadata_stream) => {
            while let Some(entry_result) = metadata_stream.next().await {
                if let Ok(entry) = entry_result
                    // Use Subxt's key().part(0) to get the MultiLocation key part directly
                    // This avoids manual byte offset calculations
                    && let Ok(key) = entry.key()
                    && let Some(key_part) = key.part(0)
                {
                    let key_part_bytes = key_part.bytes().to_vec();
                    // Use typed decode instead of manual byte decoding
                    if let Ok(metadata) = entry.value().decode() {
                        let metadata_json = serde_json::json!({
                            "deposit": metadata.deposit.to_string(),
                            "name": format!("0x{}", hex::encode(&metadata.name)),
                            "symbol": format!("0x{}", hex::encode(&metadata.symbol)),
                            "decimals": metadata.decimals.to_string(),
                            "isFrozen": metadata.is_frozen,
                        });
                        metadata_map.insert(key_part_bytes, metadata_json);
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to iterate ForeignAssets::Metadata storage: {:?}", e);
            // Continue - metadata might be empty but Asset storage could still work
        }
    }

    tracing::debug!("Fetched {} metadata entries", metadata_map.len());

    // Use dynamic storage iteration to get all foreign assets
    // ForeignAssets::Asset is a map with MultiLocation as key
    let storage_addr =
        subxt::dynamic::storage::<(scale_value::Value,), AssetDetails>("ForeignAssets", "Asset");

    let mut stream = client_at_block
        .storage()
        .iter(storage_addr, ())
        .await
        .map_err(|e| {
            tracing::error!("Failed to iterate ForeignAssets::Asset storage: {:?}", e);
            PalletError::PalletNotAvailable("ForeignAssets")
        })?;

    while let Some(entry_result) = stream.next().await {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("Error reading foreign asset entry: {:?}", e);
                continue;
            }
        };

        // Use Subxt's key().part(0) to extract the MultiLocation key part directly
        // This is cleaner than manual byte offset calculations
        let key = match entry.key() {
            Ok(k) => k,
            Err(e) => {
                tracing::debug!("Failed to decode storage key: {:?}", e);
                continue;
            }
        };
        let key_part = match key.part(0) {
            Some(part) => part,
            None => {
                tracing::debug!("Storage key has no parts, skipping entry");
                continue;
            }
        };

        // Debug: log the key part bytes for first entry
        if items.is_empty() {
            tracing::debug!(
                "First Asset key part (len={}): 0x{}",
                key_part.bytes().len(),
                hex::encode(key_part.bytes())
            );
        }

        // Decode the MultiLocation from the key part bytes
        let multi_location = decode_multi_location_from_bytes(key_part.bytes());

        // Decode the asset details using typed decode
        let foreign_asset_info = match entry.value().decode() {
            Ok(details) => format_asset_details(&details, ss58_prefix),
            Err(e) => {
                tracing::debug!("Failed to decode asset details: {:?}", e);
                serde_json::json!({})
            }
        };

        // Look up metadata using the key part bytes
        let key_part_bytes = key_part.bytes();
        let foreign_asset_metadata =
            metadata_map
                .get(key_part_bytes)
                .cloned()
                .unwrap_or_else(|| {
                    // Return default metadata structure to match Sidecar format
                    serde_json::json!({
                        "deposit": "0",
                        "name": "0x",
                        "symbol": "0x",
                        "decimals": "0",
                        "isFrozen": false,
                    })
                });

        items.push(ForeignAssetItem {
            multi_location,
            foreign_asset_info,
            foreign_asset_metadata,
        });
    }

    Ok(items)
}

/// Format a number with comma separators (e.g., 2001 -> "2,001").
/// Matches Sidecar's formatting for parachain IDs and other numbers.
fn format_number_with_commas(n: u128) -> String {
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
const BLAKE2_128_HASH_LEN: usize = 16;

/// Decode MultiLocation from SCALE-encoded bytes.
///
/// The bytes from Subxt's key().part(0).bytes() include the Blake2_128Concat hash prefix
/// (16 bytes) followed by the actual SCALE-encoded Location. We skip the hash prefix
/// to get the raw Location bytes for decoding.
///
/// Uses typed Location struct with DecodeAsType for efficient decoding.
/// The custom Serialize implementations produce Sidecar-compatible JSON format
/// (PascalCase variants, comma-formatted numbers).
fn decode_multi_location_from_bytes(key_part_bytes: &[u8]) -> serde_json::Value {
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

/// Format asset details into JSON.
fn format_asset_details(details: &AssetDetails, ss58_prefix: u16) -> serde_json::Value {
    serde_json::json!({
        "owner": format_account_id(&details.owner, ss58_prefix),
        "issuer": format_account_id(&details.issuer, ss58_prefix),
        "admin": format_account_id(&details.admin, ss58_prefix),
        "freezer": format_account_id(&details.freezer, ss58_prefix),
        "supply": details.supply.to_string(),
        "deposit": details.deposit.to_string(),
        "minBalance": details.min_balance.to_string(),
        "isSufficient": details.is_sufficient,
        "accounts": details.accounts.to_string(),
        "sufficients": details.sufficients.to_string(),
        "approvals": details.approvals.to_string(),
        "status": details.status.as_str().to_string(),
    })
}

/// Fetches timestamp from Timestamp::Now storage.
async fn fetch_timestamp(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> Option<String> {
    let timestamp_addr = subxt::dynamic::storage::<(), u64>("Timestamp", "Now");
    let timestamp = client_at_block
        .storage()
        .fetch(timestamp_addr, ())
        .await
        .ok()?;
    let timestamp_value = timestamp.decode().ok()?;
    Some(timestamp_value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_foreign_asset_item_serialization() {
        let item = ForeignAssetItem {
            multi_location: serde_json::json!({
                "parents": "2",
                "interior": {
                    "X1": {
                        "GlobalConsensus": "Polkadot"
                    }
                }
            }),
            foreign_asset_info: serde_json::json!({
                "owner": "FxqimVubBRPqJ8kTwb3wL7G4q645hEkBEnXPyttLsTrFc5Q",
                "issuer": "FxqimVubBRPqJ8kTwb3wL7G4q645hEkBEnXPyttLsTrFc5Q",
                "admin": "FxqimVubBRPqJ8kTwb3wL7G4q645hEkBEnXPyttLsTrFc5Q",
                "freezer": "FxqimVubBRPqJ8kTwb3wL7G4q645hEkBEnXPyttLsTrFc5Q",
                "supply": "0",
                "deposit": "0",
                "minBalance": "100000000",
                "isSufficient": true,
                "accounts": "0",
                "sufficients": "0",
                "approvals": "0",
                "status": "Live"
            }),
            foreign_asset_metadata: serde_json::json!({
                "deposit": "0",
                "name": "0x506f6c6b61646f74",
                "symbol": "0x444f54",
                "decimals": "10",
                "isFrozen": false
            }),
        };

        let json = serde_json::to_string(&item).unwrap();

        // Verify camelCase serialization
        assert!(json.contains("\"multiLocation\""));
        assert!(json.contains("\"foreignAssetInfo\""));
        assert!(json.contains("\"foreignAssetMetadata\""));
        assert!(json.contains("\"minBalance\""));
        assert!(json.contains("\"isSufficient\""));
        assert!(json.contains("\"isFrozen\""));

        // Verify no snake_case
        assert!(!json.contains("\"multi_location\""));
        assert!(!json.contains("\"foreign_asset_info\""));
        assert!(!json.contains("\"foreign_asset_metadata\""));
    }

    #[test]
    fn test_foreign_assets_response_serialization() {
        let response = PalletsForeignAssetsResponse {
            at: AtResponse {
                hash: "0x1234567890abcdef".to_string(),
                height: "12345".to_string(),
            },
            items: vec![],
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();

        // Verify structure
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"items\""));
        assert!(json.contains("\"hash\""));
        assert!(json.contains("\"height\""));

        // Verify optional fields are not included when None
        assert!(!json.contains("\"rcBlockHash\""));
        assert!(!json.contains("\"rcBlockNumber\""));
        assert!(!json.contains("\"ahTimestamp\""));
    }

    #[test]
    fn test_foreign_assets_response_with_rc_block() {
        let response = PalletsForeignAssetsResponse {
            at: AtResponse {
                hash: "0x1234567890abcdef".to_string(),
                height: "12345".to_string(),
            },
            items: vec![],
            rc_block_hash: Some("0xabcdef".to_string()),
            rc_block_number: Some("67890".to_string()),
            ah_timestamp: Some("1234567890000".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();

        // Verify RC block fields are included in camelCase
        assert!(json.contains("\"rcBlockHash\""));
        assert!(json.contains("\"rcBlockNumber\""));
        assert!(json.contains("\"ahTimestamp\""));
    }

    #[test]
    fn test_empty_foreign_asset_info() {
        let item = ForeignAssetItem {
            multi_location: serde_json::json!({
                "parents": "0",
                "interior": "Here"
            }),
            foreign_asset_info: serde_json::json!({}),
            foreign_asset_metadata: serde_json::json!({}),
        };

        let json = serde_json::to_string(&item).unwrap();

        // Verify empty objects are serialized correctly
        assert!(json.contains("\"foreignAssetInfo\":{}"));
        assert!(json.contains("\"foreignAssetMetadata\":{}"));
    }

    #[test]
    fn test_format_asset_details() {
        // Test that format_asset_details returns correctly formatted JSON
        let details = AssetDetails {
            owner: [1u8; 32],
            issuer: [2u8; 32],
            admin: [3u8; 32],
            freezer: [4u8; 32],
            supply: 1000,
            deposit: 100,
            min_balance: 1,
            is_sufficient: true,
            accounts: 10,
            sufficients: 5,
            approvals: 2,
            status: AssetStatus::Live,
        };
        let result = format_asset_details(&details, 0);

        // Check that the result has the expected structure
        assert!(result.get("owner").is_some());
        assert!(result.get("supply").is_some());
        assert_eq!(result["supply"], "1000");
        assert_eq!(result["isSufficient"], true);
        assert_eq!(result["status"], "Live");
    }

    #[test]
    fn test_decode_multi_location_simple() {
        // Location { parents: 0, interior: Here } encodes to: [0, 0]
        // The function expects key_part bytes which include a 16-byte Blake2_128 hash prefix
        // followed by the actual SCALE-encoded Location bytes
        let mut key_part_bytes = vec![0u8; BLAKE2_128_HASH_LEN]; // fake hash prefix
        key_part_bytes.extend_from_slice(&[
            0, // parents = 0
            0, // interior = Here variant
        ]);

        let result = decode_multi_location_from_bytes(&key_part_bytes);

        // Should successfully decode
        assert!(
            result.get("raw").is_none() && result.get("error").is_none(),
            "Should not fall back to raw hex or error: {}",
            result
        );
        assert_eq!(result["parents"], "0");
        // interior is serialized as object {"Here": null}
        assert!(result["interior"].get("Here").is_some());
    }

    #[test]
    fn test_decode_multi_location_with_parachain() {
        // Location { parents: 1, interior: X1([Parachain(1000)]) }
        // SCALE encoding:
        // parents = 1
        // interior variant X1 = 1
        // Junction::Parachain variant = 0
        // 1000 as compact u32 = [0xa1, 0x0f] (1000 << 2 = 4000 = 0x0fa0, little endian)
        let mut key_part_bytes = vec![0u8; BLAKE2_128_HASH_LEN]; // fake hash prefix
        key_part_bytes.extend_from_slice(&[
            1, // parents = 1
            1, // X1 variant
            0, // Parachain variant
            0xa1, 0x0f, // compact encoded 1000
        ]);

        let result = decode_multi_location_from_bytes(&key_part_bytes);

        // Should successfully decode
        assert!(
            result.get("raw").is_none() && result.get("error").is_none(),
            "Should not fall back to raw hex or error: {}",
            result
        );
        assert_eq!(result["parents"], "1");
    }

    #[test]
    fn test_decode_multi_location_invalid() {
        // Test with bytes that are too short (just the hash, no location data)
        let short_bytes = vec![0u8; BLAKE2_128_HASH_LEN]; // only hash, no location

        let result = decode_multi_location_from_bytes(&short_bytes);

        // Should fall back to raw hex (bytes too short after skipping hash)
        assert!(result.get("raw").is_some() || result.get("error").is_some());
    }

    #[test]
    fn test_asset_status_as_str() {
        assert_eq!(AssetStatus::Live.as_str(), "Live");
        assert_eq!(AssetStatus::Frozen.as_str(), "Frozen");
        assert_eq!(AssetStatus::Destroying.as_str(), "Destroying");
    }

    #[test]
    fn test_decode_multi_location_empty() {
        // Empty bytes should return raw hex
        let empty_bytes: Vec<u8> = vec![];
        let result = decode_multi_location_from_bytes(&empty_bytes);
        // Should return raw hex representation (bytes too short)
        assert!(result.get("raw").is_some());
    }

    #[test]
    fn test_decode_multi_location_only_hash() {
        // Only hash prefix, no actual location data
        let only_hash = vec![0u8; BLAKE2_128_HASH_LEN];
        let result = decode_multi_location_from_bytes(&only_hash);
        // Should return raw hex (no location data after hash)
        assert!(result.get("raw").is_some());
    }

    #[test]
    fn test_query_params_deserialization() {
        // Test default use_rc_block is false
        let params: ForeignAssetsQueryParams = serde_json::from_str(r#"{"at":"12345"}"#).unwrap();
        assert_eq!(params.at, Some("12345".to_string()));
        assert!(!params.use_rc_block);

        // Test explicit use_rc_block
        let params: ForeignAssetsQueryParams =
            serde_json::from_str(r#"{"at":"12345","useRcBlock":true}"#).unwrap();
        assert!(params.use_rc_block);
    }
}
