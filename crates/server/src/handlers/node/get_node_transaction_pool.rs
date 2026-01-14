use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::{Query, State}, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sp_core::hashing::blake2_256;
use std::cmp;
use subxt_rpcs::client::rpc_params;
use thiserror::Error;
use scale_value::scale::decode_as_type;
use scale_value::ValueDef;

#[derive(Debug, Error)]
pub enum GetNodeTransactionPoolError {
    #[error("Failed to get pending extrinsics")]
    PendingExtrinsicsFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get fee info")]
    FeeInfoFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get fee details")]
    FeeDetailsFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get block hash")]
    BlockHashFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get metadata")]
    MetadataFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to decode metadata")]
    MetadataDecodeFailed(#[source] parity_scale_codec::Error),

    #[error("Constant not found: {0}")]
    ConstantNotFound(String),
}

impl IntoResponse for GetNodeTransactionPoolError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        
        let (status, message) = match &self {
            GetNodeTransactionPoolError::PendingExtrinsicsFailed(err)
            | GetNodeTransactionPoolError::FeeInfoFailed(err)
            | GetNodeTransactionPoolError::FeeDetailsFailed(err)
            | GetNodeTransactionPoolError::BlockHashFailed(err)
            | GetNodeTransactionPoolError::MetadataFailed(err) => utils::rpc_error_to_status(err),
            GetNodeTransactionPoolError::MetadataDecodeFailed(_)
            | GetNodeTransactionPoolError::ConstantNotFound(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPoolQueryParams {
    #[serde(default)]
    pub include_fee: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPoolEntry {
    pub hash: String,
    pub encoded_extrinsic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_fee: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPoolResponse {
    pub pool: Vec<TransactionPoolEntry>,
}

/// Handler for GET /node/transaction-pool
///
/// Returns the transaction pool with optional fee information.
pub async fn get_node_transaction_pool(
    State(state): State<AppState>,
    Query(params): Query<TransactionPoolQueryParams>,
) -> Result<Json<TransactionPoolResponse>, GetNodeTransactionPoolError> {
    if !params.include_fee {
        let extrinsics: Vec<String> = state
            .rpc_client
            .request("author_pendingExtrinsics", rpc_params![])
            .await
            .map_err(GetNodeTransactionPoolError::PendingExtrinsicsFailed)?;

        let pool: Vec<TransactionPoolEntry> = extrinsics
            .into_iter()
            .map(|encoded_extrinsic| {
                let extrinsic_bytes = hex::decode(encoded_extrinsic.trim_start_matches("0x"))
                    .unwrap_or_default();
                let hash_bytes = blake2_256(&extrinsic_bytes);
                let hash = format!("0x{}", hex::encode(hash_bytes));
                
                TransactionPoolEntry {
                    hash,
                    encoded_extrinsic,
                    tip: None,
                    priority: None,
                    partial_fee: None,
                }
            })
            .collect();

        return Ok(Json(TransactionPoolResponse { pool }));
    }

    let (extrinsics_result, latest_hash_result) = tokio::join!(
        state.rpc_client.request::<Vec<String>>("author_pendingExtrinsics", rpc_params![]),
        state.rpc_client.request::<String>("chain_getFinalizedHead", rpc_params![])
    );

    let extrinsics = extrinsics_result
        .map_err(GetNodeTransactionPoolError::PendingExtrinsicsFailed)?;
    let latest_hash = latest_hash_result
        .map_err(GetNodeTransactionPoolError::BlockHashFailed)?;

    let mut pool = Vec::new();
    
    for encoded_extrinsic in extrinsics {
        let extrinsic_bytes = hex::decode(encoded_extrinsic.trim_start_matches("0x"))
            .unwrap_or_default();
        let hash_bytes = blake2_256(&extrinsic_bytes);
        let hash = format!("0x{}", hex::encode(hash_bytes));

        let encoded_length = extrinsic_bytes.len();
        let tip = extract_tip_from_extrinsic_bytes(&extrinsic_bytes);

        let fee_info = state
            .query_fee_info(&encoded_extrinsic, &latest_hash)
            .await
            .map_err(GetNodeTransactionPoolError::FeeInfoFailed)?;

        let partial_fee = fee_info
            .get("partialFee")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let priority = calculate_priority(
            &fee_info,
            &state,
            &encoded_extrinsic,
            &latest_hash,
            encoded_length,
            tip.as_ref().and_then(|t| t.parse::<u128>().ok()).unwrap_or(0),
        )
        .await
        .ok()
        .flatten();

        pool.push(TransactionPoolEntry {
            hash,
            encoded_extrinsic,
            tip,
            priority,
            partial_fee,
        });
    }

    Ok(Json(TransactionPoolResponse { pool }))
}

/// Extract tip from extrinsic bytes by decoding transaction extensions
/// Uses the same SCALE decoding pattern as extract_era_from_extrinsic_bytes in utils/extrinsic.rs
fn extract_tip_from_extrinsic_bytes(bytes: &[u8]) -> Option<String> {
    use parity_scale_codec::{Decode, Compact};
    use sp_runtime::generic::Era;
    
    if bytes.is_empty() {
        return None;
    }

    let mut cursor = &bytes[..];
    Compact::<u32>::decode(&mut cursor).ok()?;
    
    if cursor.is_empty() {
        return None;
    }

    let version = cursor[0];
    if version & 0b1000_0000 == 0 {
        return Some("0".to_string());
    }
    
    cursor = &cursor[1..];
    
    let addr_variant = u8::decode(&mut cursor).ok()?;
    match addr_variant {
        0x00 => {
            if cursor.len() < 32 { return None; }
            cursor = &cursor[32..];
        }
        0x01 => {
            Compact::<u32>::decode(&mut cursor).ok()?;
        }
        0x02 => {
            let Compact(len) = Compact::<u32>::decode(&mut cursor).ok()?;
            let len = len as usize;
            if cursor.len() < len { return None; }
            cursor = &cursor[len..];
        }
        0x03 => {
            if cursor.len() < 32 { return None; }
            cursor = &cursor[32..];
        }
        0x04 => {
            if cursor.len() < 20 { return None; }
            cursor = &cursor[20..];
        }
        _ => return None,
    }
    
    let sig_variant = u8::decode(&mut cursor).ok()?;
    match sig_variant {
        0x00 | 0x01 => {
            if cursor.len() < 64 { return None; }
            cursor = &cursor[64..];
        }
        0x02 => {
            if cursor.len() < 65 { return None; }
            cursor = &cursor[65..];
        }
        _ => return None,
    }
    
    
    Era::decode(&mut cursor).ok()?;
    
    Compact::<u32>::decode(&mut cursor).ok()?;
    
    let Compact(tip) = Compact::<u128>::decode(&mut cursor).ok()?;
    
    Some(tip.to_string())
}

/// Calculate priority for an extrinsic
/// Implements as in substrate: tip * (max_block_{weight|length} / bounded_{weight|length})
async fn calculate_priority(
    fee_info: &Value,
    state: &AppState,
    encoded_extrinsic: &str,
    latest_hash: &str,
    encoded_length: usize,
    tip: u128,
) -> Result<Option<String>, GetNodeTransactionPoolError> {
    let class_str = fee_info
        .get("class")
        .and_then(|v| v.as_str())
        .unwrap_or("Normal")
        .to_lowercase();
    
    let versioned_weight = if let Some(weight_obj) = fee_info.get("weight").and_then(|w| w.as_object()) {
        weight_obj
            .get("refTime")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| GetNodeTransactionPoolError::ConstantNotFound("weight.refTime".to_string()))?
    } else {
        fee_info
            .get("weight")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| GetNodeTransactionPoolError::ConstantNotFound("weight".to_string()))?
    };

    let max_block_weight = get_max_block_weight(state, latest_hash).await?;
    let max_length = get_max_block_length(state, latest_hash, &class_str).await?;
    
    let bounded_weight = cmp::max(cmp::min(versioned_weight, max_block_weight), 1);
    let bounded_length = cmp::max(cmp::min(encoded_length, max_length as usize), 1);
    
    let max_tx_per_block_weight = max_block_weight / bounded_weight;
    let max_tx_per_block_length = max_length / bounded_length as u64;
    let max_tx_per_block = cmp::min(max_tx_per_block_weight, max_tx_per_block_length);
    
    let saturated_tip = tip.saturating_add(1);
    
    let scaled_tip = saturated_tip.saturating_mul(max_tx_per_block as u128);
    
    let priority = match class_str.as_str() {
        "normal" | "mandatory" => scaled_tip.to_string(),
        "operational" => {
            match state.query_fee_details(encoded_extrinsic, latest_hash).await {
                Ok(fee_details) => {
                    if let Some(inclusion_fee) = fee_details.get("inclusionFee").and_then(|v| v.as_object()) {
                        let base_fee = inclusion_fee
                            .get("baseFee")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<u128>().ok())
                            .ok_or_else(|| GetNodeTransactionPoolError::ConstantNotFound("baseFee".to_string()))?;
                        let len_fee = inclusion_fee
                            .get("lenFee")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<u128>().ok())
                            .ok_or_else(|| GetNodeTransactionPoolError::ConstantNotFound("lenFee".to_string()))?;
                        let adjusted_weight_fee = inclusion_fee
                            .get("adjustedWeightFee")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<u128>().ok())
                            .ok_or_else(|| GetNodeTransactionPoolError::ConstantNotFound("adjustedWeightFee".to_string()))?;
                        
                        let computed_inclusion_fee = base_fee.saturating_add(len_fee).saturating_add(adjusted_weight_fee);
                        let final_fee = computed_inclusion_fee.saturating_add(tip);
                        
                        let operational_fee_multiplier = get_operational_fee_multiplier(state, latest_hash).await?;
                        
                        let virtual_tip = final_fee.saturating_mul(operational_fee_multiplier);
                        let scaled_virtual_tip = virtual_tip.saturating_mul(max_tx_per_block as u128);
                        
                        scaled_tip.saturating_add(scaled_virtual_tip).to_string()
                    } else {
                        "0".to_string()
                    }
                }
                Err(_) => "0".to_string(),
            }
        }
        _ => "0".to_string(),
    };
    
    Ok(Some(priority))
}

async fn get_max_block_weight(state: &AppState, block_hash: &str) -> Result<u64, GetNodeTransactionPoolError> {
    let metadata = get_runtime_metadata(state, block_hash).await?;
    extract_max_block_weight(&metadata)
        .ok_or_else(|| GetNodeTransactionPoolError::ConstantNotFound(
            "System::BlockWeights::maxBlock::refTime".to_string()
        ))
}

async fn get_max_block_length(state: &AppState, block_hash: &str, class: &str) -> Result<u64, GetNodeTransactionPoolError> {
    let metadata = get_runtime_metadata(state, block_hash).await?;
    extract_max_block_length(&metadata, class)
        .ok_or_else(|| GetNodeTransactionPoolError::ConstantNotFound(
            format!("System::BlockLength::max[{}]", class)
        ))
}

async fn get_operational_fee_multiplier(state: &AppState, block_hash: &str) -> Result<u128, GetNodeTransactionPoolError> {
    let metadata = get_runtime_metadata(state, block_hash).await?;
    extract_operational_fee_multiplier(&metadata)
        .ok_or_else(|| GetNodeTransactionPoolError::ConstantNotFound(
            "TransactionPayment::operationalFeeMultiplier".to_string()
        ))
}

async fn get_runtime_metadata(state: &AppState, block_hash: &str) -> Result<frame_metadata::RuntimeMetadataPrefixed, GetNodeTransactionPoolError> {
    use frame_metadata::RuntimeMetadataPrefixed;
    use parity_scale_codec::Decode;
    
    let metadata_hex: String = state
        .rpc_client
        .request("state_getMetadata", subxt_rpcs::client::rpc_params![block_hash])
        .await
        .map_err(GetNodeTransactionPoolError::MetadataFailed)?;

    let hex_str = metadata_hex.strip_prefix("0x").unwrap_or(&metadata_hex);
    let metadata_bytes = hex::decode(hex_str)
        .map_err(|_| GetNodeTransactionPoolError::MetadataDecodeFailed(
            parity_scale_codec::Error::from("Failed to decode hex")
        ))?;

    RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])
        .map_err(GetNodeTransactionPoolError::MetadataDecodeFailed)
}

fn extract_max_block_weight(metadata: &frame_metadata::RuntimeMetadataPrefixed) -> Option<u64> {
    use frame_metadata::RuntimeMetadata;
    
    match &metadata.1 {
        RuntimeMetadata::V14(m) => {
            let registry = &m.types;
            let system_pallet = m.pallets.iter().find(|p| p.name == "System")?;
            let block_weights_constant = system_pallet.constants.iter().find(|c| c.name == "BlockWeights")?;
            
            let mut bytes = &block_weights_constant.value[..];
            let decoded = decode_as_type(&mut bytes, block_weights_constant.ty.id, registry).ok()?;
            
            if let ValueDef::Composite(scale_value::Composite::Named(fields)) = &decoded.value {
                if let Some((_, max_block_val)) = fields.iter().find(|(name, _)| name == "maxBlock") {
                    if let ValueDef::Composite(scale_value::Composite::Named(weight_fields)) = &max_block_val.value {
                        if let Some((_, ref_time_val)) = weight_fields.iter().find(|(name, _)| name == "refTime") {
                            if let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &ref_time_val.value {
                                return Some(*n as u64);
                            }
                        }
                    }
                }
            }
            None
        }
        RuntimeMetadata::V15(m) => {
            let registry = &m.types;
            let system_pallet = m.pallets.iter().find(|p| p.name == "System")?;
            let block_weights_constant = system_pallet.constants.iter().find(|c| c.name == "BlockWeights")?;
            
            let mut bytes = &block_weights_constant.value[..];
            let decoded = decode_as_type(&mut bytes, block_weights_constant.ty.id, registry).ok()?;
            
            if let ValueDef::Composite(scale_value::Composite::Named(fields)) = &decoded.value {
                if let Some((_, max_block_val)) = fields.iter().find(|(name, _)| name == "maxBlock") {
                    if let ValueDef::Composite(scale_value::Composite::Named(weight_fields)) = &max_block_val.value {
                        if let Some((_, ref_time_val)) = weight_fields.iter().find(|(name, _)| name == "refTime") {
                            if let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &ref_time_val.value {
                                return Some(*n as u64);
                            }
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_max_block_length(metadata: &frame_metadata::RuntimeMetadataPrefixed, class: &str) -> Option<u64> {
    use frame_metadata::RuntimeMetadata;
    
    let class_index = match class {
        "normal" => 0,
        "operational" => 1,
        "mandatory" => 2,
        _ => return None,
    };
    
    match &metadata.1 {
        RuntimeMetadata::V14(m) => {
            let registry = &m.types;
            let system_pallet = m.pallets.iter().find(|p| p.name == "System")?;
            let block_length_constant = system_pallet.constants.iter().find(|c| c.name == "BlockLength")?;
            
            let mut bytes = &block_length_constant.value[..];
            let decoded = decode_as_type(&mut bytes, block_length_constant.ty.id, registry).ok()?;
            
            if let ValueDef::Composite(scale_value::Composite::Named(fields)) = &decoded.value {
                if let Some((_, max_val)) = fields.iter().find(|(name, _)| name == "max") {
                    if let ValueDef::Composite(scale_value::Composite::Unnamed(array_fields)) = &max_val.value {
                        let fields_vec: Vec<_> = array_fields.iter().collect();
                        if let Some(class_val) = fields_vec.get(class_index) {
                            if let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &class_val.value {
                                return Some(*n as u64);
                            }
                        }
                    } else if let ValueDef::Composite(scale_value::Composite::Named(named_fields)) = &max_val.value {
                        let class_name = match class_index {
                            0 => "normal",
                            1 => "operational",
                            2 => "mandatory",
                            _ => return None,
                        };
                        if let Some((_, class_val)) = named_fields.iter().find(|(name, _)| name == class_name) {
                            if let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &class_val.value {
                                return Some(*n as u64);
                            }
                        }
                    }
                }
            }
            None
        }
        RuntimeMetadata::V15(m) => {
            let registry = &m.types;
            let system_pallet = m.pallets.iter().find(|p| p.name == "System")?;
            let block_length_constant = system_pallet.constants.iter().find(|c| c.name == "BlockLength")?;
            
            let mut bytes = &block_length_constant.value[..];
            let decoded = decode_as_type(&mut bytes, block_length_constant.ty.id, registry).ok()?;
            
            if let ValueDef::Composite(scale_value::Composite::Named(fields)) = &decoded.value {
                if let Some((_, max_val)) = fields.iter().find(|(name, _)| name == "max") {
                    if let ValueDef::Composite(scale_value::Composite::Unnamed(array_fields)) = &max_val.value {
                        let fields_vec: Vec<_> = array_fields.iter().collect();
                        if let Some(class_val) = fields_vec.get(class_index) {
                            if let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &class_val.value {
                                return Some(*n as u64);
                            }
                        }
                    } else if let ValueDef::Composite(scale_value::Composite::Named(named_fields)) = &max_val.value {
                        let class_name = match class_index {
                            0 => "normal",
                            1 => "operational",
                            2 => "mandatory",
                            _ => return None,
                        };
                        if let Some((_, class_val)) = named_fields.iter().find(|(name, _)| name == class_name) {
                            if let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &class_val.value {
                                return Some(*n as u64);
                            }
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_operational_fee_multiplier(metadata: &frame_metadata::RuntimeMetadataPrefixed) -> Option<u128> {
    use frame_metadata::RuntimeMetadata;
    
    match &metadata.1 {
        RuntimeMetadata::V14(m) => {
            let registry = &m.types;
            let tx_payment_pallet = m.pallets.iter().find(|p| p.name == "TransactionPayment")?;
            let constant = tx_payment_pallet.constants.iter().find(|c| c.name == "OperationalFeeMultiplier")?;
            
            let mut bytes = &constant.value[..];
            let decoded = decode_as_type(&mut bytes, constant.ty.id, registry).ok()?;
            
            match &decoded.value {
                ValueDef::Primitive(scale_value::Primitive::U128(n)) => Some(*n),
                _ => None,
            }
        }
        RuntimeMetadata::V15(m) => {
            let registry = &m.types;
            let tx_payment_pallet = m.pallets.iter().find(|p| p.name == "TransactionPayment")?;
            let constant = tx_payment_pallet.constants.iter().find(|c| c.name == "OperationalFeeMultiplier")?;
            
            let mut bytes = &constant.value[..];
            let decoded = decode_as_type(&mut bytes, constant.ty.id, registry).ok()?;
            
            match &decoded.value {
                ValueDef::Primitive(scale_value::Primitive::U128(n)) => Some(*n),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use axum::extract::{Query, State};
    use config::SidecarConfig;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

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
            relay_client: None,
            relay_rpc_client: None,
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: crate::routes::RouteRegistry::new(),
        }
    }

    fn create_minimal_test_metadata() -> String {
        "0x6d657461".to_string()
    }

    fn real_polkadot_extrinsic_tip_zero() -> String {
        "0xed098400af3e1db41e95040f7630e64d1b3104235c08545e452b15fd70601881aa224b740048ceb5c1995db4427ba1322f48702cebe4b4564e03d660d6a713f25e48143be454875d56716def88a61283643fcb9a0aed7caccbfe285dfba8399b07bc448c063501740001070540000000966d74f8027e07b43717b6876d97544fe0d71facef06acc8382749ae944e00005fa73637062b".to_string()
    }

    fn real_asset_hub_extrinsic_transfer() -> String {
        "0x4902840004316d995f0adb06d918a1fc96077ebdfa93aab9ccf2a8525efd7bf0c1e2282700a24152685f52e4726466e80247d965bb3d349637fc8a1ea6f7cc1451ddec98b5bf30b6e8e31b31f0870ac46f07ccb559402a0fafe90b74127f28e8644281730c00d12b0000000a0000d61e33684a7a41d7233e89955316dbc875fef1428e4f16ec260617dc57de3972078064288004".to_string()
    }

    fn real_asset_hub_extrinsic_assets() -> String {
        "0x550284000a6679243e822e0538039d187529d67c1bb74d8d5f121be63d00243233b4b01c01b8af8c2b3b7f1d020f42fc98e3957ae79957173a6e29d25fc6ad976d851ad743f4316e9e370a3a3aa3f8747f870235c18c5d8fdb75e34a831f0d0b85a9f72181f400f50b0000013208011f00d426c7726e426586d570e2ef43f3b0784fea005e80fa6e3bca9139a38f5ff1ad078068c84b0d".to_string()
    }

    fn build_extrinsic_with_tip(tip: u128) -> String {
        use parity_scale_codec::{Encode, Compact};

        let mut body = vec![0x84];
        
        body.push(0x00);
        body.extend_from_slice(&[0x42u8; 32]);
        
        body.push(0x01);
        body.extend_from_slice(&[0xAAu8; 64]);
        
        body.push(0x00);
        
        Compact(1u32).encode_to(&mut body);
        
        Compact(tip).encode_to(&mut body);
        
        body.push(0x00);
        body.push(0x00);
        
        let mut extrinsic = Vec::new();
        Compact(body.len() as u32).encode_to(&mut extrinsic);
        extrinsic.extend(body);
        
        format!("0x{}", hex::encode(&extrinsic))
    }

    #[tokio::test]
    async fn test_transaction_pool_empty() {
        let mock_client = MockRpcClient::builder()
            .method_handler("author_pendingExtrinsics", async |_params| {
                MockJson(serde_json::json!([]))
            })
            .build();

        let state = create_test_state_with_mock(mock_client);
        let params = TransactionPoolQueryParams {
            include_fee: false,
        };

        let result = get_node_transaction_pool(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.pool.len(), 0);
    }

    #[tokio::test]
    async fn test_transaction_pool_without_fee_real_extrinsics() {
        let extrinsic1 = real_asset_hub_extrinsic_transfer();
        let extrinsic2 = real_asset_hub_extrinsic_assets();
        
        let mock_client = MockRpcClient::builder()
            .method_handler("author_pendingExtrinsics", async |_params| {
                MockJson(serde_json::json!([
                    real_asset_hub_extrinsic_transfer(),
                    real_asset_hub_extrinsic_assets()
                ]))
            })
            .build();

        let state = create_test_state_with_mock(mock_client);
        let params = TransactionPoolQueryParams {
            include_fee: false,
        };

        let result = get_node_transaction_pool(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.pool.len(), 2);
        
        let entry1 = &response.pool[0];
        assert!(!entry1.hash.is_empty());
        assert_eq!(entry1.encoded_extrinsic, extrinsic1);
        assert!(entry1.tip.is_none(), "tip should be None when includeFee=false");
        assert!(entry1.priority.is_none());
        assert!(entry1.partial_fee.is_none());

        let entry2 = &response.pool[1];
        assert!(!entry2.hash.is_empty());
        assert_eq!(entry2.encoded_extrinsic, extrinsic2);
    }

    #[tokio::test]
    async fn test_transaction_pool_with_fee_real_extrinsic() {
        let extrinsic_hex = real_asset_hub_extrinsic_transfer();
        
        let mock_client = MockRpcClient::builder()
            .method_handler("author_pendingExtrinsics", async |_params| {
                MockJson(serde_json::json!([real_asset_hub_extrinsic_transfer()]))
            })
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("payment_queryInfo", async |_params| {
                MockJson(serde_json::json!({
                    "weight": { "refTime": "1000000", "proofSize": "0" },
                    "class": "Normal",
                    "partialFee": "14668864"
                }))
            })
            .method_handler("state_getMetadata", async |_params| {
                MockJson(create_minimal_test_metadata())
            })
            .build();

        let state = create_test_state_with_mock(mock_client);
        let params = TransactionPoolQueryParams {
            include_fee: true,
        };

        let result = get_node_transaction_pool(State(state), Query(params)).await;
        if let Ok(response) = result {
            assert_eq!(response.pool.len(), 1);
            let entry = &response.pool[0];
            assert!(!entry.hash.is_empty());
            assert_eq!(entry.encoded_extrinsic, extrinsic_hex);
            assert_eq!(entry.tip, Some("0".to_string()), "Real extrinsic has tip=0");
            assert_eq!(entry.partial_fee, Some("14668864".to_string()));
        }
    }

    #[test]
    fn test_extract_tip_real_extrinsics() {
        let test_cases = [
            ("Polkadot Staking::nominate", real_polkadot_extrinsic_tip_zero(), "0"),
            ("Asset Hub transfer", real_asset_hub_extrinsic_transfer(), "0"),
            ("Asset Hub assets", real_asset_hub_extrinsic_assets(), "0"),
        ];

        for (name, hex, expected_tip) in test_cases {
            let bytes = hex::decode(hex.trim_start_matches("0x")).unwrap();
            let tip = extract_tip_from_extrinsic_bytes(&bytes);
            assert_eq!(tip, Some(expected_tip.to_string()), "Failed for: {}", name);
        }
    }

    #[test]
    fn test_extract_tip_synthetic_various_values() {
        for expected_tip in [0u128, 1, 100, 1000, 1_000_000, u64::MAX as u128, u128::MAX / 2] {
            let extrinsic_hex = build_extrinsic_with_tip(expected_tip);
            let extrinsic_bytes = hex::decode(extrinsic_hex.trim_start_matches("0x")).unwrap();
            
            let tip = extract_tip_from_extrinsic_bytes(&extrinsic_bytes);
            assert_eq!(tip, Some(expected_tip.to_string()), "Failed for tip: {}", expected_tip);
        }
    }

    #[test]
    fn test_extract_tip_unsigned_extrinsic() {
        use parity_scale_codec::{Encode, Compact};
        
        let body = vec![0x04, 0x00, 0x00];
        let mut extrinsic = Vec::new();
        Compact(body.len() as u32).encode_to(&mut extrinsic);
        extrinsic.extend(body);
        
        let tip = extract_tip_from_extrinsic_bytes(&extrinsic);
        assert_eq!(tip, Some("0".to_string()));
    }

    #[test]
    fn test_extract_tip_edge_cases() {
        assert!(extract_tip_from_extrinsic_bytes(&[]).is_none(), "Empty bytes");
        assert!(extract_tip_from_extrinsic_bytes(&[0x00]).is_none(), "Invalid/truncated bytes");
    }
}
