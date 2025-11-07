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
use sp_core::crypto::AccountId32;
use sp_runtime::traits::BlakeTwo256;
use sp_runtime::traits::Hash as HashT;
use subxt_historic::error::{OnlineClientAtBlockError, StorageEntryIsNotAPlainValue, StorageError};
use thiserror::Error;

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

/// Represents a decoded digest log entry
#[derive(Debug, Serialize)]
pub struct DigestLog {
    #[serde(rename = "type")]
    pub log_type: String,
    pub index: u32,
    pub value: Value,
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

/// Length of consensus engine ID in digest items (e.g., "BABE", "aura", "pow_")
const CONSENSUS_ENGINE_ID_LEN: usize = 4;

/// Format bytes as hex string with "0x" prefix
fn hex_with_prefix(data: &[u8]) -> String {
    format!("0x{}", hex::encode(data))
}

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

/// Method information for extrinsic calls
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodInfo {
    pub pallet: String,
    pub method: String,
}

/// Signature information for signed extrinsics
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInfo {
    pub signature: String,
    pub signer: String,
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
    // TODO: Add more fields (events, success, paysFee)
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
    pub extrinsics: Vec<ExtrinsicInfo>,
    // TODO: Add more fields (onInitialize, onFinalize, etc.)
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

/// Extract extrinsics from a block using subxt-historic
async fn extract_extrinsics(
    state: &AppState,
    block_number: u64,
) -> Result<Vec<ExtrinsicInfo>, GetBlockError> {
    // Use subxt-historic to get a client at the specific block height
    // This ensures we use the correct metadata for that block
    let client_at_block = match state.client.at(block_number).await {
        Ok(client) => client,
        Err(_) => {
            // If we can't get the client at the block (e.g., in tests with mock RPC),
            // return empty extrinsics. In production with real chains, this will work.
            return Ok(Vec::new());
        }
    };

    // Fetch extrinsics for this block
    let extrinsics = match client_at_block.extrinsics().fetch().await {
        Ok(exts) => exts,
        Err(_) => {
            // If fetching fails, return empty (graceful degradation)
            return Ok(Vec::new());
        }
    };

    let mut result = Vec::new();

    for extrinsic in extrinsics.iter() {
        // Extract pallet and method name from the call
        let pallet_name = extrinsic.call().pallet_name().to_string();
        let method_name = extrinsic.call().name().to_string();

        // Extract call arguments
        // We decode into scale_value::Composite which can represent any SCALE type
        let args_composite = extrinsic
            .call()
            .fields()
            .decode::<scale_value::Composite<()>>()
            .map_err(|e| {
                GetBlockError::ExtrinsicDecodeFailed(format!("Failed to decode args: {}", e))
            })?;

        // Convert to JSON using serde
        let args_json = serde_json::to_value(&args_composite).map_err(|e| {
            GetBlockError::ExtrinsicDecodeFailed(format!("Failed to serialize args: {}", e))
        })?;

        // Convert byte arrays to hex strings
        let args_converted = convert_bytes_to_hex(args_json);

        // Extract as map (should be an object from Composite)
        let args_map = if let Value::Object(map) = args_converted {
            map
        } else {
            // Fallback to empty map if not an object
            serde_json::Map::new()
        };

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

            (
                Some(SignatureInfo {
                    signature: format!("0x{}", hex::encode(sig_bytes)),
                    signer: format!("0x{}", hex::encode(addr_bytes)),
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
        });
    }

    Ok(result)
}

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

    // Build response
    let response = BlockResponse {
        number: resolved_block.number.to_string(),
        hash: resolved_block.hash,
        parent_hash,
        state_root,
        extrinsics_root,
        author_id,
        logs,
        extrinsics,
    };

    Ok(Json(response))
}

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
