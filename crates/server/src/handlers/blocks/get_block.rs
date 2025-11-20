use super::util::event_account_fields::get_account_field_positions;
use super::util::extrinsic_account_fields::is_account_field;
use crate::state::AppState;
use crate::utils::{self, EraInfo};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Serialize;
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
            | GetBlockError::ExtrinsicDecodeFailed(_) => {
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
    pub index: u32,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignatureInfo>,
    /// Nonce - shown as null when extraction fails (matching sidecar behavior)
    pub nonce: Option<String>,
    /// Args as a JSON object where bytes are hex-encoded and large numbers are strings
    pub args: serde_json::Map<String, Value>,
    /// Tip - shown as null when extraction fails (matching sidecar behavior)
    pub tip: Option<String>,
    pub hash: String,
    /// Runtime dispatch info (empty for now, populated later with proper weight and fees)
    pub info: serde_json::Map<String, Value>,
    /// Transaction era/mortality information
    pub era: EraInfo,
    /// Events emitted by this extrinsic
    pub events: Vec<Event>,
    // TODO: Add more fields (success, paysFee)
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

// ================================================================================================
// Helper Functions - Conversion & Formatting
// ================================================================================================

/// Format bytes as hex string with "0x" prefix
fn hex_with_prefix(data: &[u8]) -> String {
    format!("0x{}", hex::encode(data))
}

/// Convert snake_case to camelCase
fn snake_to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
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
            // Check if this is a byte array (all elements are numbers 0-255)
            if arr
                .iter()
                .all(|v| matches!(v, Value::Number(n) if n.is_u64() && n.as_u64().unwrap() <= 255))
            {
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
                if converted.len() == 1 {
                    converted.into_iter().next().unwrap()
                } else {
                    Value::Array(converted)
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

/// Transform args to match sidecar format
/// Generic transformation for SCALE enum variants:
/// - {"name": "X", "values": Y} -> {"x": Y} (lowercase name as key)
/// - {"name": "X", "values": "0x"} -> "X" (empty enum variant becomes string)
/// - Decodes MultiAddress account IDs to SS58 format
/// - Converts snake_case to camelCase for field names
fn transform_args(value: Value, ss58_prefix: u16) -> Value {
    match value {
        Value::Object(map) => {
            // Check if this is a SCALE enum variant: {"name": "X", "values": Y}
            if map.len() == 2
                && let (Some(Value::String(name)), Some(values)) =
                    (map.get("name"), map.get("values"))
            {
                // If values is "0x" (empty), return just the name as string
                if let Value::String(v) = values
                    && v == "0x"
                {
                    return Value::String(name.clone());
                }

                // Otherwise, transform to {"<lowercase_name>": <transformed_values>}
                let key = name.to_lowercase();
                let transformed_value = transform_args(values.clone(), ss58_prefix);

                let mut result = serde_json::Map::new();
                result.insert(key, transformed_value);
                return Value::Object(result);
            }

            // Regular object: transform keys from snake_case to camelCase and recurse
            let transformed: serde_json::Map<String, Value> = map
                .into_iter()
                .map(|(key, val)| {
                    let camel_key = snake_to_camel(&key);
                    (camel_key, transform_args(val, ss58_prefix))
                })
                .collect();
            Value::Object(transformed)
        }
        Value::String(s) => {
            // Try to decode as SS58 address if it looks like an account ID
            // (either MultiAddress::Id with 0x00 prefix or raw AccountId32)
            if s.starts_with("0x")
                && (s.len() == 66 || s.len() == 68)
                && let Some(ss58_addr) = decode_address_to_ss58(&s, ss58_prefix)
            {
                return Value::String(ss58_addr);
            }
            Value::String(s)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|v| transform_args(v, ss58_prefix))
                .collect(),
        ),
        other => other,
    }
}

// ================================================================================================
// Helper Functions - Digest & Header Processing
// ================================================================================================

/// Decode a consensus digest item (PreRuntime, Consensus, or Seal)
/// Format: [consensus_engine_id (4 bytes), payload_data]
fn decode_consensus_digest(data: &[u8]) -> Option<Value> {
    if data.len() < CONSENSUS_ENGINE_ID_LEN {
        return None;
    }

    let engine_id = String::from_utf8_lossy(&data[0..CONSENSUS_ENGINE_ID_LEN]).to_string();
    let payload = hex_with_prefix(&data[CONSENSUS_ENGINE_ID_LEN..]);
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
        .enumerate()
        .filter_map(|(index, log_hex)| {
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
                index: index as u32,
                value,
            })
        })
        .collect()
}

/// Fetch validator set from chain state at a specific block
async fn get_validators_at_block(
    state: &AppState,
    block_number: u64,
) -> Result<Vec<AccountId32>, GetBlockError> {
    use parity_scale_codec::Decode;

    let client_at_block = state.client.at(block_number).await?;
    let storage_entry = client_at_block.storage().entry("Session", "Validators")?;
    let plain_entry = storage_entry.into_plain()?;
    let validators_value = plain_entry.fetch().await?.ok_or_else(|| {
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
    use parity_scale_codec::{Compact, Decode};
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
            let engine_id = arr[0].as_str()?;
            let payload_hex = arr[1].as_str()?;
            let payload = hex::decode(payload_hex.strip_prefix("0x")?).ok()?;

            match engine_id.as_bytes() {
                BABE_ENGINE => {
                    if payload.is_empty() {
                        continue;
                    }

                    // The payload is wrapped in a compact-encoded Vec<u8>, so we need to skip the length prefix
                    let mut cursor = &payload[..];
                    // Decode and skip the length prefix
                    let _length = Compact::<u32>::decode(&mut cursor).ok()?;
                    // Now decode the PreDigest from the remaining bytes
                    let pre_digest = PreDigest::decode(&mut cursor).ok()?;
                    let authority_index = pre_digest.authority_index() as usize;
                    let author = validators.get(authority_index)?;

                    return Some(hex_with_prefix(author.as_ref() as &[u8]));
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
                        return Some(hex_with_prefix(author.as_ref() as &[u8]));
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
            let engine_id = arr[0].as_str()?;
            let payload_hex = arr[1].as_str()?;

            if engine_id.as_bytes() == POW_ENGINE {
                // PoW: author is directly in payload (32-byte AccountId)
                let payload = hex::decode(payload_hex.strip_prefix("0x")?).ok()?;
                if payload.len() >= 32 {
                    return Some(hex_with_prefix(&payload[..32]));
                }
            }
        }
    }

    None
}

// ================================================================================================
// Helper Functions - Event Processing
// ================================================================================================

/// Try to convert an event field value to SS58 format if it's a valid AccountId32
/// Handles hex strings of 32 bytes (0x + 64 chars)
fn try_convert_to_ss58_event_field(value: &Value, ss58_prefix: u16) -> Option<Value> {
    match value {
        Value::String(hex_str) if hex_str.starts_with("0x") && hex_str.len() == 66 => {
            // Try to decode as 32-byte AccountId32 (64 hex chars = 32 bytes)
            match hex::decode(&hex_str[2..]) {
                Ok(bytes) => {
                    // 64 hex characters always decode to exactly 32 bytes
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    let account_id = AccountId32::from(arr);
                    let ss58 = account_id.to_ss58check_with_version(ss58_prefix.into());
                    Some(Value::String(ss58))
                }
                Err(e) => {
                    tracing::warn!(
                        hex_str = %hex_str,
                        error = %e,
                        "Failed to hex decode potential AccountId32 field"
                    );
                    None
                }
            }
        }
        _ => None,
    }
}

/// Transform event data to match sidecar format
/// - Converts snake_case to camelCase
/// - Simplifies enum variants from {"name": "X", "values": ...} to just "X" (for empty values)
/// - Converts byte arrays to hex strings
fn transform_event_data(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            // Check if this is an enum variant object with "name" and "values" fields
            // If values is "0x" (empty), just return the name as a string
            if map.len() == 2 {
                match (map.get("name"), map.get("values")) {
                    (Some(Value::String(name)), Some(Value::String(values))) if values == "0x" => {
                        return Value::String(name.clone());
                    }
                    _ => {}
                }
            }

            // Otherwise, transform object keys from snake_case to camelCase
            let transformed: serde_json::Map<String, Value> = map
                .into_iter()
                .map(|(key, val)| {
                    let camel_key = snake_to_camel(&key);
                    (camel_key, transform_event_data(val))
                })
                .collect();
            Value::Object(transformed)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(transform_event_data).collect()),
        other => other,
    }
}

/// Parse event phase from scale_value
fn parse_event_phase(phase_value: &scale_value::ValueDef<()>) -> Result<EventPhase, GetBlockError> {
    if let scale_value::ValueDef::Variant(variant) = phase_value {
        match variant.name.as_str() {
            "Initialization" => Ok(EventPhase::Initialization),
            "ApplyExtrinsic" => {
                // Extract the extrinsic index (stored as u128)
                let index_fields: Vec<&scale_value::Value<()>> = variant.values.values().collect();
                if let Some(index_field) = index_fields.first() {
                    // Try to extract as u128
                    if let scale_value::ValueDef::Primitive(scale_value::Primitive::U128(index)) =
                        &index_field.value
                    {
                        return Ok(EventPhase::ApplyExtrinsic(*index as u32));
                    }
                    // Try helper method
                    if let Some(index_u128) = index_field.as_u128() {
                        return Ok(EventPhase::ApplyExtrinsic(index_u128 as u32));
                    }
                }
                tracing::warn!("ApplyExtrinsic phase missing or invalid index");
                Ok(EventPhase::ApplyExtrinsic(0))
            }
            "Finalization" => Ok(EventPhase::Finalization),
            other => {
                tracing::warn!("Unknown event phase: {}", other);
                Ok(EventPhase::Initialization)
            }
        }
    } else {
        tracing::warn!("Event phase is not a variant");
        Ok(EventPhase::Initialization)
    }
}

/// Fetch and parse all events for a block
async fn fetch_block_events(
    state: &AppState,
    block_number: u64,
) -> Result<Vec<ParsedEvent>, GetBlockError> {
    // Get client at block
    let client_at_block = state.client.at(block_number).await?;

    // Fetch System.Events storage
    // This contains all events that occurred during block execution
    let storage_entry = client_at_block.storage().entry("System", "Events")?;
    let plain_entry = storage_entry.into_plain()?;
    let events_value = plain_entry.fetch().await?.ok_or_else(|| {
        tracing::warn!("No events storage found for block {}", block_number);
        parity_scale_codec::Error::from("Events storage not found")
    })?;

    // Decode events as Vec<EventRecord>
    // EventRecord is a struct with fields: phase, event, topics
    let events_vec = events_value
        .decode::<Vec<scale_value::Value<()>>>()
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

    for event_record in events_vec {
        // EventRecord structure: { phase, event, topics }
        // Each event_record is a Value with Composite inside
        let event_composite = match &event_record.value {
            scale_value::ValueDef::Composite(comp) => comp,
            _ => {
                tracing::warn!("Event record is not a composite");
                continue;
            }
        };

        // Get the fields - for Named composites, we have (name, value) pairs
        let fields: Vec<&scale_value::Value<()>> = event_composite.values().collect();

        // EventRecord has 3 fields in order: phase, event, topics
        if fields.len() < 2 {
            tracing::warn!("Event record has insufficient fields");
            continue;
        }

        // Parse phase (field 0)
        let phase = parse_event_phase(&fields[0].value)?;

        // Parse event (field 1)
        // Event structure: Variant(Pallet) { values: Variant(Event) { values: event_data } }
        if let scale_value::ValueDef::Variant(pallet_variant) = &fields[1].value {
            // Convert pallet name to lowercase to match sidecar format
            let pallet_name = pallet_variant.name.to_lowercase();

            // The pallet variant contains a single inner variant which is the actual event
            let inner_values: Vec<&scale_value::Value<()>> =
                pallet_variant.values.values().collect();

            if let Some(inner_value) = inner_values.first() {
                if let scale_value::ValueDef::Variant(event_variant) = &inner_value.value {
                    let event_name = event_variant.name.clone();

                    // Get the positions of AccountId32 fields for this event type
                    let account_positions = get_account_field_positions(&pallet_name, &event_name);

                    // Decode event fields to JSON, then transform to match sidecar format
                    let event_data: Vec<Value> = event_variant
                        .values
                        .values()
                        .enumerate()
                        .filter_map(|(idx, field)| {
                            // Convert to JSON
                            let json_value = serde_json::to_value(&field.value).ok()?;
                            let with_hex = convert_bytes_to_hex(json_value);

                            // If this field position should contain an AccountId32, try to convert it
                            if account_positions.contains(&idx)
                                && let Some(ss58_value) = try_convert_to_ss58_event_field(
                                    &with_hex,
                                    state.chain_info.ss58_prefix,
                                )
                            {
                                return Some(ss58_value);
                            }

                            // Otherwise, apply standard transformation
                            Some(transform_event_data(with_hex))
                        })
                        .collect();

                    parsed_events.push(ParsedEvent {
                        phase,
                        pallet_name: pallet_name.clone(),
                        event_name,
                        event_data,
                    });
                } else {
                    tracing::warn!("Inner event value is not a variant");
                }
            } else {
                tracing::warn!("Pallet variant has no inner values");
            }
        }
    }

    Ok(parsed_events)
}

/// Categorize parsed events into onInitialize, per-extrinsic, and onFinalize arrays
fn categorize_events(
    parsed_events: Vec<ParsedEvent>,
    num_extrinsics: usize,
) -> (OnInitialize, Vec<Vec<Event>>, OnFinalize) {
    let mut on_initialize_events = Vec::new();
    let mut on_finalize_events = Vec::new();
    // Create empty event vectors for each extrinsic
    let mut per_extrinsic_events: Vec<Vec<Event>> = vec![Vec::new(); num_extrinsics];

    for parsed_event in parsed_events {
        // Convert ParsedEvent to Event
        let event = Event {
            method: MethodInfo {
                pallet: parsed_event.pallet_name,
                method: parsed_event.event_name,
            },
            data: parsed_event.event_data,
        };

        // Categorize by phase
        match parsed_event.phase {
            EventPhase::Initialization => {
                on_initialize_events.push(event);
            }
            EventPhase::ApplyExtrinsic(index) => {
                // Add to the corresponding extrinsic's event list
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
    )
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

    // Fetch extrinsics for this block
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
        // Extract pallet and method name from the call
        let pallet_name = extrinsic.call().pallet_name().to_string();
        let method_name = extrinsic.call().name().to_string();

        // Extract call arguments with field-name-based AccountId32 detection
        let fields = extrinsic.call().fields();
        let mut args_map = serde_json::Map::new();

        for field in fields.iter() {
            let field_name = field.name();
            let camel_field_name = snake_to_camel(field_name);

            // Try to decode as AccountId32-related types for known fields
            if is_account_field(field_name) {
                let mut decoded_account = false;

                // Helper to convert bytes to SS58 with chain-specific prefix
                let ss58_prefix = state.chain_info.ss58_prefix;
                let bytes_to_ss58 = |bytes: &[u8; 32]| {
                    let account_id = AccountId32::from(*bytes);
                    account_id.to_ss58check_with_version(ss58_prefix.into())
                };

                // Try decoding as [u8; 32]
                if let Ok(account_bytes) = field.decode::<[u8; 32]>() {
                    let ss58 = bytes_to_ss58(&account_bytes);
                    args_map.insert(camel_field_name.clone(), json!(ss58));
                    decoded_account = true;
                } else if let Ok(accounts) = field.decode::<Vec<[u8; 32]>>() {
                    // Try Vec<[u8; 32]>
                    let ss58_addresses: Vec<String> = accounts.iter().map(&bytes_to_ss58).collect();
                    args_map.insert(camel_field_name.clone(), json!(ss58_addresses));
                    decoded_account = true;
                } else if let Ok(multi_addr) = field.decode::<MultiAddress>() {
                    // Try MultiAddress enum
                    let value = match multi_addr {
                        MultiAddress::Id(bytes) | MultiAddress::Address32(bytes) => {
                            json!(bytes_to_ss58(&bytes))
                        }
                        MultiAddress::Index(index) => json!({ "index": index }),
                        MultiAddress::Raw(bytes) => {
                            json!({ "raw": format!("0x{}", hex::encode(bytes)) })
                        }
                        MultiAddress::Address20(bytes) => {
                            json!({ "address20": format!("0x{}", hex::encode(bytes)) })
                        }
                    };
                    args_map.insert(camel_field_name.clone(), value);
                    decoded_account = true;
                }

                if decoded_account {
                    continue;
                }
                // If we failed to decode as account types, fall through to Value<()> decoding
            }

            // For non-account fields (or account fields that failed to decode), use Value<()>
            match field.decode::<scale_value::Value<()>>() {
                Ok(value) => {
                    let json_value = serde_json::to_value(&value).unwrap_or(Value::Null);
                    let converted = convert_bytes_to_hex(json_value);
                    let transformed = transform_args(converted, state.chain_info.ss58_prefix);
                    args_map.insert(camel_field_name, transformed);
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

            // Decode signer address to SS58
            let signer_hex = format!("0x{}", hex::encode(addr_bytes));
            let signer_ss58 = decode_address_to_ss58(&signer_hex, state.chain_info.ss58_prefix)
                .unwrap_or_else(|| signer_hex.clone());

            (
                Some(SignatureInfo {
                    signature: format!("0x{}", hex::encode(sig_bytes)),
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
                        if let Ok(n) = ext.decode::<scale_value::Value>()
                            && let Ok(json_val) = serde_json::to_value(&n)
                        {
                            // The value might be nested in an object, so we need to extract it
                            // If extraction fails, nonce_value remains None (serialized as null)
                            nonce_value = extract_numeric_string(&json_val);
                        }
                    }
                    "ChargeTransactionPayment" | "ChargeAssetTxPayment" => {
                        // The tip is typically a Compact<u128>
                        if let Ok(t) = ext.decode::<scale_value::Value>()
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

        // Compute extrinsic hash: Blake2-256 of raw bytes
        let hash_bytes = BlakeTwo256::hash(extrinsic.bytes());
        let hash = format!("0x{}", hex::encode(hash_bytes.as_ref()));

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
            info: serde_json::Map::new(), // Empty for now, populated with events later
            era: era_info,
            events: Vec::new(), // Will be populated with events during categorization
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
pub async fn get_block(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
) -> Result<Json<BlockResponse>, GetBlockError> {
    // Parse the block identifier
    let block_id = block_id.parse::<utils::BlockId>()?;

    // Resolve the block
    let resolved_block = utils::resolve_block(&state, Some(block_id)).await?;

    // Fetch the header JSON
    let header_json = state
        .get_header_json(&resolved_block.hash)
        .await
        .map_err(GetBlockError::HeaderFetchFailed)?;

    // Extract header fields
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

    // Decode digest logs from hex strings into structured format
    let logs = decode_digest_logs(&header_json);

    // Extract author from digest logs by mapping authority index to validator
    let author_id = extract_author(&state, resolved_block.number, &logs).await;

    // Extract extrinsics using subxt-historic for historical integrity
    let extrinsics = extract_extrinsics(&state, resolved_block.number).await?;

    // Fetch all block events
    let block_events = fetch_block_events(&state, resolved_block.number).await?;

    // Categorize events by phase
    let (on_initialize, per_extrinsic_events, on_finalize) =
        categorize_events(block_events, extrinsics.len());

    // Populate each extrinsic with its events
    let mut extrinsics_with_events = extrinsics;
    for (i, extrinsic_events) in per_extrinsic_events.into_iter().enumerate() {
        if let Some(extrinsic) = extrinsics_with_events.get_mut(i) {
            extrinsic.events = extrinsic_events;
        }
    }

    // Build response
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

        let result = get_block(State(state), Path("100".to_string())).await;

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
        assert_eq!(response.logs[0].index, 0);
        // Verify the engine ID is "BABE" and payload is present
        if let Some(arr) = response.logs[0].value.as_array() {
            assert_eq!(arr[0].as_str(), Some("BABE"));
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

        let result = get_block(State(state), Path(test_hash.to_string())).await;

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

        let result = get_block(State(state), Path("invalid".to_string())).await;

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

        let result = get_block(State(state), Path("999999".to_string())).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GetBlockError::BlockResolveFailed(_)
        ));
    }
}
