use crate::state::AppState;

// Type visitor for extracting type names from extrinsic fields
use super::type_name_visitor::GetTypeName;
use crate::utils::{self, EraInfo};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use heck::{ToLowerCamelCase, ToSnakeCase, ToUpperCamelCase};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sp_core::crypto::{AccountId32, Ss58Codec};
use sp_runtime::traits::BlakeTwo256;
use sp_runtime::traits::Hash as HashT;
use subxt_historic::error::{OnlineClientAtBlockError, StorageEntryIsNotAPlainValue, StorageError};
use thiserror::Error;

// ================================================================================================
// Constants
// ================================================================================================

/// Length of consensus engine ID in digest items (e.g., "BABE", "aura", "pow_")
const CONSENSUS_ENGINE_ID_LEN: usize = 4;

// ================================================================================================
// Query Parameters
// ================================================================================================

/// Query parameters for /blocks/{blockId} endpoint
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BlockQueryParams {
    /// When true, include documentation for events
    #[serde(default)]
    pub event_docs: bool,
    /// When true, include documentation for extrinsics
    #[serde(default)]
    pub extrinsic_docs: bool,
}

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum GetBlockError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Failed to get block header")]
    HeaderFetchFailed(#[source] subxt_rpcs::Error),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[from] OnlineClientAtBlockError),

    #[error("Failed to fetch chain storage")]
    StorageFetchFailed(#[from] StorageError),

    #[error("Storage entry is not a plain value")]
    StorageNotPlainValue(#[from] StorageEntryIsNotAPlainValue),

    #[error("Failed to decode storage value")]
    StorageDecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Failed to fetch extrinsics")]
    ExtrinsicsFetchFailed(String),

    #[error("Missing signature bytes for signed extrinsic")]
    MissingSignatureBytes,

    #[error("Missing address bytes for signed extrinsic")]
    MissingAddressBytes,

    #[error("Failed to decode extrinsic field: {0}")]
    ExtrinsicDecodeFailed(String),

    #[error("Failed to get finalized head")]
    FinalizedHeadFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get canonical block hash")]
    CanonicalHashFailed(#[source] subxt_rpcs::Error),
}

impl IntoResponse for GetBlockError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetBlockError::InvalidBlockParam(_) | GetBlockError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetBlockError::HeaderFetchFailed(_)
            | GetBlockError::HeaderFieldMissing(_)
            | GetBlockError::ClientAtBlockFailed(_)
            | GetBlockError::StorageFetchFailed(_)
            | GetBlockError::StorageNotPlainValue(_)
            | GetBlockError::StorageDecodeFailed(_)
            | GetBlockError::ExtrinsicsFetchFailed(_)
            | GetBlockError::MissingSignatureBytes
            | GetBlockError::MissingAddressBytes
            | GetBlockError::ExtrinsicDecodeFailed(_)
            | GetBlockError::FinalizedHeadFailed(_)
            | GetBlockError::CanonicalHashFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

// ================================================================================================
// Enums
// ================================================================================================

/// SCALE encoding discriminants for the DigestItem enum from sp_runtime::generic
///
/// These discriminants match the SCALE encoding of substrate's DigestItem enum.
/// Reference: sp_runtime::generic::DigestItem
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum DigestItemDiscriminant {
    /// ChangesTrieRoot has been removed but was 2
    /// ChangesTrieSignal has been removed but was 3
    Other = 0,
    Consensus = 4,
    Seal = 5,
    PreRuntime = 6,
    RuntimeEnvironmentUpdated = 8,
}

impl TryFrom<u8> for DigestItemDiscriminant {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Other),
            4 => Ok(Self::Consensus),
            5 => Ok(Self::Seal),
            6 => Ok(Self::PreRuntime),
            8 => Ok(Self::RuntimeEnvironmentUpdated),
            _ => Err(()),
        }
    }
}

impl DigestItemDiscriminant {
    /// Convert discriminant to string representation for JSON serialization
    fn as_str(&self) -> &'static str {
        match self {
            Self::Other => "Other",
            Self::Consensus => "Consensus",
            Self::Seal => "Seal",
            Self::PreRuntime => "PreRuntime",
            Self::RuntimeEnvironmentUpdated => "RuntimeEnvironmentUpdated",
        }
    }
}

/// MultiAddress type for decoding Substrate address variants
/// This represents the different ways an address can be encoded in Substrate
#[derive(scale_decode::DecodeAsType)]
#[allow(dead_code)]
enum MultiAddress {
    /// An AccountId32 (32 bytes)
    Id([u8; 32]),
    /// An account index
    Index(u32),
    /// Raw bytes
    Raw(Vec<u8>),
    /// A 32-byte address
    Address32([u8; 32]),
    /// A 20-byte address (Ethereum-style)
    Address20([u8; 20]),
}

/// Event phase - when during block execution the event was emitted
#[derive(Debug, Clone)]
enum EventPhase {
    /// During block initialization
    Initialization,
    /// During extrinsic application (contains extrinsic index)
    ApplyExtrinsic(u32),
    /// During block finalization
    Finalization,
}

// ================================================================================================
// Structs
// ================================================================================================

/// Represents a decoded digest log entry
#[derive(Debug, Serialize)]
pub struct DigestLog {
    #[serde(rename = "type")]
    pub log_type: String,
    pub index: String,
    pub value: Value,
}

/// Method information for extrinsic calls
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MethodInfo {
    pub pallet: String,
    pub method: String,
}

/// Event information in block response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub method: MethodInfo,
    pub data: Vec<Value>,
    /// Documentation for this event (only present when eventDocs=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}

/// Events that occurred during block initialization
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnInitialize {
    pub events: Vec<Event>,
}

/// Events that occurred during block finalization
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnFinalize {
    pub events: Vec<Event>,
}

/// Signer ID wrapper matching sidecar format
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignerId {
    pub id: String,
}

/// Signature information for signed extrinsics
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInfo {
    pub signature: String,
    pub signer: SignerId,
}

/// Extrinsic information matching sidecar format
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtrinsicInfo {
    pub method: MethodInfo,
    /// Signature information - null for unsigned extrinsics (inherents)
    pub signature: Option<SignatureInfo>,
    /// Nonce - shown as null when extraction fails (matching sidecar behavior)
    pub nonce: Option<String>,
    /// Args as a JSON object where bytes are hex-encoded and large numbers are strings
    pub args: serde_json::Map<String, Value>,
    /// Tip - shown as null when extraction fails (matching sidecar behavior)
    pub tip: Option<String>,
    pub hash: String,
    /// Runtime dispatch info containing weight, class, and partialFee for signed extrinsics
    pub info: serde_json::Map<String, Value>,
    /// Transaction era/mortality information
    pub era: EraInfo,
    /// Events emitted by this extrinsic
    pub events: Vec<Event>,
    /// Whether the extrinsic executed successfully (determined from System.ExtrinsicSuccess event)
    pub success: bool,
    /// Whether the extrinsic pays a fee (None for unsigned, Some(bool) for signed)
    /// Extracted from DispatchInfo in System.ExtrinsicSuccess/ExtrinsicFailed events
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pays_fee: Option<bool>,
    /// Documentation for this extrinsic (only present when extrinsicDocs=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// Raw extrinsic bytes as hex (used internally for fee queries, not serialized)
    #[serde(skip)]
    pub raw_hex: String,
}

/// Basic block information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockResponse {
    pub number: String,
    pub hash: String,
    pub parent_hash: String,
    pub state_root: String,
    pub extrinsics_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    pub logs: Vec<DigestLog>,
    pub on_initialize: OnInitialize,
    pub extrinsics: Vec<ExtrinsicInfo>,
    pub on_finalize: OnFinalize,
    /// Whether this block has been finalized
    pub finalized: bool,
}

/// A parsed event with its phase information
#[derive(Debug)]
struct ParsedEvent {
    /// When in the block this event occurred
    phase: EventPhase,
    /// Event pallet name
    pallet_name: String,
    /// Event variant name
    event_name: String,
    /// Event data as JSON
    event_data: Vec<Value>,
}

/// Weight information extracted from DispatchInfo in events
/// Can be either a single value (legacy) or ref_time + proof_size (modern)
#[derive(Debug, Default, Clone)]
pub struct ActualWeight {
    /// The ref_time component (or the single weight value for legacy format)
    pub ref_time: Option<String>,
    /// The proof_size component (None for legacy weight format)
    pub proof_size: Option<String>,
}

/// Outcome information for an extrinsic (success and paysFee)
/// Extracted from System.ExtrinsicSuccess or System.ExtrinsicFailed events
#[derive(Debug, Default, Clone)]
struct ExtrinsicOutcome {
    /// Whether the extrinsic succeeded (true if ExtrinsicSuccess event found)
    success: bool,
    /// Whether the extrinsic pays a fee (extracted from DispatchInfo)
    /// None means we couldn't determine it from events
    pays_fee: Option<bool>,
    /// Actual weight used during extrinsic execution (from DispatchInfo)
    /// This is needed for accurate fee calculation with calc_partial_fee
    actual_weight: Option<ActualWeight>,
    /// Dispatch class (Normal, Operational, or Mandatory)
    class: Option<String>,
}

// ================================================================================================
// Helper Functions - Conversion & Formatting
// ================================================================================================

/// Format bytes as hex string with "0x" prefix
fn hex_with_prefix(data: &[u8]) -> String {
    format!("0x{}", hex::encode(data))
}

/// Convert to lowerCamelCase by only lowercasing the first character
/// This preserves snake_case names (e.g., "inbound_messages_data" stays unchanged)
/// while converting PascalCase to lowerCamelCase (e.g., "PreRuntime" → "preRuntime")
/// Used for SCALE enum variant names which should preserve their original casing
fn lowercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().chain(chars).collect(),
    }
}

/// Extract a numeric value from a JSON value as a string
/// Handles direct numbers, nested objects, or string representations
///
/// Returns None if the value cannot be extracted, which will serialize as null
/// in the JSON response (matching sidecar's behavior for missing/unextractable values)
fn extract_numeric_string(value: &Value) -> Option<String> {
    match value {
        // Direct number
        Value::Number(n) => Some(n.to_string()),
        // Direct string
        Value::String(s) => {
            // Remove parentheses if present: "(23)" -> "23"
            // This was present with Nonce values
            Some(s.trim_matches(|c| c == '(' || c == ')').to_string())
        }
        // Object - might be {"primitive": 23} or similar
        Value::Object(map) => {
            // Try to find a numeric field
            if let Some(val) = map.get("primitive") {
                return extract_numeric_string(val);
            }
            // Try other common field names
            for key in ["value", "0"] {
                if let Some(val) = map.get(key) {
                    return extract_numeric_string(val);
                }
            }
            // Could not find expected numeric field
            tracing::warn!(
                "Could not extract numeric value from object with keys: {:?}",
                map.keys().collect::<Vec<_>>()
            );
            None
        }
        // Array - take first element
        Value::Array(arr) => {
            if let Some(first) = arr.first() {
                extract_numeric_string(first)
            } else {
                tracing::warn!("Cannot extract numeric value from empty array");
                None
            }
        }
        _ => {
            tracing::warn!("Unexpected JSON type for numeric extraction: {:?}", value);
            None
        }
    }
}

/// Decode account address bytes to SS58 format
/// Tries to decode:
/// 1. MultiAddress::Id variant (0x00 + 32 bytes)
/// 2. Raw 32-byte AccountId32 (0x + 32 bytes)
fn decode_address_to_ss58(hex_str: &str, ss58_prefix: u16) -> Option<String> {
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
    let account_id = match sp_core::crypto::AccountId32::try_from(account_bytes.as_slice()) {
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

/// Convert JSON value, replacing byte arrays with hex strings and all numbers with strings recursively
///
/// This matches substrate-api-sidecar's behavior of returning all numeric values as strings
/// for consistency across the API.
fn convert_bytes_to_hex(value: Value) -> Value {
    match value {
        Value::Number(n) => {
            // Convert all numbers to strings to match substrate-api-sidecar behavior
            Value::String(n.to_string())
        }
        Value::Array(arr) => {
            // Check if this is a byte array (non-empty and all elements are numbers 0-255)
            // We must check !arr.is_empty() to avoid converting empty arrays to "0x"
            let is_byte_array = !arr.is_empty()
                && arr.iter().all(|v| match v {
                    Value::Number(n) => n.as_u64().is_some_and(|val| val <= 255),
                    _ => false,
                });

            if is_byte_array {
                // Convert to hex string
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                Value::String(format!("0x{}", hex::encode(&bytes)))
            } else {
                // Recurse into array elements
                let converted: Vec<Value> = arr.into_iter().map(convert_bytes_to_hex).collect();

                // If array has single element, unwrap it (this handles cases like ["0x..."] -> "0x...")
                // This is specific to how the data is formatted in substrate-api-sidecar
                match converted.len() {
                    1 => converted.into_iter().next().unwrap(),
                    _ => Value::Array(converted),
                }
            }
        }
        Value::Object(mut map) => {
            // Check if this is a bitvec object (scale-value represents bitvecs specially)
            if let Some(Value::Array(bits)) = map.get("__bitvec__values__") {
                // Convert boolean array to bytes, then to hex
                // BitVec uses LSB0 ordering (least significant bit first within each byte)
                let mut bytes = Vec::new();
                let mut current_byte = 0u8;

                for (i, bit) in bits.iter().enumerate() {
                    if let Some(true) = bit.as_bool() {
                        current_byte |= 1 << (i % 8);
                    }

                    // Every 8 bits, push the byte and reset
                    if (i + 1) % 8 == 0 {
                        bytes.push(current_byte);
                        current_byte = 0;
                    }
                }

                // Push any remaining bits
                if bits.len() % 8 != 0 {
                    bytes.push(current_byte);
                }

                return Value::String(format!("0x{}", hex::encode(&bytes)));
            }

            // Recurse into object values
            for (_, v) in map.iter_mut() {
                *v = convert_bytes_to_hex(v.clone());
            }
            Value::Object(map)
        }
        other => other,
    }
}

// ================================================================================================
// Helper Functions - Digest & Header Processing
// ================================================================================================

/// Decode a consensus digest item (PreRuntime, Consensus, or Seal)
/// The data here is SCALE-encoded as: (ConsensusEngineId, Vec<u8>)
/// where ConsensusEngineId is 4 raw bytes, and Vec<u8> is compact_length + bytes
fn decode_consensus_digest(data: &[u8]) -> Option<Value> {
    use parity_scale_codec::Decode;

    // First 4 bytes are the consensus engine ID (not length-prefixed)
    if data.len() < CONSENSUS_ENGINE_ID_LEN {
        return None;
    }

    let engine_id = hex_with_prefix(&data[0..CONSENSUS_ENGINE_ID_LEN]);

    // The rest is a SCALE-encoded Vec<u8> (compact length + payload bytes)
    let mut remaining = &data[CONSENSUS_ENGINE_ID_LEN..];
    let payload_bytes = Vec::<u8>::decode(&mut remaining).ok()?;
    let payload = hex_with_prefix(&payload_bytes);

    Some(json!([engine_id, payload]))
}

/// Decode digest logs from hex-encoded strings in the JSON response
/// Each hex string is a SCALE-encoded DigestItem
fn decode_digest_logs(header_json: &Value) -> Vec<DigestLog> {
    let logs = match header_json
        .get("digest")
        .and_then(|d| d.get("logs"))
        .and_then(|l| l.as_array())
    {
        Some(logs) => logs,
        None => return Vec::new(),
    };

    logs.iter()
        .filter_map(|log_hex| {
            let hex_str = log_hex.as_str()?;
            let hex_data = hex_str.strip_prefix("0x")?;
            let bytes = hex::decode(hex_data).ok()?;

            if bytes.is_empty() {
                return None;
            }

            // The first byte is the digest item type discriminant
            let discriminant_byte = bytes[0];
            let data = &bytes[1..];

            // Try to parse the discriminant into a known type
            let discriminant = DigestItemDiscriminant::try_from(discriminant_byte)
                .unwrap_or(DigestItemDiscriminant::Other);

            let (log_type, value) = match discriminant {
                // Consensus-related digests: PreRuntime, Consensus, Seal
                // All have format: [consensus_engine_id (4 bytes), payload_data]
                DigestItemDiscriminant::PreRuntime
                | DigestItemDiscriminant::Consensus
                | DigestItemDiscriminant::Seal => match decode_consensus_digest(data) {
                    Some(val) => (discriminant.as_str().to_string(), val),
                    None => ("Other".to_string(), json!(hex_with_prefix(&bytes))),
                },
                // RuntimeEnvironmentUpdated has no associated data
                DigestItemDiscriminant::RuntimeEnvironmentUpdated => {
                    (discriminant.as_str().to_string(), Value::Null)
                }
                // Other (includes unknown discriminants that were converted to Other)
                DigestItemDiscriminant::Other => (
                    discriminant.as_str().to_string(),
                    json!(hex_with_prefix(data)),
                ),
            };

            Some(DigestLog {
                log_type,
                index: discriminant_byte.to_string(),
                value,
            })
        })
        .collect()
}

/// Fetch the canonical block hash at a given block number
/// This is used to verify that a queried block hash is on the canonical chain
async fn get_canonical_hash_at_number(
    state: &AppState,
    block_number: u64,
) -> Result<Option<String>, GetBlockError> {
    let hash = state
        .legacy_rpc
        .chain_get_block_hash(Some(block_number.into()))
        .await
        .map_err(GetBlockError::CanonicalHashFailed)?;

    Ok(hash.map(|h| format!("0x{}", hex::encode(h.0))))
}

/// Fetch the finalized block number from the chain
async fn get_finalized_block_number(state: &AppState) -> Result<u64, GetBlockError> {
    let finalized_hash = state
        .legacy_rpc
        .chain_get_finalized_head()
        .await
        .map_err(GetBlockError::FinalizedHeadFailed)?;
    let finalized_hash_str = format!("0x{}", hex::encode(finalized_hash.0));
    let header_json = state
        .get_header_json(&finalized_hash_str)
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;
    let number_hex = header_json
        .get("number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("number".to_string()))?;
    let number = u64::from_str_radix(number_hex.trim_start_matches("0x"), 16)
        .map_err(|_| GetBlockError::HeaderFieldMissing("number (invalid format)".to_string()))?;

    Ok(number)
}

/// Fetch validator set from chain state at a specific block
async fn get_validators_at_block(
    state: &AppState,
    block_number: u64,
) -> Result<Vec<AccountId32>, GetBlockError> {
    use parity_scale_codec::Decode;

    let client_at_block = state.client.at(block_number).await?;
    let storage_entry = client_at_block.storage().entry("Session", "Validators")?;
    let validators_value = storage_entry.fetch(()).await?.ok_or_else(|| {
        // Use the parity_scale_codec::Error for missing validators which will be converted to StorageDecodeFailed
        parity_scale_codec::Error::from("validators storage not found")
    })?;
    let raw_bytes = validators_value.into_bytes();
    let validators_raw: Vec<[u8; 32]> = Vec::<[u8; 32]>::decode(&mut &raw_bytes[..])?;
    let validators: Vec<AccountId32> = validators_raw.into_iter().map(AccountId32::from).collect();

    if validators.is_empty() {
        return Err(parity_scale_codec::Error::from("no validators found in storage").into());
    }

    Ok(validators)
}

/// Extract author ID from block header digest logs by mapping authority index to validator
async fn extract_author(state: &AppState, block_number: u64, logs: &[DigestLog]) -> Option<String> {
    use parity_scale_codec::Decode;
    use sp_consensus_babe::digests::PreDigest;

    const BABE_ENGINE: &[u8] = b"BABE";
    const AURA_ENGINE: &[u8] = b"aura";
    const POW_ENGINE: &[u8] = b"pow_";

    // Fetch validators once for this block
    let validators = match get_validators_at_block(state, block_number).await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Failed to get validators for block {}: {}", block_number, e);
            return None;
        }
    };

    // Check PreRuntime logs for BABE/Aura
    for log in logs {
        if log.log_type == "PreRuntime"
            && let Some(arr) = log.value.as_array()
            && arr.len() >= 2
        {
            let engine_id_hex = arr[0].as_str()?;
            let payload_hex = arr[1].as_str()?;
            let payload = hex::decode(payload_hex.strip_prefix("0x")?).ok()?;

            // Decode hex-encoded engine ID to bytes for comparison
            let engine_id_bytes = hex::decode(engine_id_hex.strip_prefix("0x")?).ok()?;

            match engine_id_bytes.as_slice() {
                BABE_ENGINE => {
                    if payload.is_empty() {
                        continue;
                    }

                    // The payload has already been decoded from SCALE in decode_consensus_digest
                    // So we can decode the PreDigest directly without skipping compact length
                    let mut cursor = &payload[..];
                    let pre_digest = PreDigest::decode(&mut cursor).ok()?;
                    let authority_index = pre_digest.authority_index() as usize;
                    let author = validators.get(authority_index)?;

                    // Convert to SS58 format
                    return Some(
                        author
                            .clone()
                            .to_ss58check_with_version(state.chain_info.ss58_prefix.into()),
                    );
                }
                AURA_ENGINE => {
                    // Aura: slot_number (u64 LE), calculate index = slot % validator_count
                    if payload.len() >= 8 {
                        let slot = u64::from_le_bytes([
                            payload[0], payload[1], payload[2], payload[3], payload[4], payload[5],
                            payload[6], payload[7],
                        ]) as usize;

                        let index = slot % validators.len();
                        let author = validators.get(index)?;

                        // Convert to SS58 format
                        return Some(
                            author
                                .clone()
                                .to_ss58check_with_version(state.chain_info.ss58_prefix.into()),
                        );
                    }
                }
                _ => continue,
            }
        }
    }

    // Check Consensus logs for PoW
    for log in logs {
        if log.log_type == "Consensus"
            && let Some(arr) = log.value.as_array()
            && arr.len() >= 2
        {
            let engine_id_hex = arr[0].as_str()?;
            let payload_hex = arr[1].as_str()?;

            // Decode hex-encoded engine ID to bytes for comparison
            let engine_id_bytes = hex::decode(engine_id_hex.strip_prefix("0x")?).ok()?;

            if engine_id_bytes.as_slice() == POW_ENGINE {
                // PoW: author is directly in payload (32-byte AccountId)
                let payload = hex::decode(payload_hex.strip_prefix("0x")?).ok()?;
                if payload.len() == 32 {
                    // Payload is exactly 32 bytes, convert directly to AccountId32
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&payload);
                    let account_id = AccountId32::from(arr);
                    return Some(
                        account_id.to_ss58check_with_version(state.chain_info.ss58_prefix.into()),
                    );
                } else {
                    tracing::debug!(
                        "PoW payload has unexpected length: {} bytes (expected 32)",
                        payload.len()
                    );
                }
            }
        }
    }

    None
}

// ================================================================================================
// Helper Functions - Event Processing
// ================================================================================================

/// Extract `paysFee` value from DispatchInfo in event data
///
/// DispatchInfo contains: { weight, class, paysFee }
/// paysFee can be:
/// - A boolean (true/false)
/// - A string ("Yes"/"No")
/// - An object with a "name" field containing "Yes"/"No"
///
/// For ExtrinsicSuccess: event_data = [DispatchInfo]
/// For ExtrinsicFailed: event_data = [DispatchError, DispatchInfo]
fn extract_pays_fee_from_event_data(event_data: &[Value], is_success: bool) -> Option<bool> {
    // For ExtrinsicSuccess, DispatchInfo is the first element
    // For ExtrinsicFailed, DispatchInfo is the second element (after DispatchError)
    let dispatch_info_index = if is_success { 0 } else { 1 };

    let dispatch_info = event_data.get(dispatch_info_index)?;

    // DispatchInfo should be an object with paysFee field
    let pays_fee_value = dispatch_info.get("paysFee")?;

    match pays_fee_value {
        // Direct boolean
        Value::Bool(b) => Some(*b),
        // String "Yes" or "No"
        Value::String(s) => match s.as_str() {
            "Yes" => Some(true),
            "No" => Some(false),
            _ => {
                tracing::debug!("Unknown paysFee string value: {}", s);
                None
            }
        },
        // Object with "name" field (e.g., { "name": "Yes", "values": ... })
        Value::Object(obj) => {
            if let Some(Value::String(name)) = obj.get("name") {
                match name.as_str() {
                    "Yes" => Some(true),
                    "No" => Some(false),
                    _ => {
                        tracing::debug!("Unknown paysFee name value: {}", name);
                        None
                    }
                }
            } else {
                None
            }
        }
        _ => {
            tracing::debug!("Unexpected paysFee value type: {:?}", pays_fee_value);
            None
        }
    }
}

/// Extract fee from TransactionFeePaid event if present
///
/// TransactionFeePaid event data: [who, actualFee, tip]
/// The actualFee is the exact fee paid for the transaction
fn extract_fee_from_transaction_paid_event(events: &[Event]) -> Option<String> {
    for event in events {
        // Check for System.TransactionFeePaid or TransactionPayment.TransactionFeePaid
        // Use case-insensitive comparison since pallet names may vary in casing
        let pallet_lower = event.method.pallet.to_lowercase();
        let is_fee_paid = (pallet_lower == "system" || pallet_lower == "transactionpayment")
            && event.method.method == "TransactionFeePaid";

        if is_fee_paid && event.data.len() >= 2 {
            // event.data[1] is the actualFee
            if let Some(fee_value) = event.data.get(1) {
                return Some(extract_number_as_string(fee_value));
            }
        }
    }
    None
}

/// Extract actual weight from DispatchInfo in event data
///
/// DispatchInfo contains: { weight, class, paysFee }
/// Weight can be:
/// - Modern format: { refTime/ref_time: "...", proofSize/proof_size: "..." }
/// - Legacy format: a single number (just refTime)
///
/// For ExtrinsicSuccess: event_data = [DispatchInfo]
/// For ExtrinsicFailed: event_data = [DispatchError, DispatchInfo]
fn extract_weight_from_event_data(event_data: &[Value], is_success: bool) -> Option<ActualWeight> {
    let dispatch_info_index = if is_success { 0 } else { 1 };
    let dispatch_info = event_data.get(dispatch_info_index)?;
    let weight_value = dispatch_info.get("weight")?;

    match weight_value {
        Value::Object(obj) => {
            // Handle both camelCase and snake_case key variants
            let ref_time = obj
                .get("refTime")
                .or_else(|| obj.get("ref_time"))
                .map(extract_number_as_string);
            let proof_size = obj
                .get("proofSize")
                .or_else(|| obj.get("proof_size"))
                .map(extract_number_as_string);

            Some(ActualWeight {
                ref_time,
                proof_size,
            })
        }
        // Legacy weight format: single number
        Value::Number(n) => Some(ActualWeight {
            ref_time: Some(n.to_string()),
            proof_size: None,
        }),
        Value::String(s) => {
            // Could be a hex string or decimal string
            let value = if s.starts_with("0x") {
                u128::from_str_radix(s.trim_start_matches("0x"), 16)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|_| s.clone())
            } else {
                s.clone()
            };
            Some(ActualWeight {
                ref_time: Some(value),
                proof_size: None,
            })
        }
        _ => {
            tracing::debug!("Unexpected weight value type: {:?}", weight_value);
            None
        }
    }
}

/// Extract class from DispatchInfo in event data
///
/// For ExtrinsicSuccess: event_data = [DispatchInfo]
/// For ExtrinsicFailed: event_data = [DispatchError, DispatchInfo]
fn extract_class_from_event_data(event_data: &[Value], is_success: bool) -> Option<String> {
    let dispatch_info_index = if is_success { 0 } else { 1 };
    let dispatch_info = event_data.get(dispatch_info_index)?;
    let class_value = dispatch_info.get("class")?;

    match class_value {
        Value::String(s) => Some(s.clone()),
        Value::Object(obj) => {
            // Might be { "name": "Normal", "values": ... } format
            obj.get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }
        _ => None,
    }
}

/// Unified transformation function that combines byte-to-hex conversion and structural transformations
/// in a single pass through the JSON tree.
///
/// This performs all of the following transformations in one traversal:
/// - Converts byte arrays to hex strings
/// - Converts numbers to strings
/// - Handles bitvec special encoding
/// - Transforms snake_case keys to camelCase
/// - Simplifies SCALE enum variants
/// - Optionally decodes AccountId32 to SS58 format
/// - Unwraps single-element arrays
fn transform_json_unified(value: Value, ss58_prefix: Option<u16>) -> Value {
    match value {
        Value::Number(n) => {
            // Convert all numbers to strings to match substrate-api-sidecar behavior
            Value::String(n.to_string())
        }
        Value::Array(arr) => {
            // Check if this is a byte array (all elements are numbers 0-255)
            // Require at least 2 elements - single-element arrays are typically newtype wrappers
            // (e.g., ValidatorIndex(32) -> [32]), not actual byte data
            let is_byte_array = arr.len() > 1
                && arr.iter().all(|v| match v {
                    Value::Number(n) => n.as_u64().is_some_and(|val| val <= 255),
                    _ => false,
                });

            if is_byte_array {
                // Convert to hex string
                let bytes: Vec<u8> = arr
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                Value::String(format!("0x{}", hex::encode(&bytes)))
            } else {
                // Recurse into array elements
                let converted: Vec<Value> = arr
                    .into_iter()
                    .map(|v| transform_json_unified(v, ss58_prefix))
                    .collect();

                // If array has single element, unwrap it
                match converted.len() {
                    1 => converted.into_iter().next().unwrap(),
                    _ => Value::Array(converted),
                }
            }
        }
        Value::Object(map) => {
            // Check if this is a bitvec object (scale-value represents bitvecs specially)
            if let Some(Value::Array(bits)) = map.get("__bitvec__values__") {
                // Convert boolean array to bytes, then to hex
                let mut bytes = Vec::new();
                let mut current_byte = 0u8;

                for (i, bit) in bits.iter().enumerate() {
                    if let Some(true) = bit.as_bool() {
                        current_byte |= 1 << (i % 8);
                    }

                    if (i + 1) % 8 == 0 {
                        bytes.push(current_byte);
                        current_byte = 0;
                    }
                }

                if bits.len() % 8 != 0 {
                    bytes.push(current_byte);
                }

                return Value::String(format!("0x{}", hex::encode(&bytes)));
            }

            // Check if this is a SCALE enum variant: {"name": "X", "values": Y}
            if map.len() == 2
                && let (Some(Value::String(name)), Some(values)) =
                    (map.get("name"), map.get("values"))
            {
                // If values is "0x" (empty string) or [] (empty array), return just the name as string
                // This is evident in class, and paysFee
                let is_empty = match values {
                    Value::String(v) => v == "0x",
                    Value::Array(arr) => arr.is_empty(),
                    _ => false,
                };

                if is_empty {
                    // Special case: "None" variant should serialize as JSON null
                    if name == "None" {
                        return Value::Null;
                    }
                    return Value::String(name.clone());
                }

                // For args (when ss58_prefix is Some), transform to {"<name>": <transformed_values>}
                if ss58_prefix.is_some() {
                    // Only lowercase the first letter for CamelCase names (e.g., "PreRuntime" -> "preRuntime")
                    // Keep snake_case names as-is (e.g., "inbound_messages_data" stays unchanged)
                    let key = lowercase_first_char(name);
                    let transformed_value = transform_json_unified(values.clone(), ss58_prefix);

                    let mut result = serde_json::Map::new();
                    result.insert(key, transformed_value);
                    return Value::Object(result);
                }
                // For events (when ss58_prefix is None), we don't transform the enum further
                // Fall through to regular object handling
            }

            // Regular object: transform keys from snake_case to camelCase and recurse
            let transformed: serde_json::Map<String, Value> = map
                .into_iter()
                .map(|(key, val)| {
                    let camel_key = key.to_lower_camel_case();
                    (camel_key, transform_json_unified(val, ss58_prefix))
                })
                .collect();
            Value::Object(transformed)
        }
        Value::String(s) => {
            // Try to decode as SS58 address if ss58_prefix is provided
            if let Some(prefix) = ss58_prefix
                && s.starts_with("0x")
                && (s.len() == 66 || s.len() == 68)
                && let Some(ss58_addr) = decode_address_to_ss58(&s, prefix)
            {
                return Value::String(ss58_addr);
            }

            Value::String(s)
        }
        other => other,
    }
}

/// Fetch and parse all events for a block
async fn fetch_block_events(
    state: &AppState,
    block_number: u64,
) -> Result<Vec<ParsedEvent>, GetBlockError> {
    use crate::handlers::blocks::events_visitor::{EventPhase as VisitorEventPhase, EventsVisitor};

    let client_at_block = state.client.at(block_number).await?;
    let storage_entry = client_at_block.storage().entry("System", "Events")?;
    let events_value = storage_entry.fetch(()).await?.ok_or_else(|| {
        tracing::warn!("No events storage found for block {}", block_number);
        parity_scale_codec::Error::from("Events storage not found")
    })?;

    // Use the visitor pattern to get type information for each field
    let events_with_types = events_value.visit(EventsVisitor::new()).map_err(|e| {
        tracing::warn!(
            "Failed to decode events for block {}: {:?}",
            block_number,
            e
        );
        GetBlockError::StorageDecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode events",
        ))
    })?;

    // Also decode with scale_value to preserve structure
    let events_vec = events_value
        .decode_as::<Vec<scale_value::Value<()>>>()
        .map_err(|e| {
            tracing::warn!(
                "Failed to decode events for block {}: {:?}",
                block_number,
                e
            );
            GetBlockError::StorageDecodeFailed(parity_scale_codec::Error::from(
                "Failed to decode events",
            ))
        })?;

    let mut parsed_events = Vec::new();

    // Process each event, combining type info from visitor with structure from scale_value
    for (event_info, event_record) in events_with_types.iter().zip(events_vec.iter()) {
        let phase = match event_info.phase {
            VisitorEventPhase::Initialization => EventPhase::Initialization,
            VisitorEventPhase::ApplyExtrinsic(idx) => EventPhase::ApplyExtrinsic(idx),
            VisitorEventPhase::Finalization => EventPhase::Finalization,
        };

        // Get the event variant from scale_value (to preserve structure)
        let event_composite = match &event_record.value {
            scale_value::ValueDef::Composite(comp) => comp,
            _ => continue,
        };

        let fields: Vec<&scale_value::Value<()>> = event_composite.values().collect();
        if fields.len() < 2 {
            continue;
        }

        if let scale_value::ValueDef::Variant(pallet_variant) = &fields[1].value {
            let inner_values: Vec<&scale_value::Value<()>> =
                pallet_variant.values.values().collect();

            if let Some(inner_value) = inner_values.first()
                && let scale_value::ValueDef::Variant(event_variant) = &inner_value.value
            {
                let field_values: Vec<&scale_value::Value<()>> =
                    event_variant.values.values().collect();

                let event_data: Vec<Value> = field_values
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, field)| {
                        let json_value = serde_json::to_value(&field.value).ok()?;

                        // Type-based AccountId32 detection using type info from visitor
                        if let Some(type_name) = event_info
                            .fields
                            .get(idx)
                            .and_then(|f| f.type_name.as_ref())
                            && (type_name == "AccountId32"
                                || type_name == "MultiAddress"
                                || type_name == "AccountId")
                        {
                            // For AccountId fields, we need hex conversion first, then SS58 conversion
                            let with_hex = convert_bytes_to_hex(json_value.clone());
                            if let Some(ss58_value) = try_convert_accountid_to_ss58(
                                &with_hex,
                                state.chain_info.ss58_prefix,
                            ) {
                                return Some(ss58_value);
                            }
                            // If SS58 conversion failed, fall through to unified transformation
                        }

                        // Single-pass transformation for non-AccountId fields (or AccountId fields where conversion failed)
                        Some(transform_json_unified(json_value, None))
                    })
                    .collect();

                parsed_events.push(ParsedEvent {
                    phase,
                    pallet_name: event_info.pallet_name.clone(),
                    event_name: event_info.event_name.clone(),
                    event_data,
                });
            }
        }
    }

    Ok(parsed_events)
}

/// Convert AccountId32 (as hex or array) to SS58 format
fn try_convert_accountid_to_ss58(value: &Value, ss58_prefix: u16) -> Option<Value> {
    if let Some(hex_str) = value.as_str()
        && hex_str.starts_with("0x")
        && hex_str.len() == 66
    {
        match hex::decode(&hex_str[2..]) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                let account_id = AccountId32::from(arr);
                let ss58 = account_id.to_ss58check_with_version(ss58_prefix.into());
                return Some(Value::String(ss58));
            }
            _ => {}
        }
    }

    if let Some(arr) = value.as_array()
        && arr.len() == 32
    {
        let mut bytes = [0u8; 32];
        for (i, val) in arr.iter().enumerate() {
            if let Some(byte) = val.as_u64() {
                bytes[i] = byte as u8;
            } else {
                return None;
            }
        }
        let account_id = AccountId32::from(bytes);
        let ss58 = account_id.to_ss58check_with_version(ss58_prefix.into());
        return Some(Value::String(ss58));
    }

    None
}

/// Categorize parsed events into onInitialize, per-extrinsic, and onFinalize arrays
/// Also extracts extrinsic outcomes (success, paysFee) from System.ExtrinsicSuccess/ExtrinsicFailed events
fn categorize_events(
    parsed_events: Vec<ParsedEvent>,
    num_extrinsics: usize,
) -> (
    OnInitialize,
    Vec<Vec<Event>>,
    OnFinalize,
    Vec<ExtrinsicOutcome>,
) {
    let mut on_initialize_events = Vec::new();
    let mut on_finalize_events = Vec::new();
    // Create empty event vectors for each extrinsic
    let mut per_extrinsic_events: Vec<Vec<Event>> = vec![Vec::new(); num_extrinsics];
    // Create default outcomes for each extrinsic (success=false, pays_fee=None)
    let mut extrinsic_outcomes: Vec<ExtrinsicOutcome> =
        vec![ExtrinsicOutcome::default(); num_extrinsics];

    for parsed_event in parsed_events {
        // Check for System.ExtrinsicSuccess or System.ExtrinsicFailed events
        // to determine extrinsic outcomes before consuming the event data
        // Note: pallet_name is lowercase (from events_visitor.rs which uses to_lowercase())
        let is_system_event = parsed_event.pallet_name == "system";
        let is_success_event = is_system_event && parsed_event.event_name == "ExtrinsicSuccess";
        let is_failed_event = is_system_event && parsed_event.event_name == "ExtrinsicFailed";

        // Extract outcome info if this is a success/failed event for an extrinsic
        if let EventPhase::ApplyExtrinsic(index) = &parsed_event.phase {
            let idx = *index as usize;
            if idx < num_extrinsics {
                if is_success_event {
                    extrinsic_outcomes[idx].success = true;
                    // Extract paysFee from DispatchInfo (first element in event data)
                    if let Some(pays_fee) =
                        extract_pays_fee_from_event_data(&parsed_event.event_data, true)
                    {
                        extrinsic_outcomes[idx].pays_fee = Some(pays_fee);
                    }
                    // Extract actual weight from DispatchInfo for fee calculation
                    if let Some(weight) =
                        extract_weight_from_event_data(&parsed_event.event_data, true)
                    {
                        extrinsic_outcomes[idx].actual_weight = Some(weight);
                    }
                    // Extract class from DispatchInfo
                    if let Some(class) =
                        extract_class_from_event_data(&parsed_event.event_data, true)
                    {
                        extrinsic_outcomes[idx].class = Some(class);
                    }
                } else if is_failed_event {
                    // success stays false
                    // Extract paysFee from DispatchInfo (second element in event data, after DispatchError)
                    if let Some(pays_fee) =
                        extract_pays_fee_from_event_data(&parsed_event.event_data, false)
                    {
                        extrinsic_outcomes[idx].pays_fee = Some(pays_fee);
                    }
                    // Extract actual weight from DispatchInfo for fee calculation
                    if let Some(weight) =
                        extract_weight_from_event_data(&parsed_event.event_data, false)
                    {
                        extrinsic_outcomes[idx].actual_weight = Some(weight);
                    }
                    // Extract class from DispatchInfo
                    if let Some(class) =
                        extract_class_from_event_data(&parsed_event.event_data, false)
                    {
                        extrinsic_outcomes[idx].class = Some(class);
                    }
                }
            }
        }

        let event = Event {
            method: MethodInfo {
                pallet: parsed_event.pallet_name,
                method: parsed_event.event_name,
            },
            data: parsed_event.event_data,
            docs: None, // Will be populated if eventDocs=true
        };

        match parsed_event.phase {
            EventPhase::Initialization => {
                on_initialize_events.push(event);
            }
            EventPhase::ApplyExtrinsic(index) => {
                if let Some(extrinsic_events) = per_extrinsic_events.get_mut(index as usize) {
                    extrinsic_events.push(event);
                } else {
                    tracing::warn!(
                        "Event has ApplyExtrinsic phase with index {} but only {} extrinsics exist",
                        index,
                        num_extrinsics
                    );
                }
            }
            EventPhase::Finalization => {
                on_finalize_events.push(event);
            }
        }
    }

    (
        OnInitialize {
            events: on_initialize_events,
        },
        per_extrinsic_events,
        OnFinalize {
            events: on_finalize_events,
        },
        extrinsic_outcomes,
    )
}

/// Transform fee info from payment_queryInfo RPC response into the expected format
///
/// The RPC returns RuntimeDispatchInfo with:
/// - weight: either { refTime/ref_time, proofSize/proof_size } (modern) or a single number (legacy)
/// - class: "Normal", "Operational", or "Mandatory"
/// - partialFee: fee amount (usually as hex string from RPC)
///
/// We transform this to match sidecar's format with string values
fn transform_fee_info(fee_info: Value) -> serde_json::Map<String, Value> {
    let mut result = serde_json::Map::new();

    if let Some(weight) = fee_info.get("weight") {
        if weight.is_object() {
            // Handle both camelCase and snake_case key variants from different node versions
            let mut weight_map = serde_json::Map::new();

            let ref_time = weight.get("refTime").or_else(|| weight.get("ref_time"));
            let proof_size = weight.get("proofSize").or_else(|| weight.get("proof_size"));

            if let Some(rt) = ref_time {
                weight_map.insert(
                    "refTime".to_string(),
                    Value::String(extract_number_as_string(rt)),
                );
            }
            if let Some(ps) = proof_size {
                weight_map.insert(
                    "proofSize".to_string(),
                    Value::String(extract_number_as_string(ps)),
                );
            }

            if !weight_map.is_empty() {
                result.insert("weight".to_string(), Value::Object(weight_map));
            }
        } else {
            result.insert(
                "weight".to_string(),
                Value::String(extract_number_as_string(weight)),
            );
        }
    }

    if let Some(class) = fee_info.get("class") {
        result.insert("class".to_string(), class.clone());
    }

    if let Some(partial_fee) = fee_info.get("partialFee") {
        result.insert(
            "partialFee".to_string(),
            Value::String(extract_number_as_string(partial_fee)),
        );
    }

    result
}

/// Extract a number from a JSON value and return it as a string
/// Handles: numbers, hex strings (0x...), and string numbers
fn extract_number_as_string(value: &Value) -> String {
    match value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.starts_with("0x") {
                if let Ok(n) = u128::from_str_radix(s.trim_start_matches("0x"), 16) {
                    n.to_string()
                } else {
                    s.clone()
                }
            } else {
                s.clone()
            }
        }
        _ => "0".to_string(),
    }
}

/// Convert ActualWeight to JSON value (V1: string, V2: object)
fn actual_weight_to_json(actual_weight: &ActualWeight) -> Option<Value> {
    let ref_time = actual_weight.ref_time.as_ref()?;
    Some(if let Some(ref proof_size) = actual_weight.proof_size {
        json!({ "refTime": ref_time, "proofSize": proof_size })
    } else {
        Value::String(ref_time.clone())
    })
}

/// Extract fee info for a signed extrinsic using the three-priority system:
/// 1. TransactionFeePaid event (exact fee from runtime)
/// 2. queryFeeDetails + calc_partial_fee (post-dispatch calculation)
/// 3. queryInfo (pre-dispatch estimation)
async fn extract_fee_info_for_extrinsic(
    state: &AppState,
    extrinsic_hex: &str,
    events: &[Event],
    outcome: Option<&ExtrinsicOutcome>,
    parent_hash: &str,
    spec_version: u32,
) -> serde_json::Map<String, Value> {
    // Priority 1: TransactionFeePaid event (exact fee from runtime)
    if let Some(fee_from_event) = extract_fee_from_transaction_paid_event(events) {
        let mut info = serde_json::Map::new();

        if let Some(outcome) = outcome {
            if let Some(ref actual_weight) = outcome.actual_weight
                && let Some(weight_value) = actual_weight_to_json(actual_weight)
            {
                info.insert("weight".to_string(), weight_value);
            }
            if let Some(ref class) = outcome.class {
                info.insert("class".to_string(), Value::String(class.clone()));
            }
        }

        info.insert("partialFee".to_string(), Value::String(fee_from_event));
        info.insert("kind".to_string(), Value::String("fromEvent".to_string()));
        return info;
    }

    // Priority 2: queryFeeDetails + calc_partial_fee (post-dispatch calculation)
    let actual_weight_str = outcome
        .and_then(|o| o.actual_weight.as_ref())
        .and_then(|w| w.ref_time.clone());

    if let Some(ref actual_weight_str) = actual_weight_str {
        let use_fee_details = state
            .fee_details_cache
            .is_available(&state.chain_info.spec_name, spec_version)
            .unwrap_or(true);

        if use_fee_details {
            if let Ok(fee_details_response) =
                state.query_fee_details(extrinsic_hex, parent_hash).await
            {
                state.fee_details_cache.set_available(spec_version, true);

                if let Some(fee_details) = utils::parse_fee_details(&fee_details_response) {
                    // Get estimated weight from queryInfo (try RPC first, then runtime API)
                    let query_info_result = get_query_info(state, extrinsic_hex, parent_hash).await;

                    if let Some((query_info, estimated_weight)) = query_info_result
                        && let Ok(partial_fee) = utils::calculate_accurate_fee(
                            &fee_details,
                            &estimated_weight,
                            actual_weight_str,
                        )
                    {
                        let mut info = transform_fee_info(query_info);
                        info.insert("partialFee".to_string(), Value::String(partial_fee));
                        info.insert(
                            "kind".to_string(),
                            Value::String("postDispatch".to_string()),
                        );
                        return info;
                    }
                }
            } else {
                state.fee_details_cache.set_available(spec_version, false);
            }
        }
    }

    // Priority 3: queryInfo (pre-dispatch estimation)
    if let Some((query_info, _)) = get_query_info(state, extrinsic_hex, parent_hash).await {
        let mut info = transform_fee_info(query_info);
        info.insert("kind".to_string(), Value::String("preDispatch".to_string()));
        return info;
    }

    serde_json::Map::new()
}

/// Get query info from RPC or runtime API fallback
async fn get_query_info(
    state: &AppState,
    extrinsic_hex: &str,
    parent_hash: &str,
) -> Option<(Value, String)> {
    // Try RPC first
    if let Ok(query_info) = state.query_fee_info(extrinsic_hex, parent_hash).await
        && let Some(weight) = utils::extract_estimated_weight(&query_info)
    {
        return Some((query_info, weight));
    }

    // Fall back to runtime API for historic blocks
    let extrinsic_bytes = hex::decode(extrinsic_hex.trim_start_matches("0x")).ok()?;
    let dispatch_info = state
        .query_fee_info_via_runtime_api(&extrinsic_bytes, parent_hash)
        .await
        .ok()?;

    let query_info = dispatch_info.to_json();
    let weight = dispatch_info.weight.ref_time().to_string();
    Some((query_info, weight))
}

// ================================================================================================
// Helper Functions - Documentation
// ================================================================================================

/// Zero-copy reference to documentation strings from metadata.
/// Supports all metadata versions V9-V16 without expensive encode/decode operations.
pub struct Docs<'a> {
    inner: DocsInner<'a>,
}

/// Internal representation of docs that can hold different reference types
/// depending on the metadata version.
enum DocsInner<'a> {
    /// Reference to Vec<String> (V14+ metadata uses this format)
    Strings(&'a [String]),
    /// Reference to static str slice (V9-V13 compile-time metadata)
    Static(&'a [&'static str]),
}

impl<'a> Docs<'a> {
    /// Create docs from a slice of Strings (V14+ metadata)
    fn from_strings(docs: &'a [String]) -> Option<Self> {
        if docs.is_empty() || docs.iter().all(|s| s.is_empty()) {
            None
        } else {
            Some(Self {
                inner: DocsInner::Strings(docs),
            })
        }
    }

    /// Create docs from a static str slice (V9-V13 metadata)
    fn from_static(docs: &'a [&'static str]) -> Option<Self> {
        if docs.is_empty() || docs.iter().all(|s| s.is_empty()) {
            None
        } else {
            Some(Self {
                inner: DocsInner::Static(docs),
            })
        }
    }

    /// Get event documentation from RuntimeMetadata.
    /// Works with all metadata versions V9-V16.
    pub fn for_event(
        metadata: &'a frame_metadata::RuntimeMetadata,
        pallet_name: &str,
        event_name: &str,
    ) -> Option<Docs<'a>> {
        get_event_docs(metadata, pallet_name, event_name)
    }

    /// Get call documentation from RuntimeMetadata.
    /// Works with all metadata versions V9-V16.
    pub fn for_call(
        metadata: &'a frame_metadata::RuntimeMetadata,
        pallet_name: &str,
        call_name: &str,
    ) -> Option<Docs<'a>> {
        get_call_docs(metadata, pallet_name, call_name)
    }
}

impl std::fmt::Display for Docs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            DocsInner::Strings(docs) => {
                let mut first = true;
                for doc in *docs {
                    if !first {
                        writeln!(f)?;
                    }
                    write!(f, "{}", doc)?;
                    first = false;
                }
                Ok(())
            }
            DocsInner::Static(docs) => {
                let mut first = true;
                for doc in *docs {
                    if !first {
                        writeln!(f)?;
                    }
                    write!(f, "{}", doc)?;
                    first = false;
                }
                Ok(())
            }
        }
    }
}

impl Serialize for Docs<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Extract event documentation from metadata (V9-V16)
/// Returns a zero-copy Docs reference when possible.
fn get_event_docs<'a>(
    metadata: &'a frame_metadata::RuntimeMetadata,
    pallet_name: &str,
    event_name: &str,
) -> Option<Docs<'a>> {
    use frame_metadata::RuntimeMetadata::*;
    use frame_metadata::decode_different::DecodeDifferent;

    // Helper to extract string from DecodeDifferent
    fn extract_str<'a>(s: &'a DecodeDifferent<&'static str, String>) -> &'a str {
        match s {
            DecodeDifferent::Decoded(v) => v.as_str(),
            DecodeDifferent::Encode(s) => s,
        }
    }

    // Helper to create Docs from DecodeDifferent docs
    fn docs_from_decode_different<'a>(
        docs: &'a DecodeDifferent<&'static [&'static str], Vec<String>>,
    ) -> Option<Docs<'a>> {
        match docs {
            DecodeDifferent::Decoded(v) => Docs::from_strings(v),
            DecodeDifferent::Encode(s) => Docs::from_static(s),
        }
    }

    match metadata {
        V9(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V10(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V11(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V12(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V13(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(events)) = &module.event
                    {
                        for event in events {
                            if extract_str(&event.name).eq_ignore_ascii_case(event_name) {
                                return docs_from_decode_different(&event.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V14(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(event_ty) = &pallet.event
                    && let Some(ty) = meta.types.resolve(event_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(event_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        V15(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(event_ty) = &pallet.event
                    && let Some(ty) = meta.types.resolve(event_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(event_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        V16(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(event_ty) = &pallet.event
                    && let Some(ty) = meta.types.resolve(event_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(event_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract call documentation from metadata (V9-V16)
/// Returns a zero-copy Docs reference when possible.
fn get_call_docs<'a>(
    metadata: &'a frame_metadata::RuntimeMetadata,
    pallet_name: &str,
    call_name: &str,
) -> Option<Docs<'a>> {
    use frame_metadata::RuntimeMetadata::*;
    use frame_metadata::decode_different::DecodeDifferent;

    // Helper to extract string from DecodeDifferent
    fn extract_str<'a>(s: &'a DecodeDifferent<&'static str, String>) -> &'a str {
        match s {
            DecodeDifferent::Decoded(v) => v.as_str(),
            DecodeDifferent::Encode(s) => s,
        }
    }

    // Helper to create Docs from DecodeDifferent docs
    fn docs_from_decode_different<'a>(
        docs: &'a DecodeDifferent<&'static [&'static str], Vec<String>>,
    ) -> Option<Docs<'a>> {
        match docs {
            DecodeDifferent::Decoded(v) => Docs::from_strings(v),
            DecodeDifferent::Encode(s) => Docs::from_static(s),
        }
    }

    match metadata {
        V9(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V10(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V11(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V12(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V13(meta) => {
            if let DecodeDifferent::Decoded(modules) = &meta.modules {
                for module in modules {
                    if extract_str(&module.name).eq_ignore_ascii_case(pallet_name)
                        && let Some(DecodeDifferent::Decoded(calls)) = &module.calls
                    {
                        for call in calls {
                            if extract_str(&call.name).eq_ignore_ascii_case(call_name) {
                                return docs_from_decode_different(&call.documentation);
                            }
                        }
                    }
                }
            }
            None
        }
        V14(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(call_ty) = &pallet.calls
                    && let Some(ty) = meta.types.resolve(call_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(call_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        V15(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(call_ty) = &pallet.calls
                    && let Some(ty) = meta.types.resolve(call_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(call_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        V16(meta) => {
            for pallet in &meta.pallets {
                if pallet.name.eq_ignore_ascii_case(pallet_name)
                    && let Some(call_ty) = &pallet.calls
                    && let Some(ty) = meta.types.resolve(call_ty.ty.id)
                    && let scale_info::TypeDef::Variant(variant_def) = &ty.type_def
                {
                    for variant in &variant_def.variants {
                        if variant.name.eq_ignore_ascii_case(call_name) {
                            return Docs::from_strings(&variant.docs);
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

// ================================================================================================
// Helper Functions - Extrinsic Processing
// ================================================================================================

/// Extract extrinsics from a block using subxt-historic
async fn extract_extrinsics(
    state: &AppState,
    block_number: u64,
) -> Result<Vec<ExtrinsicInfo>, GetBlockError> {
    // Use subxt-historic to get a client at the specific block height
    // This ensures we use the correct metadata for that block
    let client_at_block = match state.client.at(block_number).await {
        Ok(client) => client,
        Err(e) => {
            // This should never happen in production with real chains
            // If it does, it indicates a serious issue with metadata or RPC
            tracing::warn!(
                "Failed to get client at block {}: {:?}. Returning empty extrinsics. \
                 This is expected in tests with mock RPC, but should not happen in production.",
                block_number,
                e
            );
            return Ok(Vec::new());
        }
    };

    let extrinsics = match client_at_block.extrinsics().fetch().await {
        Ok(exts) => exts,
        Err(e) => {
            // This could indicate RPC issues or network problems
            tracing::warn!(
                "Failed to fetch extrinsics for block {}: {:?}. Returning empty extrinsics.",
                block_number,
                e
            );
            return Ok(Vec::new());
        }
    };

    let mut result = Vec::new();

    for extrinsic in extrinsics.iter() {
        // Extract pallet and method name from the call, converting to lowerCamelCase
        let pallet_name = extrinsic.call().pallet_name().to_lower_camel_case();
        let method_name = extrinsic.call().name().to_lower_camel_case();

        // Extract call arguments with field-name-based AccountId32 detection
        let fields = extrinsic.call().fields();
        let mut args_map = serde_json::Map::new();

        for field in fields.iter() {
            let field_name = field.name();
            // Keep field names as-is (snake_case from SCALE metadata)
            // Only nested object keys are transformed to camelCase via transform_json_unified
            let field_key = field_name.to_string();

            // Use the visitor pattern to get type information
            // This definitively detects AccountId32 fields by their actual type!
            let type_name = field.visit(GetTypeName::new()).ok().flatten();

            // Log the type name for demonstration
            if let Some(tn) = type_name {
                tracing::debug!(
                    "Field '{}' in {}.{} has type: {}",
                    field_name,
                    pallet_name,
                    method_name,
                    tn
                );
            }

            // Try to decode as AccountId32-related types based on the detected type name
            let is_account_type = type_name == Some("AccountId32")
                || type_name == Some("MultiAddress")
                || type_name == Some("AccountId");

            if is_account_type {
                let mut decoded_account = false;
                let ss58_prefix = state.chain_info.ss58_prefix;
                let bytes_to_ss58 = |bytes: &[u8; 32]| {
                    let account_id = AccountId32::from(*bytes);
                    account_id.to_ss58check_with_version(ss58_prefix.into())
                };

                if let Ok(account_bytes) = field.decode_as::<[u8; 32]>() {
                    let ss58 = bytes_to_ss58(&account_bytes);
                    args_map.insert(field_key.clone(), json!(ss58));
                    decoded_account = true;
                } else if let Ok(accounts) = field.decode_as::<Vec<[u8; 32]>>() {
                    let ss58_addresses: Vec<String> = accounts.iter().map(&bytes_to_ss58).collect();
                    args_map.insert(field_key.clone(), json!(ss58_addresses));
                    decoded_account = true;
                } else if let Ok(multi_addr) = field.decode_as::<MultiAddress>() {
                    let value = match multi_addr {
                        MultiAddress::Id(bytes) => {
                            json!({ "id": bytes_to_ss58(&bytes) })
                        }
                        MultiAddress::Address32(bytes) => {
                            json!({ "address32": bytes_to_ss58(&bytes) })
                        }
                        MultiAddress::Index(index) => json!({ "index": index }),
                        MultiAddress::Raw(bytes) => {
                            json!({ "raw": format!("0x{}", hex::encode(bytes)) })
                        }
                        MultiAddress::Address20(bytes) => {
                            json!({ "address20": format!("0x{}", hex::encode(bytes)) })
                        }
                    };
                    args_map.insert(field_key.clone(), value);
                    decoded_account = true;
                }

                if decoded_account {
                    continue;
                }
                // If we failed to decode as account types, fall through to Value<()> decoding
            }

            // For non-account fields (or account fields that failed to decode), use Value<()>
            match field.decode_as::<scale_value::Value<()>>() {
                Ok(value) => {
                    let json_value = serde_json::to_value(&value).unwrap_or(Value::Null);
                    // Single-pass transformation: combines byte-to-hex, snake_case, enum simplification, and SS58 decoding
                    let transformed =
                        transform_json_unified(json_value, Some(state.chain_info.ss58_prefix));
                    args_map.insert(field_key, transformed);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to decode field '{}' in {}.{}: {}",
                        field_name,
                        pallet_name,
                        method_name,
                        e
                    );
                }
            }
        }

        // Extract signature and signer (if signed)
        let (signature_info, era_from_bytes) = if extrinsic.is_signed() {
            let sig_bytes = extrinsic
                .signature_bytes()
                .ok_or(GetBlockError::MissingSignatureBytes)?;
            let addr_bytes = extrinsic
                .address_bytes()
                .ok_or(GetBlockError::MissingAddressBytes)?;

            // Try to extract era from raw extrinsic bytes
            // Era comes right after address and signature in the SignedExtra/TransactionExtension
            let era_info = utils::extract_era_from_extrinsic_bytes(extrinsic.bytes());

            let signer_hex = format!("0x{}", hex::encode(addr_bytes));
            let signer_ss58 = decode_address_to_ss58(&signer_hex, state.chain_info.ss58_prefix)
                .unwrap_or_else(|| signer_hex.clone());

            // Strip the signature type prefix byte (0x00=Ed25519, 0x01=Sr25519, 0x02=Ecdsa)
            let signature_without_type_prefix = if sig_bytes.len() > 1 {
                &sig_bytes[1..]
            } else {
                sig_bytes
            };

            (
                Some(SignatureInfo {
                    signature: format!("0x{}", hex::encode(signature_without_type_prefix)),
                    signer: SignerId { id: signer_ss58 },
                }),
                era_info,
            )
        } else {
            (None, None)
        };

        // Extract nonce, tip, and era from transaction extensions (if present)
        let (nonce, tip, era_info) = if let Some(extensions) = extrinsic.transaction_extensions() {
            let mut nonce_value = None;
            let mut tip_value = None;
            let mut era_value = None;

            tracing::trace!(
                "Extrinsic {} has {} extensions",
                extrinsic.index(),
                extensions.iter().count()
            );

            for ext in extensions.iter() {
                let ext_name = ext.name();
                tracing::trace!("Extension name: {}", ext_name);

                match ext_name {
                    "CheckNonce" => {
                        // Decode as a u64/u32 compact value, then serialize to JSON
                        if let Ok(n) = ext.decode_as::<scale_value::Value>()
                            && let Ok(json_val) = serde_json::to_value(&n)
                        {
                            // The value might be nested in an object, so we need to extract it
                            // If extraction fails, nonce_value remains None (serialized as null)
                            nonce_value = extract_numeric_string(&json_val);
                        }
                    }
                    "ChargeTransactionPayment" | "ChargeAssetTxPayment" => {
                        // The tip is typically a Compact<u128>
                        if let Ok(t) = ext.decode_as::<scale_value::Value>()
                            && let Ok(json_val) = serde_json::to_value(&t)
                        {
                            // If extraction fails, tip_value remains None (serialized as null)
                            tip_value = extract_numeric_string(&json_val);
                        }
                    }
                    "CheckMortality" | "CheckEra" => {
                        // Era information - decode directly from raw bytes
                        // The JSON representation is complex (e.g., "Mortal230") and harder to parse
                        let era_bytes = ext.bytes();
                        tracing::debug!(
                            "Found CheckMortality extension, raw bytes: {}",
                            hex::encode(era_bytes)
                        );

                        let mut offset = 0;
                        if let Some(decoded_era) =
                            utils::decode_era_from_bytes(era_bytes, &mut offset)
                        {
                            tracing::debug!("Decoded era: {:?}", decoded_era);

                            // Create a JSON representation that parse_era_info can understand
                            if let Some(ref mortal) = decoded_era.mortal_era {
                                // Format: {"name": "Mortal", "values": [[period], [phase]]}
                                let mut map = serde_json::Map::new();
                                map.insert("name".to_string(), Value::String("Mortal".to_string()));

                                let values = vec![
                                    Value::Array(vec![Value::Number(
                                        mortal[0].parse::<u64>().unwrap().into(),
                                    )]),
                                    Value::Array(vec![Value::Number(
                                        mortal[1].parse::<u64>().unwrap().into(),
                                    )]),
                                ];
                                map.insert("values".to_string(), Value::Array(values));

                                era_value = Some(Value::Object(map));
                            } else if decoded_era.immortal_era.is_some() {
                                let mut map = serde_json::Map::new();
                                map.insert(
                                    "name".to_string(),
                                    Value::String("Immortal".to_string()),
                                );
                                era_value = Some(Value::Object(map));
                            }
                        }
                    }
                    _ => {
                        // Silently skip other extensions
                    }
                }
            }

            let era = if let Some(era_json) = era_value {
                // Try to parse era information from extension
                utils::parse_era_info(&era_json)
            } else if let Some(era_parsed) = era_from_bytes {
                // Use era extracted from raw bytes
                era_parsed
            } else {
                // Default to immortal era for signed transactions without explicit era
                EraInfo {
                    immortal_era: Some("0x00".to_string()),
                    mortal_era: None,
                }
            };

            (nonce_value, tip_value, era)
        } else {
            // Unsigned extrinsics are immortal
            (
                None,
                None,
                EraInfo {
                    immortal_era: Some("0x00".to_string()),
                    mortal_era: None,
                },
            )
        };

        let extrinsic_bytes = extrinsic.bytes();
        let hash_bytes = BlakeTwo256::hash(extrinsic_bytes);
        let hash = format!("0x{}", hex::encode(hash_bytes.as_ref()));
        let raw_hex = format!("0x{}", hex::encode(extrinsic_bytes));

        // Initialize pays_fee based on whether the extrinsic is signed:
        // - Unsigned extrinsics (inherents) never pay fees → Some(false)
        // - Signed extrinsics: determined from DispatchInfo in events → None (will be updated later)
        let is_signed = signature_info.is_some();
        let pays_fee = if is_signed { None } else { Some(false) };

        result.push(ExtrinsicInfo {
            method: MethodInfo {
                pallet: pallet_name,
                method: method_name,
            },
            signature: signature_info,
            nonce,
            args: args_map,
            tip,
            hash,
            info: serde_json::Map::new(),
            era: era_info,
            events: Vec::new(),
            success: false,
            pays_fee,
            docs: None, // Will be populated if extrinsicDocs=true
            raw_hex,
        });
    }

    Ok(result)
}

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /blocks/{blockId}
///
/// Returns block information for a given block identifier (hash or number)
///
/// Query Parameters:
/// - `eventDocs` (boolean, default: false): Include documentation for events
/// - `extrinsicDocs` (boolean, default: false): Include documentation for extrinsics
pub async fn get_block(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<BlockQueryParams>,
) -> Result<Json<BlockResponse>, GetBlockError> {
    let block_id = block_id.parse::<utils::BlockId>()?;
    // Track if the block was queried by hash (needed for canonical chain check)
    let queried_by_hash = matches!(block_id, utils::BlockId::Hash(_));
    let resolved_block = utils::resolve_block(&state, Some(block_id)).await?;
    let header_json = state
        .get_header_json(&resolved_block.hash)
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;

    let parent_hash = header_json
        .get("parentHash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("parentHash".to_string()))?
        .to_string();

    let state_root = header_json
        .get("stateRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("stateRoot".to_string()))?
        .to_string();

    let extrinsics_root = header_json
        .get("extrinsicsRoot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GetBlockError::HeaderFieldMissing("extrinsicsRoot".to_string()))?
        .to_string();

    let logs = decode_digest_logs(&header_json);
    let (author_id, extrinsics_result, events_result, finalized_head_result, canonical_hash_result) = tokio::join!(
        extract_author(&state, resolved_block.number, &logs),
        extract_extrinsics(&state, resolved_block.number),
        fetch_block_events(&state, resolved_block.number),
        get_finalized_block_number(&state),
        // Only fetch canonical hash if queried by hash (needed for fork detection)
        async {
            if queried_by_hash {
                Some(get_canonical_hash_at_number(&state, resolved_block.number).await)
            } else {
                None
            }
        }
    );

    let extrinsics = extrinsics_result?;
    let block_events = events_result?;
    let finalized_head_number = finalized_head_result?;

    // Determine if the block is finalized:
    // 1. Block number must be <= finalized head number
    // 2. If queried by hash, the hash must match the canonical chain hash
    //    (to detect blocks on forked/orphaned chains)
    let finalized = if resolved_block.number <= finalized_head_number {
        if let Some(canonical_result) = canonical_hash_result {
            // Queried by hash - verify it's on the canonical chain
            match canonical_result? {
                Some(canonical_hash) => canonical_hash == resolved_block.hash,
                // If canonical hash not found, block is not finalized
                None => false,
            }
        } else {
            // Queried by number - assumed to be canonical
            true
        }
    } else {
        false
    };

    // Categorize events by phase and extract extrinsic outcomes (success, paysFee)
    let (on_initialize, per_extrinsic_events, on_finalize, extrinsic_outcomes) =
        categorize_events(block_events, extrinsics.len());

    let mut extrinsics_with_events = extrinsics;
    for (i, (extrinsic_events, outcome)) in per_extrinsic_events
        .iter()
        .zip(extrinsic_outcomes.iter())
        .enumerate()
    {
        if let Some(extrinsic) = extrinsics_with_events.get_mut(i) {
            extrinsic.events = extrinsic_events.clone();
            extrinsic.success = outcome.success;
            // Only update pays_fee from events if the extrinsic is SIGNED.
            // Unsigned extrinsics (inherents) never pay fees, regardless of what
            // DispatchInfo.paysFee says in the event. The event's paysFee indicates
            // whether the call *would* pay a fee if called as a transaction, but
            // inherents are inserted by block authors and don't actually pay fees.
            if extrinsic.signature.is_some() && outcome.pays_fee.is_some() {
                extrinsic.pays_fee = outcome.pays_fee;
            }
        }
    }

    let spec_version = state
        .get_runtime_version_at_hash(&resolved_block.hash)
        .await
        .ok()
        .and_then(|v| v.get("specVersion").and_then(|sv| sv.as_u64()))
        .map(|v| v as u32)
        .unwrap_or(state.chain_info.spec_version);

    // Populate fee info for signed extrinsics that pay fees
    for (i, extrinsic) in extrinsics_with_events.iter_mut().enumerate() {
        if extrinsic.signature.is_some() && extrinsic.pays_fee == Some(true) {
            extrinsic.info = extract_fee_info_for_extrinsic(
                &state,
                &extrinsic.raw_hex,
                &extrinsic.events,
                extrinsic_outcomes.get(i),
                &parent_hash,
                spec_version,
            )
            .await;
        }
    }

    // Optionally populate documentation for events and extrinsics
    let (mut on_initialize, mut on_finalize) = (on_initialize, on_finalize);

    if (params.event_docs || params.extrinsic_docs)
        && let Ok(client_at_block) = state.client.at(resolved_block.number).await
    {
        let metadata = client_at_block.metadata();

        if params.event_docs {
            let add_docs_to_events =
                |events: &mut Vec<Event>, metadata: &frame_metadata::RuntimeMetadata| {
                    for event in events.iter_mut() {
                        // Pallet names in metadata are PascalCase, but our pallet names are lowerCamelCase
                        // We need to convert back: "system" -> "System", "balances" -> "Balances"
                        let pallet_name = event.method.pallet.to_upper_camel_case();
                        event.docs = Docs::for_event(metadata, &pallet_name, &event.method.method)
                            .map(|d| d.to_string());
                    }
                };

            add_docs_to_events(&mut on_initialize.events, metadata);
            add_docs_to_events(&mut on_finalize.events, metadata);

            for extrinsic in extrinsics_with_events.iter_mut() {
                add_docs_to_events(&mut extrinsic.events, metadata);
            }
        }

        if params.extrinsic_docs {
            for extrinsic in extrinsics_with_events.iter_mut() {
                // Pallet names in metadata are PascalCase, but our pallet names are lowerCamelCase
                // We need to convert back: "system" -> "System", "balances" -> "Balances"
                // Method names in metadata are snake_case, but our method names are lowerCamelCase
                let pallet_name = extrinsic.method.pallet.to_upper_camel_case();
                let method_name = extrinsic.method.method.to_snake_case();
                extrinsic.docs =
                    Docs::for_call(metadata, &pallet_name, &method_name).map(|d| d.to_string());
            }
        }
    }

    let response = BlockResponse {
        number: resolved_block.number.to_string(),
        hash: resolved_block.hash,
        parent_hash,
        state_root,
        extrinsics_root,
        author_id,
        logs,
        on_initialize,
        extrinsics: extrinsics_with_events,
        on_finalize,
        finalized,
    };

    Ok(Json(response))
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use config::SidecarConfig;
    use serde_json::json;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    /// Helper to create a test AppState with mocked RPC responses
    fn create_test_state_with_mock(mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let rpc_client = Arc::new(RpcClient::new(mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: config::ChainType::Relay,
            spec_name: "test".to_string(),
            spec_version: 1,
            ss58_prefix: 42,
        };

        AppState {
            config,
            client: Arc::new(subxt_historic::OnlineClient::from_rpc_client(
                subxt_historic::SubstrateConfig::new(),
                (*rpc_client).clone(),
            )),
            legacy_rpc,
            rpc_client,
            chain_info,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
        }
    }

    #[tokio::test]
    #[ignore] // Requires proper subxt metadata mocking for event fetching
    async fn test_get_block_by_number() {
        // Note: We don't mock state_getStorage here, so author_id will be None
        // Full author extraction is tested against live chain
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64",
                    "parentHash": "0xabcdef0000000000000000000000000000000000000000000000000000000000",
                    "stateRoot": "0xdef0000000000000000000000000000000000000000000000000000000000000",
                    "extrinsicsRoot": "0x1230000000000000000000000000000000000000000000000000000000000000",
                    "digest": {
                        "logs": [
                            // PreRuntime log: discriminant (6) + engine_id ("BABE") + variant (01) + authority_index (03000000 = 3 in LE)
                            "0x06424142450103000000"
                        ]
                    }
                }))
            })
            // Mock archive_v1_body to return empty extrinsics array
            .method_handler("archive_v1_body", async |_params| {
                MockJson(json!([]))
            })
            // Mock state_getRuntimeVersion for subxt metadata fetch
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(json!({
                    "specVersion": 1,
                    "transactionVersion": 1
                }))
            })
            // Mock state_getMetadata for subxt
            .method_handler("state_getMetadata", async |_params| {
                // Return minimal valid metadata (this is a complex SCALE-encoded structure)
                // For testing, we'll return a minimal valid metadata hex
                MockJson("0x6d657461")
            })
            // Mock state_getStorage for System.Events (returns empty events)
            .method_handler("state_getStorage", async |_params| {
                // Return SCALE-encoded empty Vec<EventRecord>
                MockJson("0x00")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let result = get_block(
            State(state),
            Path("100".to_string()),
            Query(BlockQueryParams::default()),
        )
        .await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.number, "100");
        assert_eq!(
            response.hash,
            "0x1234567890123456789012345678901234567890123456789012345678901234"
        );
        assert_eq!(
            response.parent_hash,
            "0xabcdef0000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            response.state_root,
            "0xdef0000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            response.extrinsics_root,
            "0x1230000000000000000000000000000000000000000000000000000000000000"
        );
        // Author extraction requires validators from chain state - tested with live chain
        assert_eq!(response.author_id, None);
        // Verify logs are decoded
        assert_eq!(response.logs.len(), 1);
        assert_eq!(response.logs[0].log_type, "PreRuntime");
        assert_eq!(response.logs[0].index, "6"); // PreRuntime discriminant
        // Verify the engine ID is hex-encoded and payload is present
        if let Some(arr) = response.logs[0].value.as_array() {
            assert_eq!(arr[0].as_str(), Some("0x42414245")); // "BABE" in hex
            assert!(arr[1].as_str().unwrap().starts_with("0x"));
        } else {
            panic!("Expected PreRuntime log value to be an array");
        }
        // Extrinsics are empty in mock (requires real chain data)
        assert_eq!(response.extrinsics.len(), 0);
    }

    #[tokio::test]
    #[ignore] // Requires proper subxt metadata mocking for event fetching
    async fn test_get_block_by_hash() {
        let test_hash = "0xabcdef1234567890123456789012345678901234567890123456789012345678";

        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getHeader", async |_params| {
                MockJson(json!({
                    "number": "0x64", // Block 100
                    "parentHash": "0x9999990000000000000000000000000000000000000000000000000000000000",
                    "stateRoot": "0x8888880000000000000000000000000000000000000000000000000000000000",
                    "extrinsicsRoot": "0x7777770000000000000000000000000000000000000000000000000000000000",
                    "digest": {
                        "logs": []
                    }
                }))
            })
            // Mock archive_v1_body to return empty extrinsics array
            .method_handler("archive_v1_body", async |_params| {
                MockJson(json!([]))
            })
            // Mock state_getRuntimeVersion for subxt metadata fetch
            .method_handler("state_getRuntimeVersion", async |_params| {
                MockJson(json!({
                    "specVersion": 1,
                    "transactionVersion": 1
                }))
            })
            // Mock state_getMetadata for subxt
            .method_handler("state_getMetadata", async |_params| {
                // Return minimal valid metadata (this is a complex SCALE-encoded structure)
                // For testing, we'll return a minimal valid metadata hex
                MockJson("0x6d657461")
            })
            // Mock state_getStorage for System.Events (returns empty events)
            .method_handler("state_getStorage", async |_params| {
                // Return SCALE-encoded empty Vec<EventRecord>
                MockJson("0x00")
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let result = get_block(
            State(state),
            Path(test_hash.to_string()),
            Query(BlockQueryParams::default()),
        )
        .await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.number, "100");
        assert_eq!(response.hash, test_hash);
        assert_eq!(
            response.parent_hash,
            "0x9999990000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            response.state_root,
            "0x8888880000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            response.extrinsics_root,
            "0x7777770000000000000000000000000000000000000000000000000000000000"
        );
        // No logs means no author can be extracted
        assert_eq!(response.author_id, None);
        // Empty logs array
        assert_eq!(response.logs.len(), 0);
        // Extrinsics are empty in mock (requires real chain data)
        assert_eq!(response.extrinsics.len(), 0);
    }

    #[tokio::test]
    async fn test_get_block_invalid_param() {
        let mock_client = MockRpcClient::builder().build();
        let state = create_test_state_with_mock(mock_client);

        let result = get_block(
            State(state),
            Path("invalid".to_string()),
            Query(BlockQueryParams::default()),
        )
        .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetBlockError::InvalidBlockParam(_)
        ));
    }

    #[tokio::test]
    async fn test_get_block_not_found() {
        let mock_client = MockRpcClient::builder()
            .method_handler("chain_getBlockHash", async |_params| {
                MockJson(serde_json::Value::Null)
            })
            .build();

        let state = create_test_state_with_mock(mock_client);

        let result = get_block(
            State(state),
            Path("999999".to_string()),
            Query(BlockQueryParams::default()),
        )
        .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetBlockError::BlockResolveFailed(_)
        ));
    }
}
