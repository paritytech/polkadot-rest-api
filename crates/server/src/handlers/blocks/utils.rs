// Utility functions for blocks handlers

use crate::state::AppState;
use crate::utils::{
    self, EraInfo,
    rc_block::{as_composite, get_field_from_composite},
};
use parity_scale_codec::Decode;
use scale_value::{Value as ScaleValue, ValueDef, Composite};
use serde::Serialize;
use serde_json::{Value, json};
use sp_core::crypto::{AccountId32, Ss58Codec};
use sp_runtime::generic::DigestItem;
use sp_runtime::traits::BlakeTwo256;
use subxt_rpcs::rpc_params;
use sp_runtime::traits::Hash as HashT;

use super::get_block::GetBlockError;

/// Represents a digest log entry for JSON serialization
#[derive(Debug, Clone, Serialize)]
pub struct DigestLog {
    #[serde(rename = "type")]
    pub log_type: String,
    #[serde(serialize_with = "serialize_index_as_string")]
    pub index: u32,
    pub value: Value,
    #[serde(skip)]
    pub original_bytes: Option<Vec<u8>>,
}

/// Serialize index as string instead of number
fn serialize_index_as_string<S>(index: &u32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&index.to_string())
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
    pub signature: Option<SignatureInfo>,
    pub nonce: Option<String>,
    pub args: serde_json::Map<String, Value>,
    pub tip: Option<String>,
    pub hash: String,
    pub info: serde_json::Map<String, Value>,
    pub era: EraInfo,
    pub events: Vec<serde_json::Value>,
    pub success: bool,
    pub pays_fee: bool,
}

/// Decode digest logs from hex-encoded strings in a JSON header
pub fn decode_digest_logs(header_json: &Value) -> Vec<DigestLog> {
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
            let hex_data = hex_str.strip_prefix("0x").unwrap_or(hex_str);
            let bytes = hex::decode(hex_data).ok()?;

            if bytes.is_empty() {
                return None;
            }

            let mut cursor = &bytes[..];
            let digest_item = match DigestItem::decode(&mut cursor) {
                Ok(item) => item,
                Err(e) => {
                    tracing::warn!("Failed to decode digest item: {:?}", e);
                    return None;
                }
            };

            let (log_type, value, original_bytes) = match &digest_item {
                DigestItem::PreRuntime(engine_id, data) => {
                    let engine_id_hex = format!("0x{}", hex::encode(engine_id));
                    let payload_hex = format!("0x{}", hex::encode(data));
                    ("PreRuntime".to_string(), json!([engine_id_hex, payload_hex]), Some(data.clone()))
                }
                DigestItem::Consensus(engine_id, data) => {
                    let engine_id_hex = format!("0x{}", hex::encode(engine_id));
                    let payload_hex = format!("0x{}", hex::encode(data));
                    ("Consensus".to_string(), json!([engine_id_hex, payload_hex]), None)
                }
                DigestItem::Seal(engine_id, data) => {
                    let engine_id_hex = format!("0x{}", hex::encode(engine_id));
                    let payload_hex = format!("0x{}", hex::encode(data));
                    ("Seal".to_string(), json!([engine_id_hex, payload_hex]), None)
                }
                DigestItem::Other(data) => {
                    let data_hex = format!("0x{}", hex::encode(data));
                    ("Other".to_string(), json!(data_hex), None)
                }
                DigestItem::RuntimeEnvironmentUpdated => {
                    ("RuntimeEnvironmentUpdated".to_string(), Value::Null, None)
                }
            };

            let index = match digest_item {
                DigestItem::Other(_) => 0,
                DigestItem::Consensus(_, _) => 4,
                DigestItem::Seal(_, _) => 5,
                DigestItem::PreRuntime(_, _) => 6,
                DigestItem::RuntimeEnvironmentUpdated => 8,
            };

            Some(DigestLog {
                log_type,
                index: index as u32,
                value,
                original_bytes,
            })
        })
        .collect()
}

/// Extract digest from header JSON (for header-only responses)
pub fn extract_digest_from_header(header_json: &serde_json::Value) -> crate::utils::DigestInfo {
    use crate::utils::DigestLog as RcDigestLog;
    
    let logs = header_json
        .get("digest")
        .and_then(|d| d.get("logs"))
        .and_then(|l| l.as_array())
        .map(|logs_arr| {
            logs_arr
                .iter()
                .filter_map(|log_hex| {
                    // Logs from RPC are hex-encoded strings, need to decode them
                    let hex_str = log_hex.as_str()?;
                    let hex_data = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                    let bytes = hex::decode(hex_data).ok()?;

                    if bytes.is_empty() {
                        return None;
                    }

                    let mut cursor = &bytes[..];
                    let digest_item = match DigestItem::decode(&mut cursor) {
                        Ok(item) => item,
                        Err(_) => return None,
                    };

                    // Convert to DigestLog format matching TypeScript sidecar
                    match digest_item {
                        DigestItem::PreRuntime(engine_id, data) => {
                            Some(RcDigestLog {
                                pre_runtime: Some((
                                    format!("0x{}", hex::encode(engine_id)),
                                    format!("0x{}", hex::encode(data)),
                                )),
                                consensus: None,
                                seal: None,
                                other: None,
                            })
                        }
                        DigestItem::Consensus(engine_id, data) => {
                            Some(RcDigestLog {
                                pre_runtime: None,
                                consensus: Some((
                                    format!("0x{}", hex::encode(engine_id)),
                                    format!("0x{}", hex::encode(data)),
                                )),
                                seal: None,
                                other: None,
                            })
                        }
                        DigestItem::Seal(engine_id, data) => {
                            Some(RcDigestLog {
                                pre_runtime: None,
                                consensus: None,
                                seal: Some((
                                    format!("0x{}", hex::encode(engine_id)),
                                    format!("0x{}", hex::encode(data)),
                                )),
                                other: None,
                            })
                        }
                        DigestItem::Other(data) => {
                            Some(RcDigestLog {
                                pre_runtime: None,
                                consensus: None,
                                seal: None,
                                other: Some(format!("0x{}", hex::encode(data))),
                            })
                        }
                        DigestItem::RuntimeEnvironmentUpdated => None,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    crate::utils::DigestInfo { logs }
}

/// Extract engine ID and payload from a digest log
pub fn extract_engine_and_payload(log: &DigestLog) -> Option<(Vec<u8>, Vec<u8>)> {
    let payload = log.original_bytes.clone().or_else(|| {
        log.value.as_array()?
            .get(1)?
            .as_str()
            .and_then(|s| hex::decode(s.strip_prefix("0x").unwrap_or(s)).ok())
    })?;

    let engine_id_bytes = log.value.as_array()?
        .get(0)?
        .as_str()
        .and_then(|s| {
            if s.starts_with("0x") {
                hex::decode(s.strip_prefix("0x")?).ok()
            } else {
                Some(s.as_bytes().to_vec())
            }
        })?;

    Some((engine_id_bytes, payload))
}

/// Extract header fields from JSON header
pub fn extract_header_fields(header_json: &serde_json::Value) -> Result<(String, String, String, Vec<DigestLog>), GetBlockError> {
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

    let logs = decode_digest_logs(header_json);

    Ok((parent_hash, state_root, extrinsics_root, logs))
}

/// Check if a block is finalized by comparing with finalized head
pub async fn is_block_finalized(rpc_client: &subxt_rpcs::RpcClient, block_hash: &str) -> Result<bool, subxt_rpcs::Error> {
    let finalized_hash = rpc_client
        .request::<Option<String>>("chain_getFinalizedHead", rpc_params![])
        .await?;
    
    Ok(finalized_hash.as_deref() == Some(block_hash))
}

/// Convert string to camelCase
pub fn to_camel_case(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    
    if s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars();
        if let Some(first) = chars.next() {
            result.push(first.to_lowercase().next().unwrap_or(first));
        }
        for c in chars {
            result.push(c);
        }
        return result;
    }
    
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_uppercase().next().unwrap_or(c));
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    
    result
}

/// Find RC block number for an AH block by extracting from parachainSystem.setValidationData
pub async fn find_rc_block_for_ah_block(
    state: &AppState,
    ah_block_number: u64,
) -> Option<u64> {
    // Get extrinsics for this block
    let client_at_block = state.client.at(ah_block_number).await.ok()?;
    let extrinsics = client_at_block.extrinsics().fetch().await.ok()?;
    
    // Find the specific extrinsic using iterator
    let target_extrinsic = extrinsics.iter().find(|ext| {
        ext.call().pallet_name() == "ParachainSystem" 
        && ext.call().name() == "set_validation_data"
    })?;
    
    let args = target_extrinsic.call()
        .fields()
        .decode::<scale_value::Composite<()>>()
        .ok()?;
    
    let data_value = get_field_from_composite(&args, &["data"], Some(0))?;
    let data_composite = as_composite(data_value)?;
    
    let validation_data_value = get_field_from_composite(
        data_composite,
        &["validationData", "validation_data"],
        Some(0)
    )?;
    let validation_data_composite = as_composite(validation_data_value)?;
    
    let relay_parent_number_value = get_field_from_composite(
        validation_data_composite,
        &["relayParentNumber", "relay_parent_number"],
        Some(1) // Usually second field after parentHead
    )?;
    
    serde_json::to_value(relay_parent_number_value)
        .ok()
        .and_then(|json| {
            json.as_u64()
                .or_else(|| json.as_array()?.first()?.as_u64())
                .or_else(|| json.as_object()?.values().next()?.as_u64())
        })
}

/// Get validators for Asset Hub block
async fn get_ah_validators(
    state: &AppState,
    block_number: u64,
) -> Result<Vec<AccountId32>, GetBlockError> {
    use parity_scale_codec::Decode;

    let block_hash: String = state.rpc_client
        .request("chain_getBlockHash", rpc_params![block_number])
        .await
        .map_err(|e| GetBlockError::HeaderFetchFailed(e))?;

    // Use state_getStorage with explicit block hash to get historical Aura::Authorities
    // Storage key for Aura::Authorities: 0x57f8dc2f5ab09467896f47300f0424385e0621c4869aa60c02be9adcc98a0d1d
    let aura_authorities_key = "0x57f8dc2f5ab09467896f47300f0424385e0621c4869aa60c02be9adcc98a0d1d";
    
    let storage_result: Option<String> = state.rpc_client
        .request("state_getStorage", rpc_params![aura_authorities_key, &block_hash])
        .await
        .map_err(|e| GetBlockError::HeaderFetchFailed(e))?;
    
    if let Some(storage_hex) = storage_result {
        let storage_bytes = hex::decode(storage_hex.trim_start_matches("0x"))
            .map_err(|_| GetBlockError::HeaderFieldMissing("Invalid hex in storage".to_string()))?;
        
        let validators: Vec<AccountId32> = Vec::<AccountId32>::decode(&mut &storage_bytes[..])
            .map_err(|e| GetBlockError::HeaderFieldMissing(format!("Failed to decode validators: {}", e)))?;
        
        if !validators.is_empty() {
            return Ok(validators);
        }
    }
    
    // Fallback to Session::Validators via RPC
    // Storage key for Session::Validators: 0xcec5070d609dd3497f72bde07fc96ba0726380404683fc89e8233450c8aa19505ffb64e1c6068bfea
    let session_validators_key = "0xcec5070d609dd3497f72bde07fc96ba0726380404683fc89e8233450c8aa19505ffb64e1c6068bfea";
    
    let storage_result: Option<String> = state.rpc_client
        .request("state_getStorage", rpc_params![session_validators_key, &block_hash])
        .await
        .map_err(|e| GetBlockError::HeaderFetchFailed(e))?;
    
    if let Some(storage_hex) = storage_result {
        let storage_bytes = hex::decode(storage_hex.trim_start_matches("0x"))
            .map_err(|_| GetBlockError::HeaderFieldMissing("Invalid hex in storage".to_string()))?;
        
        let validators: Vec<AccountId32> = Vec::<AccountId32>::decode(&mut &storage_bytes[..])
            .map_err(|e| GetBlockError::HeaderFieldMissing(format!("Failed to decode validators: {}", e)))?;
        
        if !validators.is_empty() {
            return Ok(validators);
        }
    }

    Err(GetBlockError::HeaderFieldMissing("No validators found in storage".to_string()))
}

/// Get validators at a specific block (RC validators for parachains, AH validators otherwise)
pub async fn get_validators_at_block(
    state: &AppState,
    block_number: u64,
) -> Result<Vec<AccountId32>, GetBlockError> {
    use parity_scale_codec::Decode;

    use config::ChainType;
    if state.chain_info.chain_type == ChainType::AssetHub || 
       state.chain_info.chain_type == ChainType::Parachain {
        if let Ok(rc_client) = state.get_relay_chain_subxt_client().await {
            let rc_block_number = find_rc_block_for_ah_block(state, block_number).await
                .unwrap_or(block_number);
            
            let rc_client_at_block = (*rc_client).at(rc_block_number).await?;
            let storage_entry = rc_client_at_block.storage().entry("Session", "Validators")?;
            let plain_entry = storage_entry.into_plain()?;
            let validators_value = plain_entry.fetch().await?.ok_or_else(|| {
                parity_scale_codec::Error::from("validators storage not found")
            })?;
            let raw_bytes = validators_value.into_bytes();
            let validators_raw: Vec<[u8; 32]> = Vec::<[u8; 32]>::decode(&mut &raw_bytes[..])?;
            let validators: Vec<AccountId32> = validators_raw.into_iter().map(AccountId32::from).collect();

            if validators.is_empty() {
                return Err(parity_scale_codec::Error::from("no validators found in storage").into());
            }

            return Ok(validators);
        }
    }

    get_ah_validators(state, block_number).await
}

/// Extract block author from digest logs
pub async fn extract_author(
    state: &AppState,
    block_number: u64,
    logs: &[DigestLog],
) -> Option<String> {
    use parity_scale_codec::{Compact, Decode};
    use sp_consensus_babe::digests::PreDigest;

    const BABE_ENGINE: &[u8] = b"BABE";
    const AURA_ENGINE: &[u8] = b"aura";
    const POW_ENGINE: &[u8] = b"pow_";

    let validators = get_ah_validators(state, block_number).await.ok()?;

    for log in logs.iter() {
        if log.log_type != "PreRuntime" {
            continue;
        }

        let (engine_id_bytes, payload) = match extract_engine_and_payload(log) {
            Some((engine, payload)) => (engine, payload),
            None => {
                continue;
            }
        };
        
        let engine_slice = match engine_id_bytes.get(..4) {
            Some(slice) => slice,
            None => continue,
        };
        
        match engine_slice {
            _ if engine_slice == BABE_ENGINE => {
                if payload.is_empty() {
                    continue;
                }

                let mut cursor = &payload[..];
                let pre_digest = PreDigest::decode(&mut cursor).ok()?;
                let authority_index = pre_digest.authority_index() as usize;
                
                if let Some(author) = validators.get(authority_index) {
                    return Some(author.to_ss58check_with_version(sp_core::crypto::Ss58AddressFormat::custom(0)));
                } else {
                    return None;
                }
            }
            _ if engine_slice == AURA_ENGINE => {
                let slot = if payload.len() >= 8 {
                    u64::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                        payload[4], payload[5], payload[6], payload[7],
                    ])
                } else {
                    let mut cursor = &payload[..];
                    if let Ok(compact_slot) = Compact::<u64>::decode(&mut cursor) {
                        compact_slot.0
                    } else {
                        cursor = &payload[..];
                        u64::decode(&mut cursor).ok()?
                    }
                };

                let index = (slot as usize) % validators.len();
                
                if let Some(author) = validators.get(index) {
                    return Some(author.to_ss58check_with_version(sp_core::crypto::Ss58AddressFormat::custom(0)));
                } else {
                    return None;
                }
            }
            _ => continue,
        }
    }

    for log in logs.iter() {
        if log.log_type != "Consensus" {
            continue;
        }

        let (engine_id_bytes, payload) = extract_engine_and_payload(log)?;
        
        if engine_id_bytes.as_slice() == POW_ENGINE && payload.len() >= 32 {
            let mut account_bytes = [0u8; 32];
            account_bytes.copy_from_slice(&payload[..32]);
            let account_id = AccountId32::from(account_bytes);
            return Some(account_id.to_ss58check_with_version(sp_core::crypto::Ss58AddressFormat::custom(0)));
        }
    }

    None
}

/// Restructure args for parachainSystem.setValidationData to match expected format
pub fn restructure_parachain_validation_data_args(mut args_map: serde_json::Map<String, Value>) -> serde_json::Map<String, Value> {

    let mut data_obj = if let Some(Value::Object(existing_data)) = args_map.remove("data") {
        existing_data
    } else {
        serde_json::Map::new()
    };

    let mut validation_data = if let Some(Value::Object(existing_vd)) = data_obj.remove("validationData") {
        existing_vd
    } else if let Some(Value::Object(existing_vd_snake)) = data_obj.remove("validation_data") {
        existing_vd_snake.into_iter()
            .map(|(k, v)| (to_camel_case(&k), v))
            .collect()
    } else {
        serde_json::Map::new()
    };

    if validation_data.is_empty() {
        for (key, value) in args_map.iter() {
            let camel_key = to_camel_case(key);
            match camel_key.as_str() {
                "parentHead" | "relayParentNumber" | "relayParentStorageRoot" | "maxPovSize" => {
                    validation_data.insert(camel_key, value.clone());
                }
                "relayChainState" | "downwardMessages" | "horizontalMessages" => {
                    data_obj.insert(camel_key, value.clone());
                }
                _ if !validation_data.contains_key(&camel_key) 
                    && !["relayChainState", "downwardMessages", "horizontalMessages"].contains(&camel_key.as_str()) => {
                    validation_data.insert(camel_key, value.clone());
                }
                _ => {}
            }
        }
    }

    data_obj = data_obj.into_iter()
        .map(|(k, v)| {
            let camel_key = to_camel_case(&k);
            let converted_value = if let Value::Object(nested_obj) = v {
                Value::Object(nested_obj.into_iter()
                    .map(|(nk, nv)| (to_camel_case(&nk), nv))
                    .collect())
            } else {
                v
            };
            (camel_key, converted_value)
        })
        .collect();

    if let Some(Value::String(dm)) = data_obj.get("downwardMessages") {
        if dm == "0x" {
            data_obj.insert("downwardMessages".to_string(), Value::Array(vec![]));
        }
    }
    if let Some(Value::String(hm)) = data_obj.get("horizontalMessages") {
        if hm == "0x" {
            data_obj.insert("horizontalMessages".to_string(), Value::Object(serde_json::Map::new()));
        }
    }

    data_obj.insert("validationData".to_string(), Value::Object(validation_data));
    let mut restructured = serde_json::Map::new();
    restructured.insert("data".to_string(), Value::Object(data_obj));
    restructured
}

/// Convert args to sidecar format
pub fn convert_args_to_sidecar_format(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(n) => serde_json::Value::String(n.to_string()),
        serde_json::Value::Array(arr) => {
            // Check if byte array (all numbers 0-255)
            if arr.iter().all(|v| v.as_u64().map(|n| n <= 255).unwrap_or(false)) {
                let bytes: Vec<u8> = arr.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect();
                serde_json::Value::String(format!("0x{}", hex::encode(&bytes)))
            } else {
                let converted: Vec<serde_json::Value> = arr.into_iter()
                    .map(convert_args_to_sidecar_format)
                    .collect();
                if converted.len() == 1 {
                    converted.into_iter().next().unwrap()
                } else {
                    serde_json::Value::Array(converted)
                }
            }
        }
        serde_json::Value::Object(mut map) => {
            map.values_mut().for_each(|v| *v = convert_args_to_sidecar_format(v.clone()));
            serde_json::Value::Object(map)
        }
        other => other,
    }
}

/// Convert event to sidecar format
pub fn convert_event_to_sidecar_format(
    event_val: &ScaleValue,
    to_camel_case_fn: impl Fn(&str) -> String,
) -> serde_json::Value {
    let event_json = serde_json::to_value(event_val).unwrap_or(serde_json::Value::Null);
    
    let pallet_name = event_json.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    
    let event_inner = event_json.get("values")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.get(0));
    
    if let Some(inner_val) = event_inner {
        let event_name = inner_val.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let data = if event_name == "ExtrinsicSuccess" {
            let dispatch_info = inner_val.get("values")
                .and_then(|v| v.as_object())
                .and_then(|obj| obj.get("dispatch_info"))
                .and_then(|v| v.as_object());
            
            if let Some(dispatch_info) = dispatch_info {
                let mut converted_data = serde_json::Map::new();
                
                dispatch_info.get("weight")
                    .map(|w| w.as_u64().map(|n| n.to_string())
                        .or_else(|| w.as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| w.to_string()))
                    .map(|s| converted_data.insert("weight".to_string(), serde_json::Value::String(s)));
                
                dispatch_info.get("class")
                    .and_then(|v| v.as_object())
                    .and_then(|obj| obj.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| converted_data.insert("class".to_string(), serde_json::Value::String(s.to_string())));
                
                dispatch_info.get("pays_fee")
                    .and_then(|v| v.as_object())
                    .and_then(|obj| obj.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|s| converted_data.insert("paysFee".to_string(), serde_json::Value::String(s.to_string())));
                
                if converted_data.is_empty() {
                    vec![serde_json::to_value(inner_val).unwrap_or(serde_json::Value::Null)]
                } else {
                    vec![serde_json::Value::Object(converted_data)]
                }
            } else {
                vec![serde_json::to_value(inner_val).unwrap_or(serde_json::Value::Null)]
            }
        } else {
            vec![serde_json::to_value(inner_val).unwrap_or(serde_json::Value::Null)]
        };
        
        json!({
            "method": {
                "pallet": to_camel_case_fn(pallet_name),
                "method": event_name
            },
            "data": data
        })
    } else {
        event_json
    }
}

/// Extract events for a specific extrinsic
pub fn extract_extrinsic_events(
    events_value: &ScaleValue,
    extrinsic_index: u32,
    to_camel_case_fn: impl Fn(&str) -> String,
) -> (Vec<serde_json::Value>, bool, bool) {
    let events_composite = match as_composite(events_value) {
        Some(Composite::Unnamed(values)) => values,
        _ => return (Vec::new(), true, false),
    };
    
    let mut extrinsic_events = Vec::new();
    let mut success = true;
    let mut pays_fee = false;
    
    for event_record in events_composite.iter() {
        let record_composite = match as_composite(event_record) {
            Some(c) => c,
            None => continue,
        };
        
        let phase_value = get_field_from_composite(record_composite, &["phase"], Some(0));
        
        let is_our_extrinsic = phase_value
            .and_then(|v| {
                if let ValueDef::Variant(variant) = &v.value {
                    if variant.name == "ApplyExtrinsic" {
                        if let Composite::Unnamed(values) = &variant.values {
                            return values.get(0)
                                .and_then(|idx_val| serde_json::to_value(idx_val).ok())
                                .and_then(|v| v.as_u64())
                                .map(|idx| idx == extrinsic_index as u64);
                        }
                    }
                }
                None
            })
            .unwrap_or(false);
        
        if !is_our_extrinsic {
            continue;
        }
        
        // Extract event
        let event_val = get_field_from_composite(record_composite, &["event"], Some(1));
        
        if let Some(event_val) = event_val {
            let converted_event = convert_event_to_sidecar_format(event_val, &to_camel_case_fn);
            
            if let Some(method) = converted_event.get("method").and_then(|v| v.as_object()) {
                let pallet = method.get("pallet").and_then(|v| v.as_str());
                let method_name = method.get("method").and_then(|v| v.as_str());
                
                if pallet == Some("system") {
                    match method_name {
                        Some("ExtrinsicSuccess") => {
                            success = true;
                            pays_fee = converted_event.get("data")
                                .and_then(|v| v.as_array())
                                .and_then(|arr| arr.get(0))
                                .and_then(|v| v.as_object())
                                .and_then(|obj| obj.get("paysFee"))
                                .and_then(|v| v.as_str())
                                .map(|s| s == "Yes")
                                .unwrap_or(false);
                        }
                        Some("ExtrinsicFailed") => success = false,
                        _ => {}
                    }
                }
            }
            
            extrinsic_events.push(converted_event);
        }
    }
    
    (extrinsic_events, success, pays_fee)
}

/// Extract extrinsics from a block using subxt-historic
pub async fn extract_extrinsics(
    state: &AppState,
    block_number: u64,
) -> Result<Vec<ExtrinsicInfo>, GetBlockError> {
    // Use subxt-historic to get a client at the specific block height
    // This ensures we use the correct metadata for that block
    let client_at_block = match state.client.at(block_number).await {
        Ok(client) => client,
        Err(e) => {
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

    // Fetch events for this block to associate with extrinsics
    let all_events = match client_at_block.storage().entry("System", "Events") {
        Ok(entry) => {
            match entry.into_plain() {
                Ok(plain_entry) => {
                    match plain_entry.fetch().await {
                        Ok(Some(events_value)) => {
                            match events_value.decode::<scale_value::Value>() {
                                Ok(decoded) => Some(decoded),
                                Err(e) => {
                                    tracing::warn!("Failed to decode events: {:?}", e);
                                    None
                                }
                            }
                        }
                        Ok(None) => None,
                        Err(e) => {
                            tracing::warn!("Failed to fetch events: {:?}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Events storage is not plain: {:?}", e);
                    None
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to get events storage entry: {:?}", e);
            None
        }
    };

    let mut result = Vec::new();

    for extrinsic in extrinsics.iter() {
        let pallet_name = to_camel_case(&extrinsic.call().pallet_name().to_string());
        let method_name = to_camel_case(&extrinsic.call().name().to_string());

        let args_composite = extrinsic
            .call()
            .fields()
            .decode::<scale_value::Composite<()>>()
            .map_err(|e| {
                GetBlockError::ExtrinsicDecodeFailed(format!("Failed to decode args: {}", e))
            })?;

        let args_json = serde_json::to_value(&args_composite).map_err(|e| {
            GetBlockError::ExtrinsicDecodeFailed(format!("Failed to serialize args: {}", e))
        })?;

        let args_converted = convert_args_to_sidecar_format(args_json);

        // Extract as map (should be an object from Composite)
        let mut args_map = if let Value::Object(map) = args_converted {
            map
        } else {
            serde_json::Map::new()
        };

        if pallet_name == "parachainSystem" && method_name == "setValidationData" {
            args_map = restructure_parachain_validation_data_args(args_map);
        }

        let signature_info = if extrinsic.is_signed() {
            if let (Some(sig_bytes), Some(addr_bytes)) = (
                extrinsic.signature_bytes(),
                extrinsic.address_bytes(),
            ) {
                Some(SignatureInfo {
                    signature: format!("0x{}", hex::encode(sig_bytes)),
                    signer: format!("0x{}", hex::encode(addr_bytes)),
                })
            } else {
                None
            }
        } else {
            None
        };

        // Extract nonce, tip, and era from transaction extensions
        use parity_scale_codec::Compact;
        let extensions = extrinsic.transaction_extensions();
        let (nonce, tip, era_info) = if let Some(extensions) = extensions {
            let mut nonce_value = None;
            let mut tip_value = None;
            let mut era_value = None;

            for ext in extensions.iter() {
                match ext.name() {
                    "CheckNonce" => {
                        if let Ok(compact_nonce) = ext.decode::<Compact<u64>>() {
                            nonce_value = Some(compact_nonce.0.to_string());
                        } else if let Ok(nonce) = ext.decode::<u64>() {
                            nonce_value = Some(nonce.to_string());
                        }
                    }
                    "ChargeTransactionPayment" | "ChargeAssetTxPayment" => {
                        if let Ok(compact_tip) = ext.decode::<Compact<u128>>() {
                            tip_value = Some(compact_tip.0.to_string());
                        } else if let Ok(tip) = ext.decode::<u128>() {
                            tip_value = Some(tip.to_string());
                        }
                    }
                    "CheckMortality" | "CheckEra" => {
                        let era_bytes = ext.bytes();
                        let mut offset = 0;
                        if let Some(decoded_era) = utils::decode_era_from_bytes(era_bytes, &mut offset) {
                            let era_json = if let Some(ref mortal) = decoded_era.mortal_era {
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
                                Value::Object(map)
                            } else if decoded_era.immortal_era.is_some() {
                                let mut map = serde_json::Map::new();
                                map.insert("name".to_string(), Value::String("Immortal".to_string()));
                                Value::Object(map)
                            } else {
                                continue;
                            };
                            era_value = Some(era_json);
                        }
                    }
                    _ => {}
                }
            }

            let era = if let Some(era_json) = era_value {
                utils::parse_era_info(&era_json)
            } else if let Some(era_parsed) = utils::extract_era_from_extrinsic_bytes(extrinsic.bytes()) {
                era_parsed
            } else {
                EraInfo {
                    immortal_era: Some("0x00".to_string()),
                    mortal_era: None,
                }
            };

            (nonce_value, tip_value, era)
        } else {
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

        // Extract events for this extrinsic from block events
        let (extrinsic_events, success, mut pays_fee) = if let Some(ref events_value) = all_events {
            extract_extrinsic_events(events_value, extrinsic.index() as u32, to_camel_case)
        } else {
            (Vec::new(), true, false)
        };
        
        if !extrinsic.is_signed() {
            pays_fee = false;
        }

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
            events: extrinsic_events,
            success,
            pays_fee,
        });
    }

    Ok(result)
}

