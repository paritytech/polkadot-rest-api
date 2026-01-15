//! Common logic shared between node/* and rc/node/* handlers
//!
//! This module contains the core business logic that is identical between
//! Asset Hub and Relay Chain queries. The handlers simply provide the appropriate
//! RPC client and call these shared functions.

use frame_metadata::RuntimeMetadataPrefixed;
use parity_scale_codec::Decode;
use scale_value::ValueDef;
use scale_value::scale::decode_as_type;
use serde_json::{Value, json};
use sp_core::hashing::blake2_256;
use std::cmp;
use subxt_historic::SubstrateConfig;
use subxt_rpcs::{LegacyRpcMethods, RpcClient, client::rpc_params};

use super::{NodeNetworkResponse, NodeVersionResponse};

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("RPC call failed")]
    RpcFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to decode metadata")]
    MetadataDecodeFailed(#[source] parity_scale_codec::Error),

    #[error("Constant not found: {0}")]
    ConstantNotFound(String),
}

// ============================================================================
// VERSION
// ============================================================================

pub async fn fetch_node_version(
    rpc_client: &RpcClient,
    legacy_rpc: &LegacyRpcMethods<SubstrateConfig>,
) -> Result<NodeVersionResponse, FetchError> {
    let (runtime_version_result, chain_result, version_result) = tokio::join!(
        legacy_rpc.state_get_runtime_version(None),
        rpc_client.request::<String>("system_chain", rpc_params![]),
        rpc_client.request::<String>("system_version", rpc_params![]),
    );

    let runtime_version = runtime_version_result.map_err(FetchError::RpcFailed)?;
    let chain = chain_result.map_err(FetchError::RpcFailed)?;
    let client_version = version_result.map_err(FetchError::RpcFailed)?;

    let client_impl_name = runtime_version
        .other
        .get("implName")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(NodeVersionResponse {
        client_version,
        client_impl_name,
        chain,
    })
}

// ============================================================================
// NETWORK
// ============================================================================

pub async fn fetch_node_network(rpc_client: &RpcClient) -> Result<NodeNetworkResponse, FetchError> {
    let (health_result, peer_id_result, roles_result, addresses_result) = tokio::join!(
        rpc_client.request::<Value>("system_health", rpc_params![]),
        rpc_client.request::<String>("system_localPeerId", rpc_params![]),
        rpc_client.request::<Vec<String>>("system_nodeRoles", rpc_params![]),
        rpc_client.request::<Vec<String>>("system_localListenAddresses", rpc_params![]),
    );

    let health = health_result.map_err(FetchError::RpcFailed)?;
    let local_peer_id = peer_id_result.map_err(FetchError::RpcFailed)?;
    let node_roles_raw = roles_result.map_err(FetchError::RpcFailed)?;
    let local_listen_addresses = addresses_result.map_err(FetchError::RpcFailed)?;

    let node_roles: Vec<Value> = node_roles_raw
        .into_iter()
        .map(|role| json!({ role.to_lowercase(): null }))
        .collect();

    let num_peers = health.get("peers").and_then(|v| v.as_u64()).unwrap_or(0);
    let is_syncing = health
        .get("isSyncing")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let should_have_peers = health
        .get("shouldHavePeers")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let peers_info = match rpc_client
        .request::<Value>("system_peers", rpc_params![])
        .await
    {
        Ok(peers) => transform_peers_info(peers),
        Err(_) => Value::String("Cannot query system_peers from node.".to_string()),
    };

    Ok(NodeNetworkResponse {
        node_roles,
        num_peers,
        is_syncing,
        should_have_peers,
        local_peer_id,
        local_listen_addresses,
        peers_info,
    })
}

fn transform_peers_info(peers: Value) -> Value {
    if let Value::Array(peers_array) = peers {
        let transformed: Vec<Value> = peers_array
            .into_iter()
            .filter_map(|peer| {
                if let Value::Object(peer_obj) = peer {
                    let mut transformed_peer = serde_json::Map::new();

                    let peer_id = peer_obj
                        .get("peerId")
                        .or_else(|| peer_obj.get("peer_id"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    if let Some(pid) = peer_id {
                        transformed_peer.insert("peerId".to_string(), Value::String(pid));
                    }

                    if let Some(roles) = peer_obj.get("roles") {
                        let roles_str = match roles {
                            Value::String(s) => s.clone(),
                            Value::Array(arr) => arr
                                .iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            _ => roles.to_string(),
                        };
                        transformed_peer.insert("roles".to_string(), Value::String(roles_str));
                    }

                    if let Some(protocol_version) = peer_obj
                        .get("protocolVersion")
                        .or_else(|| peer_obj.get("protocol_version"))
                    {
                        let protocol_version_str = match protocol_version {
                            Value::Number(n) => n.to_string(),
                            Value::String(s) => s.clone(),
                            _ => protocol_version.to_string(),
                        };
                        transformed_peer.insert(
                            "protocolVersion".to_string(),
                            Value::String(protocol_version_str),
                        );
                    }

                    if let Some(best_hash) = peer_obj
                        .get("bestHash")
                        .or_else(|| peer_obj.get("best_hash"))
                    {
                        let best_hash_str = match best_hash {
                            Value::String(s) => s.clone(),
                            _ => best_hash.to_string(),
                        };
                        transformed_peer
                            .insert("bestHash".to_string(), Value::String(best_hash_str));
                    }

                    if let Some(best_number) = peer_obj
                        .get("bestNumber")
                        .or_else(|| peer_obj.get("best_number"))
                    {
                        let best_number_str = match best_number {
                            Value::Number(n) => n.to_string(),
                            Value::String(s) => s.clone(),
                            _ => best_number.to_string(),
                        };
                        transformed_peer
                            .insert("bestNumber".to_string(), Value::String(best_number_str));
                    }

                    if transformed_peer.contains_key("peerId") {
                        Some(Value::Object(transformed_peer))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        Value::Array(transformed)
    } else {
        Value::Array(vec![])
    }
}

// ============================================================================
// TRANSACTION POOL
// ============================================================================

use super::{TransactionPoolEntry, TransactionPoolResponse};

pub async fn fetch_transaction_pool_simple(
    rpc_client: &RpcClient,
) -> Result<TransactionPoolResponse, FetchError> {
    let extrinsics: Vec<String> = rpc_client
        .request("author_pendingExtrinsics", rpc_params![])
        .await
        .map_err(FetchError::RpcFailed)?;

    let pool: Vec<TransactionPoolEntry> = extrinsics
        .into_iter()
        .map(|encoded_extrinsic| {
            let extrinsic_bytes =
                hex::decode(encoded_extrinsic.trim_start_matches("0x")).unwrap_or_default();
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

    Ok(TransactionPoolResponse { pool })
}

pub async fn fetch_transaction_pool_with_fees(
    rpc_client: &RpcClient,
) -> Result<TransactionPoolResponse, FetchError> {
    let (extrinsics_result, latest_hash_result) = tokio::join!(
        rpc_client.request::<Vec<String>>("author_pendingExtrinsics", rpc_params![]),
        rpc_client.request::<String>("chain_getFinalizedHead", rpc_params![])
    );

    let extrinsics = extrinsics_result.map_err(FetchError::RpcFailed)?;
    let latest_hash = latest_hash_result.map_err(FetchError::RpcFailed)?;

    let mut pool = Vec::new();

    for encoded_extrinsic in extrinsics {
        let extrinsic_bytes =
            hex::decode(encoded_extrinsic.trim_start_matches("0x")).unwrap_or_default();
        let hash_bytes = blake2_256(&extrinsic_bytes);
        let hash = format!("0x{}", hex::encode(hash_bytes));

        let encoded_length = extrinsic_bytes.len();
        let tip = extract_tip_from_extrinsic_bytes(&extrinsic_bytes);

        let fee_info = query_fee_info(rpc_client, &encoded_extrinsic, &latest_hash)
            .await
            .map_err(FetchError::RpcFailed)?;

        let partial_fee = fee_info
            .get("partialFee")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let priority = calculate_priority(
            &fee_info,
            rpc_client,
            &encoded_extrinsic,
            &latest_hash,
            encoded_length,
            tip.as_ref()
                .and_then(|t| t.parse::<u128>().ok())
                .unwrap_or(0),
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

    Ok(TransactionPoolResponse { pool })
}

pub fn extract_tip_from_extrinsic_bytes(bytes: &[u8]) -> Option<String> {
    use parity_scale_codec::Compact;
    use sp_runtime::generic::Era;

    if bytes.is_empty() {
        return None;
    }

    let mut cursor = bytes;
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
            if cursor.len() < 32 {
                return None;
            }
            cursor = &cursor[32..];
        }
        0x01 => {
            Compact::<u32>::decode(&mut cursor).ok()?;
        }
        0x02 => {
            let Compact(len) = Compact::<u32>::decode(&mut cursor).ok()?;
            let len = len as usize;
            if cursor.len() < len {
                return None;
            }
            cursor = &cursor[len..];
        }
        0x03 => {
            if cursor.len() < 32 {
                return None;
            }
            cursor = &cursor[32..];
        }
        0x04 => {
            if cursor.len() < 20 {
                return None;
            }
            cursor = &cursor[20..];
        }
        _ => return None,
    }

    let sig_variant = u8::decode(&mut cursor).ok()?;
    match sig_variant {
        0x00 | 0x01 => {
            if cursor.len() < 64 {
                return None;
            }
            cursor = &cursor[64..];
        }
        0x02 => {
            if cursor.len() < 65 {
                return None;
            }
            cursor = &cursor[65..];
        }
        _ => return None,
    }

    Era::decode(&mut cursor).ok()?;

    Compact::<u32>::decode(&mut cursor).ok()?;

    let Compact(tip) = Compact::<u128>::decode(&mut cursor).ok()?;

    Some(tip.to_string())
}

async fn query_fee_info(
    rpc_client: &RpcClient,
    encoded_extrinsic: &str,
    block_hash: &str,
) -> Result<Value, subxt_rpcs::Error> {
    rpc_client
        .request(
            "payment_queryInfo",
            rpc_params![encoded_extrinsic, block_hash],
        )
        .await
}

async fn query_fee_details(
    rpc_client: &RpcClient,
    encoded_extrinsic: &str,
    block_hash: &str,
) -> Result<Value, subxt_rpcs::Error> {
    rpc_client
        .request(
            "payment_queryFeeDetails",
            rpc_params![encoded_extrinsic, block_hash],
        )
        .await
}

async fn calculate_priority(
    fee_info: &Value,
    rpc_client: &RpcClient,
    encoded_extrinsic: &str,
    latest_hash: &str,
    encoded_length: usize,
    tip: u128,
) -> Result<Option<String>, FetchError> {
    let class_str = fee_info
        .get("class")
        .and_then(|v| v.as_str())
        .unwrap_or("Normal")
        .to_lowercase();

    let versioned_weight =
        if let Some(weight_obj) = fee_info.get("weight").and_then(|w| w.as_object()) {
            weight_obj
                .get("refTime")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| FetchError::ConstantNotFound("weight.refTime".to_string()))?
        } else {
            fee_info
                .get("weight")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| FetchError::ConstantNotFound("weight".to_string()))?
        };

    let max_block_weight = get_max_block_weight(rpc_client, latest_hash).await?;
    let max_length = get_max_block_length(rpc_client, latest_hash, &class_str).await?;

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
            match query_fee_details(rpc_client, encoded_extrinsic, latest_hash).await {
                Ok(fee_details) => {
                    if let Some(inclusion_fee) =
                        fee_details.get("inclusionFee").and_then(|v| v.as_object())
                    {
                        let base_fee = inclusion_fee
                            .get("baseFee")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<u128>().ok())
                            .ok_or_else(|| FetchError::ConstantNotFound("baseFee".to_string()))?;
                        let len_fee = inclusion_fee
                            .get("lenFee")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<u128>().ok())
                            .ok_or_else(|| FetchError::ConstantNotFound("lenFee".to_string()))?;
                        let adjusted_weight_fee = inclusion_fee
                            .get("adjustedWeightFee")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<u128>().ok())
                            .ok_or_else(|| {
                                FetchError::ConstantNotFound("adjustedWeightFee".to_string())
                            })?;

                        let computed_inclusion_fee = base_fee
                            .saturating_add(len_fee)
                            .saturating_add(adjusted_weight_fee);
                        let final_fee = computed_inclusion_fee.saturating_add(tip);

                        let operational_fee_multiplier =
                            get_operational_fee_multiplier(rpc_client, latest_hash).await?;

                        let virtual_tip = final_fee.saturating_mul(operational_fee_multiplier);
                        let scaled_virtual_tip =
                            virtual_tip.saturating_mul(max_tx_per_block as u128);

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

async fn get_max_block_weight(rpc_client: &RpcClient, block_hash: &str) -> Result<u64, FetchError> {
    let metadata = get_runtime_metadata(rpc_client, block_hash).await?;
    extract_max_block_weight(&metadata).ok_or_else(|| {
        FetchError::ConstantNotFound("System::BlockWeights::maxBlock::refTime".to_string())
    })
}

async fn get_max_block_length(
    rpc_client: &RpcClient,
    block_hash: &str,
    class: &str,
) -> Result<u64, FetchError> {
    let metadata = get_runtime_metadata(rpc_client, block_hash).await?;
    extract_max_block_length(&metadata, class)
        .ok_or_else(|| FetchError::ConstantNotFound(format!("System::BlockLength::max[{}]", class)))
}

async fn get_operational_fee_multiplier(
    rpc_client: &RpcClient,
    block_hash: &str,
) -> Result<u128, FetchError> {
    let metadata = get_runtime_metadata(rpc_client, block_hash).await?;
    extract_operational_fee_multiplier(&metadata).ok_or_else(|| {
        FetchError::ConstantNotFound("TransactionPayment::operationalFeeMultiplier".to_string())
    })
}

async fn get_runtime_metadata(
    rpc_client: &RpcClient,
    block_hash: &str,
) -> Result<RuntimeMetadataPrefixed, FetchError> {
    let metadata_hex: String = rpc_client
        .request("state_getMetadata", rpc_params![block_hash])
        .await
        .map_err(FetchError::RpcFailed)?;

    let hex_str = metadata_hex.strip_prefix("0x").unwrap_or(&metadata_hex);
    let metadata_bytes = hex::decode(hex_str).map_err(|_| {
        FetchError::MetadataDecodeFailed(parity_scale_codec::Error::from("Failed to decode hex"))
    })?;

    RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])
        .map_err(FetchError::MetadataDecodeFailed)
}

fn extract_max_block_weight(metadata: &RuntimeMetadataPrefixed) -> Option<u64> {
    use frame_metadata::RuntimeMetadata;

    match &metadata.1 {
        RuntimeMetadata::V14(m) => {
            let registry = &m.types;
            let system_pallet = m.pallets.iter().find(|p| p.name == "System")?;
            let block_weights_constant = system_pallet
                .constants
                .iter()
                .find(|c| c.name == "BlockWeights")?;

            let mut bytes = &block_weights_constant.value[..];
            let decoded =
                decode_as_type(&mut bytes, block_weights_constant.ty.id, registry).ok()?;

            if let ValueDef::Composite(scale_value::Composite::Named(fields)) = &decoded.value
                && let Some((_, max_block_val)) = fields.iter().find(|(name, _)| name == "maxBlock")
                && let ValueDef::Composite(scale_value::Composite::Named(weight_fields)) =
                    &max_block_val.value
                && let Some((_, ref_time_val)) =
                    weight_fields.iter().find(|(name, _)| name == "refTime")
                && let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &ref_time_val.value
            {
                return Some(*n as u64);
            }
            None
        }
        RuntimeMetadata::V15(m) => {
            let registry = &m.types;
            let system_pallet = m.pallets.iter().find(|p| p.name == "System")?;
            let block_weights_constant = system_pallet
                .constants
                .iter()
                .find(|c| c.name == "BlockWeights")?;

            let mut bytes = &block_weights_constant.value[..];
            let decoded =
                decode_as_type(&mut bytes, block_weights_constant.ty.id, registry).ok()?;

            if let ValueDef::Composite(scale_value::Composite::Named(fields)) = &decoded.value
                && let Some((_, max_block_val)) = fields.iter().find(|(name, _)| name == "maxBlock")
                && let ValueDef::Composite(scale_value::Composite::Named(weight_fields)) =
                    &max_block_val.value
                && let Some((_, ref_time_val)) =
                    weight_fields.iter().find(|(name, _)| name == "refTime")
                && let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &ref_time_val.value
            {
                return Some(*n as u64);
            }
            None
        }
        _ => None,
    }
}

fn extract_max_block_length(metadata: &RuntimeMetadataPrefixed, class: &str) -> Option<u64> {
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
            let block_length_constant = system_pallet
                .constants
                .iter()
                .find(|c| c.name == "BlockLength")?;

            let mut bytes = &block_length_constant.value[..];
            let decoded = decode_as_type(&mut bytes, block_length_constant.ty.id, registry).ok()?;

            if let ValueDef::Composite(scale_value::Composite::Named(fields)) = &decoded.value
                && let Some((_, max_val)) = fields.iter().find(|(name, _)| name == "max")
            {
                if let ValueDef::Composite(scale_value::Composite::Unnamed(array_fields)) =
                    &max_val.value
                {
                    let fields_vec: Vec<_> = array_fields.iter().collect();
                    if let Some(class_val) = fields_vec.get(class_index)
                        && let ValueDef::Primitive(scale_value::Primitive::U128(n)) =
                            &class_val.value
                    {
                        return Some(*n as u64);
                    }
                } else if let ValueDef::Composite(scale_value::Composite::Named(named_fields)) =
                    &max_val.value
                {
                    let class_name = match class_index {
                        0 => "normal",
                        1 => "operational",
                        2 => "mandatory",
                        _ => return None,
                    };
                    if let Some((_, class_val)) =
                        named_fields.iter().find(|(name, _)| name == class_name)
                        && let ValueDef::Primitive(scale_value::Primitive::U128(n)) =
                            &class_val.value
                    {
                        return Some(*n as u64);
                    }
                }
            }
            None
        }
        RuntimeMetadata::V15(m) => {
            let registry = &m.types;
            let system_pallet = m.pallets.iter().find(|p| p.name == "System")?;
            let block_length_constant = system_pallet
                .constants
                .iter()
                .find(|c| c.name == "BlockLength")?;

            let mut bytes = &block_length_constant.value[..];
            let decoded = decode_as_type(&mut bytes, block_length_constant.ty.id, registry).ok()?;

            if let ValueDef::Composite(scale_value::Composite::Named(fields)) = &decoded.value
                && let Some((_, max_val)) = fields.iter().find(|(name, _)| name == "max")
            {
                if let ValueDef::Composite(scale_value::Composite::Unnamed(array_fields)) =
                    &max_val.value
                {
                    let fields_vec: Vec<_> = array_fields.iter().collect();
                    if let Some(class_val) = fields_vec.get(class_index)
                        && let ValueDef::Primitive(scale_value::Primitive::U128(n)) =
                            &class_val.value
                    {
                        return Some(*n as u64);
                    }
                } else if let ValueDef::Composite(scale_value::Composite::Named(named_fields)) =
                    &max_val.value
                {
                    let class_name = match class_index {
                        0 => "normal",
                        1 => "operational",
                        2 => "mandatory",
                        _ => return None,
                    };
                    if let Some((_, class_val)) =
                        named_fields.iter().find(|(name, _)| name == class_name)
                        && let ValueDef::Primitive(scale_value::Primitive::U128(n)) =
                            &class_val.value
                    {
                        return Some(*n as u64);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_operational_fee_multiplier(metadata: &RuntimeMetadataPrefixed) -> Option<u128> {
    use frame_metadata::RuntimeMetadata;

    match &metadata.1 {
        RuntimeMetadata::V14(m) => {
            let registry = &m.types;
            let tx_payment_pallet = m.pallets.iter().find(|p| p.name == "TransactionPayment")?;
            let constant = tx_payment_pallet
                .constants
                .iter()
                .find(|c| c.name == "OperationalFeeMultiplier")?;

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
            let constant = tx_payment_pallet
                .constants
                .iter()
                .find(|c| c.name == "OperationalFeeMultiplier")?;

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
    use parity_scale_codec::{Compact, Encode};

    fn real_polkadot_extrinsic_tip_zero() -> String {
        "0xed098400af3e1db41e95040f7630e64d1b3104235c08545e452b15fd70601881aa224b740048ceb5c1995db4427ba1322f48702cebe4b4564e03d660d6a713f25e48143be454875d56716def88a61283643fcb9a0aed7caccbfe285dfba8399b07bc448c063501740001070540000000966d74f8027e07b43717b6876d97544fe0d71facef06acc8382749ae944e00005fa73637062b".to_string()
    }

    fn real_asset_hub_extrinsic_transfer() -> String {
        "0x4902840004316d995f0adb06d918a1fc96077ebdfa93aab9ccf2a8525efd7bf0c1e2282700a24152685f52e4726466e80247d965bb3d349637fc8a1ea6f7cc1451ddec98b5bf30b6e8e31b31f0870ac46f07ccb559402a0fafe90b74127f28e8644281730c00d12b0000000a0000d61e33684a7a41d7233e89955316dbc875fef1428e4f16ec260617dc57de3972078064288004".to_string()
    }

    fn build_extrinsic_with_tip(tip: u128) -> String {
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

    #[test]
    fn test_extract_tip_real_extrinsics() {
        let test_cases = [
            ("Polkadot relay", real_polkadot_extrinsic_tip_zero(), "0"),
            ("Asset Hub", real_asset_hub_extrinsic_transfer(), "0"),
        ];

        for (name, hex, expected_tip) in test_cases {
            let bytes = hex::decode(hex.trim_start_matches("0x")).unwrap();
            let tip = extract_tip_from_extrinsic_bytes(&bytes);
            assert_eq!(tip, Some(expected_tip.to_string()), "Failed for: {}", name);
        }
    }

    #[test]
    fn test_extract_tip_synthetic_various_values() {
        for expected_tip in [1u128, 100, 1000, 1_000_000, u64::MAX as u128, u128::MAX / 2] {
            let extrinsic_hex = build_extrinsic_with_tip(expected_tip);
            let extrinsic_bytes = hex::decode(extrinsic_hex.trim_start_matches("0x")).unwrap();

            let tip = extract_tip_from_extrinsic_bytes(&extrinsic_bytes);
            assert_eq!(
                tip,
                Some(expected_tip.to_string()),
                "Failed for tip: {}",
                expected_tip
            );
        }
    }

    #[test]
    fn test_extract_tip_edge_cases() {
        assert!(extract_tip_from_extrinsic_bytes(&[]).is_none());
        assert!(extract_tip_from_extrinsic_bytes(&[0x00]).is_none());

        let body = vec![0x04, 0x00, 0x00];
        let mut unsigned = Vec::new();
        Compact(body.len() as u32).encode_to(&mut unsigned);
        unsigned.extend(body);
        assert_eq!(
            extract_tip_from_extrinsic_bytes(&unsigned),
            Some("0".to_string())
        );
    }
}
