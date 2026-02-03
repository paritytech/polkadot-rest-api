use crate::state::AppState;
use crate::utils::ResolvedBlock;
use subxt::{OnlineClientAtBlock, SubstrateConfig};
use thiserror::Error;

const ASSET_HUB_PARA_ID: u32 = 1000;
pub type RcClientAtBlock = OnlineClientAtBlock<SubstrateConfig>;

#[derive(Debug, Clone)]
pub struct AhBlockInfo {
    pub hash: String,
    pub number: u64,
}

#[derive(Debug, Error)]
pub enum RcBlockError {
    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[source] Box<subxt::error::OnlineClientAtBlockError>),

    #[error("Failed to fetch storage")]
    StorageFetchFailed(#[from] subxt::error::StorageError),

    #[error("Failed to decode events")]
    EventsDecodeFailed(#[source] scale_decode::Error),

    #[error("Failed to decode events storage value")]
    EventsStorageDecodeFailed(#[from] subxt::error::StorageValueError),

    #[error("Failed to decode header from event data")]
    HeaderDecodeFailed(#[source] parity_scale_codec::Error),

    #[error("Failed to decode paraId from event data")]
    ParaIdDecodeFailed(#[source] parity_scale_codec::Error),

    #[error("Event data missing required fields")]
    EventDataIncomplete,

    #[error("Relay Chain client not available")]
    RelayChainClientNotAvailable,
}

pub async fn find_ah_blocks_in_rc_block(
    state: &AppState,
    rc_block: &ResolvedBlock,
) -> Result<Vec<AhBlockInfo>, RcBlockError> {
    let rc_client = state
        .get_relay_chain_client()
        .ok_or(RcBlockError::RelayChainClientNotAvailable)?;

    let rc_client_at_block = rc_client
        .at_block(rc_block.number)
        .await
        .map_err(|e| RcBlockError::ClientAtBlockFailed(Box::new(e)))?;

    find_ah_blocks_in_rc_block_at(&rc_client_at_block).await
}

/// Find Asset Hub blocks included in a Relay Chain block.
///
/// Uses Subxt ClientAtBlock directly - callers should use `at_block()` to get
/// the client at the desired RC block, then pass it to this function.
/// This avoids an extra RPC call when you already have the ClientAtBlock.
pub async fn find_ah_blocks_in_rc_block_at(
    rc_client_at_block: &RcClientAtBlock,
) -> Result<Vec<AhBlockInfo>, RcBlockError> {
    // Use dynamic storage address for System::Events
    let addr = subxt::dynamic::storage::<(), scale_value::Value>("System", "Events");
    let events_value = rc_client_at_block.storage().fetch(addr, ()).await?;

    let events_decoded: scale_value::Value<()> = events_value.decode_as().map_err(|e| {
        tracing::debug!("Failed to decode events: {:?}", e);
        RcBlockError::EventsStorageDecodeFailed(e)
    })?;

    let mut ah_blocks = Vec::new();

    let events_composite = match &events_decoded.value {
        scale_value::ValueDef::Composite(composite) => composite,
        _ => {
            return Ok(ah_blocks);
        }
    };

    let events_values = match events_composite {
        scale_value::Composite::Unnamed(values) => values,
        scale_value::Composite::Named(_) => {
            return Ok(ah_blocks);
        }
    };

    for event_record in events_values.iter() {
        let record_composite = match &event_record.value {
            scale_value::ValueDef::Composite(c) => c,
            _ => continue,
        };

        let event_value = match record_composite {
            scale_value::Composite::Named(fields) => fields
                .iter()
                .find(|(name, _)| name == "event")
                .map(|(_, v)| v),
            scale_value::Composite::Unnamed(values) => values.get(1),
        };

        let event = match event_value {
            Some(v) => v,
            None => continue,
        };

        let event_variant = match &event.value {
            scale_value::ValueDef::Variant(variant) => variant,
            _ => continue,
        };

        let pallet_name = &event_variant.name;

        if !pallet_name.to_lowercase().contains("parainclusion") {
            continue;
        }

        let (event_name, event_data) = match &event_variant.values {
            scale_value::Composite::Unnamed(values) => {
                let first_val = match values.first() {
                    Some(v) => v,
                    None => continue,
                };
                match &first_val.value {
                    scale_value::ValueDef::Variant(inner_variant) => {
                        (inner_variant.name.clone(), &inner_variant.values)
                    }
                    _ => continue,
                }
            }
            scale_value::Composite::Named(fields) => {
                let (_name, val) = match fields.first() {
                    Some((n, v)) => (n, v),
                    None => continue,
                };
                match &val.value {
                    scale_value::ValueDef::Variant(inner_variant) => {
                        (inner_variant.name.clone(), &inner_variant.values)
                    }
                    _ => continue,
                }
            }
        };

        if event_name != "CandidateIncluded" {
            continue;
        }

        if let Some(ah_block) =
            extract_ah_block_from_candidate_included(event_data, ASSET_HUB_PARA_ID)
        {
            ah_blocks.push(ah_block);
        }
    }

    Ok(ah_blocks)
}

fn extract_ah_block_from_candidate_included(
    event_data: &scale_value::Composite<()>,
    target_para_id: u32,
) -> Option<AhBlockInfo> {
    use sp_runtime::traits::BlakeTwo256;
    use sp_runtime::traits::Hash as HashT;

    let values: Vec<&scale_value::Value<()>> = match event_data {
        scale_value::Composite::Named(fields) => fields.iter().map(|(_, v)| v).collect(),
        scale_value::Composite::Unnamed(values) => values.iter().collect(),
    };

    if values.len() < 2 {
        return None;
    }

    let candidate_receipt = values.first()?;

    let para_id = match extract_para_id_from_candidate_receipt(candidate_receipt) {
        Ok(id) => id,
        Err(_) => return None,
    };

    if para_id != target_para_id {
        return None;
    }

    let head_data = values.get(1)?;

    let header_bytes = match serde_json::to_value(head_data)
        .ok()
        .and_then(|json| extract_bytes_from_json(&json))
    {
        Some(bytes) => bytes,
        None => {
            tracing::debug!("Failed to extract bytes from HeadData");
            return None;
        }
    };

    let block_number = extract_block_number_from_header(&header_bytes)?;

    let block_hash = BlakeTwo256::hash(&header_bytes);
    let block_hash_hex = format!("0x{}", hex::encode(block_hash.as_ref()));

    Some(AhBlockInfo {
        hash: block_hash_hex,
        number: block_number,
    })
}

pub fn extract_bytes_from_json(json: &serde_json::Value) -> Option<Vec<u8>> {
    match json {
        serde_json::Value::Array(arr) => {
            let bytes: Vec<u8> = arr
                .iter()
                .filter_map(|v| v.as_u64().and_then(|n| (n <= 255).then_some(n as u8)))
                .collect();

            if !bytes.is_empty() {
                return Some(bytes);
            }

            if arr.len() == 1 {
                return extract_bytes_from_json(&arr[0]);
            }

            None
        }
        serde_json::Value::String(s) => {
            let hex_clean = s.strip_prefix("0x").unwrap_or(s);
            hex::decode(hex_clean).ok()
        }
        _ => None,
    }
}

pub fn extract_block_number_from_header(header_bytes: &[u8]) -> Option<u64> {
    use parity_scale_codec::Decode;

    if header_bytes.len() < 32 {
        return None;
    }

    let mut cursor = &header_bytes[32..];

    let number_compact = parity_scale_codec::Compact::<u32>::decode(&mut cursor).ok()?;
    Some(number_compact.0 as u64)
}

fn get_field_from_composite<'a>(
    composite: &'a scale_value::Composite<()>,
    field_names: &[&str],
    unnamed_index: Option<usize>,
) -> Option<&'a scale_value::Value<()>> {
    match composite {
        scale_value::Composite::Named(fields) => fields
            .iter()
            .find(|(name, _)| field_names.iter().any(|&n| n == *name))
            .map(|(_, v)| v),
        scale_value::Composite::Unnamed(values) => unnamed_index.and_then(|idx| values.get(idx)),
    }
}

fn extract_para_id_from_candidate_receipt(
    candidate_receipt: &scale_value::Value<()>,
) -> Result<u32, RcBlockError> {
    let receipt_composite = match &candidate_receipt.value {
        scale_value::ValueDef::Composite(c) => c,
        _ => {
            return Err(RcBlockError::ParaIdDecodeFailed(
                parity_scale_codec::Error::from("Candidate receipt is not a composite"),
            ));
        }
    };

    let descriptor_value = get_field_from_composite(receipt_composite, &["descriptor"], Some(0))
        .ok_or_else(|| {
            RcBlockError::ParaIdDecodeFailed(parity_scale_codec::Error::from(
                "descriptor field not found",
            ))
        })?;

    let descriptor_composite = match &descriptor_value.value {
        scale_value::ValueDef::Composite(c) => c,
        _ => {
            return Err(RcBlockError::ParaIdDecodeFailed(
                parity_scale_codec::Error::from("descriptor is not a composite"),
            ));
        }
    };

    let para_id_value =
        get_field_from_composite(descriptor_composite, &["para_id", "paraId"], Some(0))
            .ok_or_else(|| {
                RcBlockError::ParaIdDecodeFailed(parity_scale_codec::Error::from(
                    "para_id field not found",
                ))
            })?;

    let para_id = serde_json::to_value(para_id_value)
        .ok()
        .and_then(|json| {
            json.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .or_else(|| {
                    json.as_array()
                        .and_then(|arr| arr.first())
                        .and_then(|first| first.as_u64())
                        .and_then(|n| u32::try_from(n).ok())
                })
                .or_else(|| {
                    json.as_object()
                        .and_then(|obj| obj.values().next())
                        .and_then(|val| val.as_u64())
                        .and_then(|n| u32::try_from(n).ok())
                })
        })
        .ok_or_else(|| {
            RcBlockError::ParaIdDecodeFailed(parity_scale_codec::Error::from(
                "Could not extract paraId as u32",
            ))
        })?;

    Ok(para_id)
}
